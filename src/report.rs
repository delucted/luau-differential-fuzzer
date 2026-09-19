//! Writes one markdown file per observed discrepancy.

use crate::runner::run::RunOutput;
use crate::comparator::DiffType;
use anyhow::{Context, Result};
use std::fs;
use std::path::PathBuf;
use uuid::Uuid;

/// Where reports land, relative to the working directory.
const DIFF_DIR: &str = "diffs";
/// Per stream, per report. A run can produce a megabyte; nobody reads that.
const MAX_QUOTED: usize = 8 * 1024;
/// A table cell has to stay on one line.
const MAX_CELL: usize = 120;

pub struct Report {}

impl Report {
    fn construct(code: &str, a: &RunOutput, b: &RunOutput, diffs: &[DiffType], id: &Uuid) -> String {
        let mut table = String::new();
        let mut details = String::new();

        for diff in diffs {
            match diff {
                DiffType::Stdout => {
                    Self::stream_rows("stdout", &a.stdout_lossy(), &b.stdout_lossy(),
                        a.stdout.len(), b.stdout.len(), &mut table, &mut details);
                },
                DiffType::Stderr => {
                    Self::stream_rows("stderr", &a.stderr_lossy(), &b.stderr_lossy(),
                        a.stderr.len(), b.stderr.len(), &mut table, &mut details);
                },
                DiffType::ExitCode => {
                    table.push_str(&format!("| exit code | {:?} | {:?} |\n", a.exit_code, b.exit_code));
                },
                DiffType::TimedOut => {
                    table.push_str(&format!("| timed out | {} | {} |\n", a.timed_out, b.timed_out));
                }
            }
        }

        // a cut-off stream can differ for no better reason than where the cut
        // landed, so say so rather than letting it look like a real difference
        if a.truncated || b.truncated {
            table.push_str(&format!("| output truncated | {} | {} |\n", a.truncated, b.truncated));
        }

        format!(
"# Diff {id}

## Observed discrepancy

| | O0 | O2 |
|---|---|---|
{table}
{details}## Source

````luau
{code}
````
"
        )
    }

    /// One summary row per stream, plus the full text below the table. Output
    /// runs to many lines, and a markdown cell cannot hold that.
    fn stream_rows(
        name: &str,
        a: &str,
        b: &str,
        a_len: usize,
        b_len: usize,
        table: &mut String,
        details: &mut String,
    ) {
        table.push_str(&format!("| {name} | {a_len} bytes | {b_len} bytes |\n"));
        if let Some((line, left, right)) = first_diff_line(a, b) {
            table.push_str(&format!(
                "| {name} line {line} | `{}` | `{}` |\n",
                cell(&left),
                cell(&right)
            ));
        }
        details.push_str(&format!(
            "### {name}\n\nO0:\n\n````\n{}\n````\n\nO2:\n\n````\n{}\n````\n\n",
            quote(a),
            quote(b)
        ));
    }

    /// Returns where the report was written, so the caller can say so.
    pub fn report(code: &str, a: &RunOutput, b: &RunOutput, diffs: Vec<DiffType>) -> Result<PathBuf> {
        let dir = PathBuf::from(DIFF_DIR);
        fs::create_dir_all(&dir)
            .with_context(|| format!("failed to create {}", dir.display()))?;

        let id = Self::gen_id();
        let path = dir.join(format!("{id}.md"));
        fs::write(&path, Self::construct(code, a, b, &diffs, &id))
            .with_context(|| format!("failed to write {}", path.display()))?;

        // absolute, but without the \\?\ prefix canonicalize adds on Windows
        Ok(std::env::current_dir().map(|cwd| cwd.join(&path)).unwrap_or(path))
    }

    fn gen_id() -> Uuid {
        Uuid::new_v4()
    }
}

/// The first line the two streams disagree on, which is usually the whole story.
fn first_diff_line(a: &str, b: &str) -> Option<(usize, String, String)> {
    let mut left = a.lines();
    let mut right = b.lines();
    let mut n = 0;
    loop {
        n += 1;
        let (l, r) = (left.next(), right.next());
        if l == r {
            l?; // both ran out: the streams differ only in trailing bytes
            continue;
        }
        return Some((
            n,
            l.unwrap_or("<no more output>").to_string(),
            r.unwrap_or("<no more output>").to_string(),
        ));
    }
}

/// Largest cut at or below `max` that does not split a character. Lossy output
/// is full of multi-byte replacement characters, and slicing one in half panics.
fn floor_boundary(s: &str, max: usize) -> usize {
    let mut i = max.min(s.len());
    while i > 0 && !s.is_char_boundary(i) {
        i -= 1;
    }
    i
}

fn cell(line: &str) -> String {
    let escaped = line.replace('|', "\\|");
    if escaped.len() <= MAX_CELL {
        return escaped;
    }
    let cut = floor_boundary(&escaped, MAX_CELL);
    format!("{}… (+{} bytes)", &escaped[..cut], escaped.len() - cut)
}

fn quote(stream: &str) -> String {
    if stream.len() <= MAX_QUOTED {
        return stream.to_string();
    }
    let cut = floor_boundary(stream, MAX_QUOTED);
    format!("{}\n… cut, {} bytes total", &stream[..cut], stream.len())
}

