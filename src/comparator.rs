//! Compares result oracles to find differences.

use crate::runner::run::RunOutput;

pub enum DiffType {
    Stdout, Stderr, ExitCode, TimedOut
}

pub struct Comparator {}

impl Comparator {
    /// Returns a `Vec` of observed differences between oracles a and b.
    fn compare(a: &RunOutput, b: &RunOutput) -> Option<Vec<DiffType>> {
        let mut diffs: Vec<DiffType> = Vec::new();
        let mut found = false;
        if a.stdout != b.stdout {
            diffs.push(DiffType::Stdout);
            found = true;
        }
        if a.stderr != b.stderr {
            diffs.push(DiffType::Stderr);
            found = true;
        }
        if a.exit_code != b.exit_code {
            diffs.push(DiffType::ExitCode);
            found = true;
        }
        if a.timed_out != b.timed_out {
            diffs.push(DiffType::TimedOut);
            found = true;
        }
        if found {
            return Some(diffs);
        }
        None
    }
}