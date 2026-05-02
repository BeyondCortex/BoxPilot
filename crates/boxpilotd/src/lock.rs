//! Re-export shell. Production impl in
//! `boxpilot_platform::linux::lock::FlockFileLock`. The free-function
//! `try_acquire(&path)` is a convenience wrapper preserving the existing
//! call signature used across boxpilotd's dispatch / mutating verbs.

pub use boxpilot_platform::linux::lock::{FlockFileLock, LockGuard};
pub use boxpilot_platform::traits::lock::FileLock;

use boxpilot_ipc::HelperError;
use std::path::Path;

pub fn try_acquire(path: &Path) -> Result<LockGuard, HelperError> {
    FlockFileLock.try_acquire(path)
}
