//! Runs Luau source code against one installed Luau version.

use anyhow::{anyhow, bail, Context, Result};
use colored::*;
use std::{
    borrow::Cow,
    env, fs,
    io::{self, Read, Write},
    path::{Path, PathBuf},
    process::{self, Child, Command, ExitStatus, Stdio},
    sync::atomic::{AtomicU64, Ordering},
    thread,
    time::{Duration, Instant},
};

use crate::runner::installer::LuauInstaller;

/// Fixed name so error messages ("script.luau:3: ...") are identical across runs and versions.
const SCRIPT_NAME: &str = "script.luau";
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(5);
/// Per stream. Generated code can print forever; this keeps memory bounded.
const MAX_OUTPUT: usize = 1 << 20; // 1 MiB

/// Makes scratch dir names unique across threads in the same process.
static RUN_COUNTER: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunOutput {
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    /// None if the process was killed (timeout, or a signal on Unix).
    pub exit_code: Option<i32>,
    pub timed_out: bool,
    /// True if either stream exceeded MAX_OUTPUT and was cut off.
    pub truncated: bool,
}

impl RunOutput {
    pub fn stdout_lossy(&self) -> Cow<'_, str> {
        String::from_utf8_lossy(&self.stdout)
    }

    pub fn stderr_lossy(&self) -> Cow<'_, str> {
        String::from_utf8_lossy(&self.stderr)
    }
}

pub struct Runner {
    installer: LuauInstaller,
    release: String,
    path: PathBuf,
    flags: Vec<String>,
    timeout: Duration,
}

impl Runner {
    /// Uses the newest installed release, or asks which one to install.
    pub fn new() -> Result<Self> {
        let installer = LuauInstaller::project_local()?;
        let release = choose_release(&installer)?;
        Self::build(installer, &release)
    }

    /// Uses a specific release, installing it if needed. Never prompts.
    pub fn for_release(release: &str) -> Result<Self> {
        let installer = LuauInstaller::project_local()?;
        Self::build(installer, release)
    }

    fn build(installer: LuauInstaller, release: &str) -> Result<Self> {
        // Returns immediately for an installed release; downloads otherwise.
        let path = installer
            .install_release(release)
            .with_context(|| format!("failed to install Luau {release}"))?;

        let runner = Self {
            installer,
            release: release.to_string(),
            path,
            flags: Vec::new(),
            timeout: DEFAULT_TIMEOUT,
        };
        runner.sanity_check()?;
        Ok(runner)
    }

    /// Extra CLI flags passed before the script, e.g. ["-O2"] or ["--codegen"].
    pub fn with_flags<I, S>(mut self, flags: I) -> Result<Self>
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.flags = flags.into_iter().map(Into::into).collect();
        self.sanity_check()?; // catch unsupported flags now, not mid-fuzz
        Ok(self)
    }
    
    pub fn with_o0(self) -> Result<Self> {
        Ok(self.with_flags(["-O0"])?)
    }

    pub fn with_o2(self) -> Result<Self> {
        Ok(self.with_flags(["-O2"])?)
    }

    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    pub fn release(&self) -> &str {
        &self.release
    }

    pub fn flags(&self) -> &[String] {
        &self.flags
    }

    pub fn binary(&self) -> &Path {
        &self.path
    }

    /// Switch this runner to a different Luau version.
    pub fn install_release(&mut self, release: &str) -> Result<()> {
        let path = self
            .installer
            .install_release(release)
            .with_context(|| format!("failed to install Luau {release}"))?;
        let old = std::mem::replace(&mut self.path, path);
        let old_release = std::mem::replace(&mut self.release, release.to_string());

        if let Err(e) = self.sanity_check() {
            // Keep the runner usable on the previous version.
            self.path = old;
            self.release = old_release;
            return Err(e);
        }
        Ok(())
    }

    /// Runs `code` and captures what Luau did.
    ///
    /// `Err` means the harness failed (couldn't write the script, couldn't spawn luau).
    /// A script that errors, crashes, or times out is still `Ok`; that's a result to compare.
    pub fn run(&self, code: &str) -> Result<RunOutput> {
        let scratch = ScratchDir::new().context("failed to create scratch dir")?;
        fs::write(scratch.0.join(SCRIPT_NAME), code).context("failed to write script")?;

        let mut child = Command::new(&self.path)
            .args(&self.flags)
            .arg(SCRIPT_NAME)
            .current_dir(&scratch.0)
            .stdin(Stdio::null()) // io.read-style calls get EOF instead of hanging
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .with_context(|| format!("failed to spawn {}", self.path.display()))?;

        // Drain both pipes on their own threads. If luau fills a pipe buffer while
        // nobody is reading it, it blocks forever and looks like a timeout.
        let stdout = child.stdout.take().context("stdout was not captured")?;
        let stderr = child.stderr.take().context("stderr was not captured")?;
        let out_reader = thread::spawn(move || read_capped(stdout, MAX_OUTPUT));
        let err_reader = thread::spawn(move || read_capped(stderr, MAX_OUTPUT));

        let (status, timed_out) = wait_with_timeout(&mut child, self.timeout)?;

        let (stdout, out_truncated) = join_reader(out_reader)?;
        let (stderr, err_truncated) = join_reader(err_reader)?;

        Ok(RunOutput {
            stdout,
            stderr,
            // A killed process still reports a code on Windows (1); don't let that
            // look like a normal exit.
            exit_code: if timed_out { None } else { status.code() },
            timed_out,
            truncated: out_truncated || err_truncated,
        })
        // `scratch` is dropped here and the directory is deleted.
    }

    /// Fails if this binary + flags can't run a trivial script correctly.
    fn sanity_check(&self) -> Result<()> {
        let probe = self.run("print(1 + 1)")?;
        if probe.stdout.trim_ascii() != b"2" {
            bail!(
                "luau {} {:?} failed a sanity check.\nstdout: {}\nstderr: {}",
                self.release,
                self.flags,
                probe.stdout_lossy(),
                probe.stderr_lossy()
            );
        }
        Ok(())
    }
}

/// A temp directory that deletes itself when dropped, including on early `?` returns.
struct ScratchDir(PathBuf);

impl ScratchDir {
    fn new() -> io::Result<Self> {
        let path = env::temp_dir().join(format!(
            "luau-fuzz-{}-{}",
            process::id(),
            RUN_COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&path); // leftover from an old crashed run with the same PID
        fs::create_dir_all(&path)?;
        Ok(Self(path))
    }
}

impl Drop for ScratchDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// Uses the newest installed release, or asks which one to install.
fn choose_release(installer: &LuauInstaller) -> Result<String> {
    if let Some(newest) = installer.installed_releases()?.pop() {
        return Ok(newest);
    }

    let answer = prompt(&format!(
        "No {} release found in {}. Install one? (Y/N) ",
        "luau".blue(),
        installer.root().display()
    ))?;
    if !matches!(answer.to_lowercase().as_str(), "y" | "yes") {
        bail!("unable to run differential fuzzer without a luau binary");
    }

    let release = prompt(&format!("{} release to install (e.g. 0.650): ", "luau".blue()))?;
    if release.is_empty() {
        bail!("no release given");
    }
    Ok(release)
}

/// Prints `msg`, then reads one trimmed line from stdin.
fn prompt(msg: &str) -> Result<String> {
    print!("{msg}");
    io::stdout().flush()?;
    let mut line = String::new();
    if io::stdin().read_line(&mut line)? == 0 {
        bail!("stdin closed while waiting for input");
    }
    Ok(line.trim().to_string())
}

/// Reads up to `cap` bytes, then keeps draining (and discarding) so the child never blocks.
fn read_capped(mut r: impl Read, cap: usize) -> io::Result<(Vec<u8>, bool)> {
    let mut buf = Vec::new();
    r.by_ref().take(cap as u64).read_to_end(&mut buf)?;
    let extra = io::copy(&mut r, &mut io::sink())?;
    Ok((buf, extra > 0))
}

fn join_reader(
    handle: thread::JoinHandle<io::Result<(Vec<u8>, bool)>>,
) -> Result<(Vec<u8>, bool)> {
    handle
        .join()
        // A thread panic payload isn't a std Error, so `.context()` can't wrap it.
        .map_err(|_| anyhow!("output reader thread panicked"))?
        .context("failed to read luau output")
}

/// Returns (status, timed_out). Kills the child if the deadline passes.
fn wait_with_timeout(child: &mut Child, timeout: Duration) -> Result<(ExitStatus, bool)> {
    let deadline = Instant::now() + timeout;
    let mut nap = Duration::from_micros(200);

    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Ok((status, false)),
            Ok(None) => {}
            Err(e) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(e).context("failed to poll luau process");
            }
        }

        let now = Instant::now();
        if now >= deadline {
            // kill() can fail if luau exited a moment ago; wait() handles both cases.
            let _ = child.kill();
            let status = child.wait().context("failed to reap killed luau process")?;
            return Ok((status, true));
        }

        // Short naps first so fast scripts return quickly, backing off to 20ms.
        thread::sleep(nap.min(deadline - now));
        nap = (nap * 2).min(Duration::from_millis(5));
    }
}