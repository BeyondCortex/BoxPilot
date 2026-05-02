//! Windows `FsPermissions` stub for Sub-project #1.
//!
//! The original PR 3 plan specified an inline `SetNamedSecurityInfoW` impl,
//! but the proposed shape passed `pDacl = NULL` with `DACL_SECURITY_INFORMATION`,
//! which per Win32 docs grants Everyone full access — the inverse of the trait
//! contract. PR 12's "Windows real impls" scope is the correct home for the
//! production ACL story (build an explicit DACL with one ACE for the current
//! user SID via `SetEntriesInAclW`, freeing the descriptor returned by
//! `GetNamedSecurityInfoW` with `LocalFree`). For Sub-project #1 the goal is
//! just compile + minimum-boot, so this stub no-ops with a debug log.

use crate::traits::fs_perms::{FsPermissions, PathKind};
use async_trait::async_trait;
use std::path::Path;

pub struct AclFsPermissions;

#[async_trait]
impl FsPermissions for AclFsPermissions {
    async fn restrict_to_owner(&self, path: &Path, kind: PathKind) -> std::io::Result<()> {
        tracing::debug!(
            ?path,
            ?kind,
            "FsPermissions stub: real ACL impl pending in PR 12 (Sub-project #1 minimum boot)"
        );
        Ok(())
    }
}
