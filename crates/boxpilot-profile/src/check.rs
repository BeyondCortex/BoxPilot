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
#[cfg(target_os = "linux")]
pub fn run_singbox_check(core_path: &Path, working_dir: &Path) -> Result<CheckOutput, CheckError> {
    use std::io::Read;
    use std::os::unix::process::CommandExt;
    use std::process::{Command, Stdio};
    use std::thread;

    let mut child = Command::new(core_path)
        .args(["check", "-c", "config.json"])
        .current_dir(working_dir)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        // Place the child in its own process group (pgid = child pid). This
        // lets us SIGKILL the entire group (child + any grandchildren it
        // spawns, e.g. a shell script forking `sleep`) in one call, which
        // ensures the pipe write-ends are closed promptly on timeout.
        .process_group(0)
        .spawn()
        .map_err(|e| CheckError::Spawn(core_path.to_path_buf(), e))?;

    let pgid = child.id() as i32;

    // Drain pipes concurrently to avoid deadlock when output exceeds
    // the pipe buffer (~64 KiB on Linux). Threads exit when the child
    // closes its end of the pipe.
    let stdout_pipe = child.stdout.take().expect("stdout was piped");
    let stderr_pipe = child.stderr.take().expect("stderr was piped");
    let stdout_handle = thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = std::io::BufReader::new(stdout_pipe).read_to_end(&mut buf);
        buf
    });
    let stderr_handle = thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = std::io::BufReader::new(stderr_pipe).read_to_end(&mut buf);
        buf
    });

    let start = std::time::Instant::now();
    let status = loop {
        match child.try_wait()? {
            Some(s) => break s,
            None => {
                if start.elapsed() >= CHECK_TIMEOUT {
                    // Kill the entire process group so that any grandchildren
                    // (e.g. a shell script forking a subprocess) are also
                    // terminated. This closes all write-ends of the pipes,
                    // allowing the drain threads to finish promptly.
                    unsafe { libc::kill(-pgid, libc::SIGKILL) };
                    let _ = child.wait();
                    let _ = stdout_handle.join();
                    let _ = stderr_handle.join();
                    return Err(CheckError::Timeout(CHECK_TIMEOUT));
                }
                std::thread::sleep(Duration::from_millis(50));
            }
        }
    };

    let stdout_bytes = stdout_handle.join().unwrap_or_default();
    let stderr_bytes = stderr_handle.join().unwrap_or_default();

    Ok(CheckOutput {
        success: status.success(),
        stdout: String::from_utf8_lossy(&stdout_bytes).into_owned(),
        stderr: String::from_utf8_lossy(&stderr_bytes).into_owned(),
    })
}

/// Per COQ14: sing-box check on Windows is short-circuited in
/// Sub-project #1. The real JobObject-based implementation arrives in
/// Sub-project #2; until then we return a successful CheckOutput so the
/// activation pipeline does not block on this step.
#[cfg(target_os = "windows")]
pub fn run_singbox_check(
    _core_path: &Path,
    _working_dir: &Path,
) -> Result<CheckOutput, CheckError> {
    Ok(CheckOutput {
        success: true,
        stdout: "sing-box check skipped on Windows in Sub-project #1".to_string(),
        stderr: String::new(),
    })
}

#[cfg(all(test, target_os = "linux"))]
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
    fn handles_large_stdout_without_deadlock() {
        let tmp = tempfile::tempdir().unwrap();
        let fake_core = tmp.path().join("noisy-core");
        // ~600 KiB on stdout, well past 64 KiB pipe buffer.
        write_executable(&fake_core, "#!/bin/sh\nseq 1 100000\nexit 0\n");
        let out = run_singbox_check(&fake_core, tmp.path()).unwrap();
        assert!(out.success);
        // Each line of seq 1..=100000 contributes 1-6 bytes + newline; 600KB+ guaranteed.
        assert!(
            out.stdout.len() > 64 * 1024,
            "expected stdout > 64 KiB, got {} bytes",
            out.stdout.len()
        );
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
