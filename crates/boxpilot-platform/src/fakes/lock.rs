//! In-memory fake [`FileLock`]. Lock state lives inside the
//! `MemoryFileLock` instance — tests that want lock contention should
//! share a single `MemoryFileLock` (clone is cheap, it's `Arc`-backed)
//! between contenders.

use crate::traits::lock::FileLock;
use boxpilot_ipc::HelperError;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

/// In-memory cooperative lock, scoped to a single `MemoryFileLock`
/// instance. Tests that want lock contention should share one
/// `MemoryFileLock` between the contenders.
#[derive(Default, Clone)]
pub struct MemoryFileLock {
    held: Arc<Mutex<HashMap<PathBuf, ()>>>,
}

impl MemoryFileLock {
    pub fn new() -> Self {
        Self::default()
    }
}

pub struct LockGuard {
    held: Arc<Mutex<HashMap<PathBuf, ()>>>,
    path: PathBuf,
}

impl Drop for LockGuard {
    fn drop(&mut self) {
        let _ = self.held.lock().unwrap().remove(&self.path);
    }
}

impl FileLock for MemoryFileLock {
    type Guard = LockGuard;

    fn try_acquire(&self, path: &Path) -> Result<LockGuard, HelperError> {
        let mut held = self.held.lock().unwrap();
        if held.contains_key(path) {
            return Err(HelperError::Busy);
        }
        held.insert(path.to_path_buf(), ());
        Ok(LockGuard {
            held: Arc::clone(&self.held),
            path: path.to_path_buf(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn acquire_then_busy_then_release() {
        let lock = MemoryFileLock::new();
        let path = std::path::Path::new("/tmp/test-lock");
        let g1 = lock.try_acquire(path).unwrap();
        assert!(matches!(lock.try_acquire(path), Err(HelperError::Busy)));
        drop(g1);
        let _g2 = lock.try_acquire(path).unwrap();
    }
}
