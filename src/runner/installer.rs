//! Downloads prebuilt Luau releases from GitHub into `bin/luau/<release>/`.
//!
//! Layout:
//!   bin/luau/
//!     0.650/luau.exe
//!     0.651/luau.exe

use anyhow::{bail, Context, Result};
use serde::Deserialize;
use std::{
    env,
    ffi::OsStr,
    fs,
    io::self,
    io::Cursor,
    path::{self, Path, PathBuf},
    process::Command,
    sync::atomic::{AtomicU64, Ordering},
};

const REPO: &str = "luau-lang/luau";

#[cfg(windows)]
const LUAU_EXE: &str = "luau.exe";
#[cfg(not(windows))]
const LUAU_EXE: &str = "luau";

/// Makes temp dir names unique across threads in the same process.
static TMP_COUNTER: AtomicU64 = AtomicU64::new(0);

#[derive(Deserialize)]
struct Release {
    tag_name: String,
    assets: Vec<Asset>,
}

#[derive(Deserialize)]
struct Asset {
    name: String,
    browser_download_url: String,
}

pub struct LuauInstaller {
    root: PathBuf,
    http: reqwest::blocking::Client,
}

impl LuauInstaller {
    /// Installs into `<root>/<release>/`.
    pub fn new(root: impl Into<PathBuf>) -> Result<Self> {
        let http = reqwest::blocking::Client::builder()
            .user_agent("luau-differential-fuzzer") // GitHub API rejects requests without one
            .build()
            .context("failed to build HTTP client")?;
        Ok(Self {
            root: root.into(),
            http,
        })
    }

    /// Installs into `<project>/bin/luau/<release>/`, independent of the working directory.
    pub fn project_local() -> Result<Self> {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("bin").join("luau");
        Self::new(root)
    }

    /// Installed releases, oldest to newest.
    pub fn installed_releases(&self) -> Result<Vec<String>> {
        let entries = match fs::read_dir(&self.root) {
            Ok(entries) => entries,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => {
                return Err(e).with_context(|| format!("failed to read {}", self.root.display()))
            }
        };

        let mut releases = Vec::new();
        for entry in entries {
            let name = entry?.file_name();
            let Some(name) = name.to_str() else { continue };
            if validate_tag(name).is_ok() && self.root.join(name).join(LUAU_EXE).is_file() {
                releases.push(name.to_string());
            }
        }

        // Numeric sort: a plain string sort would put "0.99" after "0.650".
        releases.sort_by_key(|r| {
            r.split('.')
                .map(|p| p.parse::<u64>().unwrap_or(0))
                .collect::<Vec<_>>()
        });
        Ok(releases)
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Where the binary for `release` lives (or would live once installed).
    pub fn binary_path(&self, release: &str) -> Result<PathBuf> {
        validate_tag(release)?;
        Ok(self.root.join(release).join(LUAU_EXE))
    }

    pub fn is_installed(&self, release: &str) -> Result<bool> {
        Ok(self.binary_path(release)?.is_file())
    }

    /// Downloads and extracts `release` if it is not already cached.
    /// Returns the absolute or root-relative path to the `luau` executable.
    pub fn install_release(&self, release: &str) -> Result<PathBuf> {
        let bin = self.binary_path(release)?;
        if bin.is_file() {
            return Ok(bin);
        }
        let dest = self.root.join(release);

        let rel = self.fetch_release(release)?;
        let want = platform_asset()?;
        let asset = rel
            .assets
            .iter()
            .find(|a| a.name == want)
            .with_context(|| format!("release {} has no asset named {want}", rel.tag_name))?;

        let bytes = self
            .http
            .get(&asset.browser_download_url)
            .send()
            .and_then(|r| r.error_for_status())
            .with_context(|| format!("failed to download {}", asset.browser_download_url))?
            .bytes()?;

        // Extract into a temp dir, then rename, so a crash never leaves a half-installed version.
        fs::create_dir_all(&self.root)
            .with_context(|| format!("failed to create {}", self.root.display()))?;
        let tmp = self.root.join(format!(
            ".{release}.{}.{}.tmp",
            std::process::id(),
            TMP_COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&tmp);

        if let Err(e) = extract_into(&bytes, &tmp) {
            let _ = fs::remove_dir_all(&tmp);
            return Err(e);
        }

        // A version dir without a binary is stale (manual deletion, old layout). Rename is
        // atomic, so a complete install from another worker always has the binary present.
        if dest.exists() && !bin.is_file() {
            let _ = fs::remove_dir_all(&dest);
        }

        if let Err(e) = fs::rename(&tmp, &dest) {
            let _ = fs::remove_dir_all(&tmp);
            if !bin.is_file() {
                return Err(e).with_context(|| {
                    format!("failed to move {} to {}", tmp.display(), dest.display())
                });
            }
            // Another worker installed this version first; that's fine.
        }

        Ok(bin)
    }

    /// Installs `release` if needed, then returns a Command for `program`
    /// whose PATH resolves `luau` to that version.
    ///
    /// On Windows, do not pass "luau" as `program`; use the path from
    /// `install_release` to launch luau itself.
    pub fn command(&self, release: &str, program: impl AsRef<OsStr>) -> Result<Command> {
        let bin = self.install_release(release)?;
        let mut cmd = Command::new(program);
        prepend_to_path(&mut cmd, &bin)?;
        Ok(cmd)
    }

    fn fetch_release(&self, release: &str) -> Result<Release> {
        let url = format!("https://api.github.com/repos/{REPO}/releases/tags/{release}");
        let mut req = self.http.get(&url);
        if let Ok(tok) = env::var("GITHUB_TOKEN") {
            req = req.bearer_auth(tok);
        }
        req.send()
            .and_then(|r| r.error_for_status())
            .with_context(|| format!("failed to fetch release metadata for {release}"))?
            .json()
            .context("failed to parse release metadata")
    }
}

/// Tags look like "0.650". Rejects anything that could escape the install root.
fn validate_tag(release: &str) -> Result<()> {
    let ok = release
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_alphanumeric())
        && release
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '.')
        && !release.contains("..");
    if !ok {
        bail!("invalid release tag: {release:?}");
    }
    Ok(())
}

fn platform_asset() -> Result<&'static str> {
    Ok(match env::consts::OS {
        "linux" => "luau-ubuntu.zip",
        "macos" => "luau-macos.zip",
        "windows" => "luau-windows.zip",
        os => bail!("unsupported OS: {os}"),
    })
}

fn extract_into(bytes: &[u8], dir: &Path) -> Result<()> {
    fs::create_dir_all(dir)?;
    zip::ZipArchive::new(Cursor::new(bytes))
        .context("downloaded asset is not a valid zip")?
        .extract(dir)
        .context("failed to extract archive")?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        for entry in fs::read_dir(dir)? {
            let p = entry?.path();
            if p.is_file() {
                fs::set_permissions(&p, fs::Permissions::from_mode(0o755))?;
            }
        }
    }

    if !dir.join(LUAU_EXE).is_file() {
        bail!(
            "archive extracted but {LUAU_EXE} is not at its root; the release layout may have changed"
        );
    }
    Ok(())
}

fn prepend_to_path(cmd: &mut Command, bin: &Path) -> Result<()> {
    let dir = bin.parent().context("binary has no parent dir")?;
    let dir = path::absolute(dir)?;

    let mut paths = vec![dir];
    if let Some(existing) = env::var_os("PATH") {
        paths.extend(env::split_paths(&existing));
    }
    let joined = env::join_paths(paths)
        .context("install dir contains a PATH separator (':' or ';')")?;
    cmd.env("PATH", joined);
    Ok(())
}