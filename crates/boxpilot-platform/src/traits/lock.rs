//! Global advisory lock used by every mutating helper verb. Linux uses
//! `flock(2)` on `/run/boxpilot/lock` (tmpfs auto-clears on reboot).
//! Windows uses `LockFileEx` on `%ProgramData%\BoxPilot\run\lock` —
//! Windows handle scoping is inherently process-bounded so a crashed
//! helper releases its lock automatically too.

use boxpilot_ipc::HelperError;
use std::path::Path;

pub trait FileLock: Send + Sync {
    type Guard: Send + Sync;

    /// Acquire an exclusive lock. Returns `HelperError::Busy` if another
    /// process holds it. Drops automatically when the returned guard
    /// is dropped.
    fn try_acquire(&self, path: &Path) -> Result<Self::Guard, HelperError>;
}
