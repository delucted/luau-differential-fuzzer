use std::process::Command;
use std::io::ErrorKind;

/// Spawns a process to test if a binary is installed in PATH.
pub fn command_exists(cmd: &str) -> bool {
    match Command::new(cmd).spawn() {
        Ok(mut child) => {
            let _ = child.kill();
            true
        },
        Err(e) => {
            if e.kind() == ErrorKind::NotFound {
                return false
            }
            true
        }
    }
}