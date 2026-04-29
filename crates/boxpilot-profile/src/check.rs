use std::path::Path;
use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckOutput {
    pub success: bool,
    pub stdout: String,
    pub stderr: String,
}

#[derive(Debug, thiserror::Error)]
pub enum CheckError {
    #[error("could not spawn core at {0}: {1}")]
    Spawn(std::path::PathBuf, std::io::Error),
    #[error("check timed out after {0:?}")]
    Timeout(Duration),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

pub const CHECK_TIMEOUT: Duration = Duration::from_secs(5);

/// Runs `<core_path> check -c config.json` from `working_dir`.
///
/// `config.json` (and any referenced relative assets) must exist under
/// `working_dir`. Caller is responsible for permissions: `core_path` is
/// expected to be world-executable (managed cores live under
/// `/var/lib/boxpilot/cores/` with `0755`).
pub fn run_singbox_check(core_path: &Path, working_dir: &Path) -> Result<CheckOutput, CheckError> {
    use std::process::{Command, Stdio};
    let mut child = Command::new(core_path)
        .args(["check", "-c", "config.json"])
        .current_dir(working_dir)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| CheckError::Spawn(core_path.to_path_buf(), e))?;

    let start = std::time::Instant::now();
    loop {
        match child.try_wait()? {
            Some(status) => {
                let mut stdout = String::new();
                let mut stderr = String::new();
                if let Some(mut s) = child.stdout.take() {
                    use std::io::Read;
                    let _ = s.read_to_string(&mut stdout);
                }
                if let Some(mut s) = child.stderr.take() {
                    use std::io::Read;
                    let _ = s.read_to_string(&mut stderr);
                }
                return Ok(CheckOutput { success: status.success(), stdout, stderr });
            }
            None => {
                if start.elapsed() >= CHECK_TIMEOUT {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(CheckError::Timeout(CHECK_TIMEOUT));
                }
                std::thread::sleep(Duration::from_millis(50));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_executable(path: &Path, body: &str) {
        use std::os::unix::fs::PermissionsExt;
        std::fs::write(path, body).unwrap();
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    #[test]
    fn success_case_returns_success_true() {
        let tmp = tempfile::tempdir().unwrap();
        let fake_core = tmp.path().join("fake-core");
        write_executable(&fake_core, "#!/bin/sh\necho ok\nexit 0\n");
        let out = run_singbox_check(&fake_core, tmp.path()).unwrap();
        assert!(out.success);
        assert!(out.stdout.contains("ok"));
    }

    #[test]
    fn failure_case_returns_success_false_and_stderr() {
        let tmp = tempfile::tempdir().unwrap();
        let fake_core = tmp.path().join("fake-core");
        write_executable(&fake_core, "#!/bin/sh\necho boom 1>&2\nexit 1\n");
        let out = run_singbox_check(&fake_core, tmp.path()).unwrap();
        assert!(!out.success);
        assert!(out.stderr.contains("boom"));
    }

    #[test]
    fn missing_core_returns_spawn_error() {
        let tmp = tempfile::tempdir().unwrap();
        let err = run_singbox_check(&tmp.path().join("nope"), tmp.path()).unwrap_err();
        assert!(matches!(err, CheckError::Spawn(..)));
    }

    #[test]
    fn timeout_kills_hung_process_and_returns_error() {
        let tmp = tempfile::tempdir().unwrap();
        let fake_core = tmp.path().join("hung-core");
        write_executable(&fake_core, "#!/bin/sh\nsleep 30\n");
        let start = std::time::Instant::now();
        let err = run_singbox_check(&fake_core, tmp.path()).unwrap_err();
        let elapsed = start.elapsed();
        assert!(matches!(err, CheckError::Timeout(_)));
        // We must have killed it inside CHECK_TIMEOUT (with a small slack).
        assert!(
            elapsed < CHECK_TIMEOUT + std::time::Duration::from_secs(2),
            "timeout took {:?}, expected near {CHECK_TIMEOUT:?}",
            elapsed,
        );
    }
}
