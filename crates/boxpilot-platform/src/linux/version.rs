//! Linux `VersionChecker` impl. Spawns `<binary> version` synchronously and
//! validates the stdout starts with the expected `sing-box version` prefix.

use crate::traits::version::VersionChecker;
use std::io;
use std::path::Path;

pub struct ProcessVersionChecker;

impl VersionChecker for ProcessVersionChecker {
    fn check(&self, binary: &Path) -> io::Result<String> {
        let out = std::process::Command::new(binary)
            .arg("version")
            .output()
            .map_err(|e| io::Error::other(format!("spawn: {e}")))?;
        if !out.status.success() {
            return Err(io::Error::other(format!(
                "exit {:?}: {}",
                out.status.code(),
                String::from_utf8_lossy(&out.stderr).trim()
            )));
        }
        let stdout = String::from_utf8_lossy(&out.stdout).to_string();
        if !stdout.contains("sing-box version") {
            return Err(io::Error::other(format!(
                "unexpected stdout: {}",
                stdout.lines().next().unwrap_or("")
            )));
        }
        Ok(stdout)
    }
}
