//! Owner-only filesystem permission setting.
//!
//! Linux: `chmod 0700` (dir) / `chmod 0600` (file).
//! Windows: `SetSecurityInfo` clears inheritance and grants the owner SID
//! full access (Sub-project #1 ships the real impl since this is needed for
//! `%LocalAppData%\BoxPilot\` ACLing).
//!
//! Spec §5.6 + §14: user profile directories must be 0700 (Linux) /
//! owner-only DACL (Windows); profile files 0600 / equivalent.

use async_trait::async_trait;
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathKind {
    Directory,
    File,
}

#[async_trait]
pub trait FsPermissions: Send + Sync {
    async fn restrict_to_owner(&self, path: &Path, kind: PathKind) -> std::io::Result<()>;
}
