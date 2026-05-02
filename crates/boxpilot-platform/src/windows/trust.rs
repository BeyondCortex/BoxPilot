//! Windows trust check: stub for Sub-project #1. The real impl
//! (NTFS ACL + owner-SID + parent-dir-not-writable) lands in Sub-project #2.

use crate::traits::trust::{TrustChecker, TrustError};
use std::path::{Path, PathBuf};

pub struct WindowsTrustChecker;

impl TrustChecker for WindowsTrustChecker {
    fn check(&self, path: &Path, _: &[PathBuf]) -> Result<PathBuf, TrustError> {
        Err(TrustError::SymlinkResolution(format!(
            "Windows trust check stub: implemented in Sub-project #2 (path: {})",
            path.display()
        )))
    }
}
