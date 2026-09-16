use crate::runner::run::RunOutput;
use crate::comparator::DiffType;
use anyhow::Result;
use std::fs;
use uuid::Uuid;

pub struct Report {}

impl Report {
    fn construct(code: &str, a: &RunOutput, b: &RunOutput, diffs: Vec<DiffType>, id: &Uuid) -> String {
        let mut diff_table = String::new();
        for diff in diffs {
            match diff {
                DiffType::Stdout => diff_table.push_str(
                    &format!(
                        "stdout | {} | {}",
                        String::from_utf8(a.stdout.clone()).unwrap(),
                        String::from_utf8(b.stdout.clone()).unwrap()
                    )
                ),
                DiffType::Stderr => diff_table.push_str(
                    &format!(
                        "stderr | {} | {}",
                        String::from_utf8(a.stderr.clone()).unwrap(),
                        String::from_utf8(b.stderr.clone()).unwrap()
                    )
                ),
                DiffType::ExitCode => diff_table.push_str(
                    &format!(
                        "exitcode | {:?} | {:?}",
                        a.exit_code,
                        b.exit_code
                    )
                ),
                DiffType::TimedOut => diff_table.push_str(
                    &format!(
                        "timedout | {} | {}",
                        a.timed_out,
                        b.timed_out
                    )
                )
            }
        }
        format!("# Diff {}
## Observed Discrepancy
| | O0 | O2 |
|---|---|---|
{}
## Source
```lua
{}
```",
        id,
        diff_table,
            code
        )
    }
    pub fn report(code: &str, a: &RunOutput, b: &RunOutput, diffs: Vec<DiffType>) -> Result<()> {
        let id = Uuid::new_v4();
        fs::write(format!("../diffs/{}.md", id), Self::construct(code, a, b, diffs, &id))?;
        Ok(())
    }
}