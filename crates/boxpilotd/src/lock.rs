//! Re-export shell. Production impl on Linux:
//! `boxpilot_platform::linux::lock::FlockFileLock`. Production impl on
//! Windows: `boxpilot_platform::windows::lock::LockFileExLock`. The
//! free-function `try_acquire(&path)` is a convenience wrapper preserving
//! the existing call signature used across boxpilotd's dispatch / mutating
//! verbs.

#[cfg(target_os = "linux")]
pub use boxpilot_platform::linux::lock::{FlockFileLock, LockGuard};
#[cfg(target_os = "windows")]
pub use boxpilot_platform::windows::lock::{LockFileExLock, LockGuard};

pub use boxpilot_platform::traits::lock::FileLock;

use boxpilot_ipc::HelperError;
use std::path::Path;

pub fn try_acquire(path: &Path) -> Result<LockGuard, HelperError> {
    #[cfg(target_os = "linux")]
    return boxpilot_platform::linux::lock::FlockFileLock.try_acquire(path);
    #[cfg(target_os = "windows")]
    return boxpilot_platform::windows::lock::LockFileExLock.try_acquire(path);
    #[cfg(not(any(target_os = "linux", target_os = "windows")))]
    Err(HelperError::Ipc {
        message: "locking not supported on this platform".into(),
    })
}
