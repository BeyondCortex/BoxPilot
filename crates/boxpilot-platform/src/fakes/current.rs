//! Cross-platform in-memory fake [`CurrentPointer`]. Records the last call
//! arguments in a `Mutex` so tests can assert on them without a filesystem.

use crate::traits::current::CurrentPointer;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// In-memory fake that records the last `(link, target)` pair passed to
/// `set_atomic`, or `None` if it has never been called.
pub struct InMemoryCurrent {
    pub last: Mutex<Option<(PathBuf, PathBuf)>>,
}

impl InMemoryCurrent {
    pub fn new() -> Self {
        Self {
            last: Mutex::new(None),
        }
    }
}

impl Default for InMemoryCurrent {
    fn default() -> Self {
        Self::new()
    }
}

impl CurrentPointer for InMemoryCurrent {
    fn set_atomic(&self, link: &Path, target: &Path) -> std::io::Result<()> {
        *self.last.lock().unwrap() = Some((link.to_path_buf(), target.to_path_buf()));
        Ok(())
    }
}
