//! Global advisory lock on `/run/boxpilot/lock`. Held for any privileged
//! mutating operation (spec §6.4). `/run` is tmpfs and is cleared on reboot,
//! so a stale lock cannot survive a crash-restart.

use boxpilot_ipc::HelperError;
use fs2::FileExt;
use std::fs::{File, OpenOptions};
use std::io::ErrorKind;
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;

/// RAII guard. The flock is released on drop.
pub struct LockGuard {
    file: File,
}

impl Drop for LockGuard {
    fn drop(&mut self) {
        // Best-effort unlock; if it fails the kernel will release on close.
        // Use the fully-qualified path to avoid ambiguity with the future
        // std::fs::File::unlock() method (stabilized in Rust 1.89).
        let _ = fs2::FileExt::unlock(&self.file);
    }
}

/// Try to acquire the advisory lock. Returns [`HelperError::Busy`] if another
/// holder is present. The parent directory is created if missing.
pub fn try_acquire(lock_path: &Path) -> Result<LockGuard, HelperError> {
    if let Some(parent) = lock_path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| HelperError::Ipc {
            message: format!("create {parent:?}: {e}"),
        })?;
    }
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o644)
        .open(lock_path)
        .map_err(|e| HelperError::Ipc { message: format!("open lock: {e}") })?;
    match file.try_lock_exclusive() {
        Ok(()) => Ok(LockGuard { file }),
        Err(e) if e.kind() == ErrorKind::WouldBlock => Err(HelperError::Busy),
        Err(e) => Err(HelperError::Ipc { message: format!("flock: {e}") }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn acquires_when_unheld() {
        let dir = tempdir().unwrap();
        let lock = dir.path().join("lock");
        let _g = try_acquire(&lock).unwrap();
        assert!(lock.exists());
    }

    #[test]
    fn second_concurrent_acquire_returns_busy() {
        let dir = tempdir().unwrap();
        let lock = dir.path().join("lock");
        let _g1 = try_acquire(&lock).unwrap();
        let r2 = try_acquire(&lock);
        assert!(matches!(r2, Err(HelperError::Busy)));
    }

    #[test]
    fn dropping_guard_releases_lock() {
        let dir = tempdir().unwrap();
        let lock = dir.path().join("lock");
        {
            let _g1 = try_acquire(&lock).unwrap();
        }
        let _g2 = try_acquire(&lock).expect("lock should be free after drop");
    }
}
