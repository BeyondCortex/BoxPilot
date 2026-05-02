//! Spec §6.5 trust check, abstracted so Windows ACL semantics can plug in
//! later. Linux: uid + mode bits + parent-dir walk + setuid + symlink walk.
//! Windows (Sub-project #2): NTFS ACL + owner-SID + parent-dir-not-writable.

use std::path::{Path, PathBuf};
use thiserror::Error;

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum TrustError {
    #[error("file does not exist: {0}")]
    NotFound(PathBuf),
    #[error("not a regular file: {0}")]
    NotRegular(PathBuf),
    #[error("not owned by root (uid={uid}, gid={gid}): {path}")]
    NotRootOwned { path: PathBuf, uid: u32, gid: u32 },
    #[error("group/world writable: {path} (mode={mode:o})")]
    Writable { path: PathBuf, mode: u32 },
    #[error("setuid/setgid/sticky bit set: {path} (mode={mode:o})")]
    SpecialBits { path: PathBuf, mode: u32 },
    #[error("path outside allowed prefixes: {0}")]
    DisallowedPrefix(PathBuf),
    #[error("symlink resolution failed: {0}")]
    SymlinkResolution(String),
    #[error("sing-box version self-check failed: {0}")]
    VersionCheckFailed(String),
}

/// Trust check facade: implementations decide what "trusted" means on each
/// platform. Linux walks `uid + mode + parent-dirs`; Windows (Sub-project #2)
/// inspects NTFS ACLs and owner SIDs. Returns the canonicalized path so
/// callers can pass the safe form to subsequent operations.
pub trait TrustChecker: Send + Sync {
    fn check(
        &self,
        path: &Path,
        allowed_prefixes: &[PathBuf],
    ) -> Result<PathBuf, TrustError>;
}
