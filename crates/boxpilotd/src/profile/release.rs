//! Spec §10 step 8 (`rename(2)` staging→releases) and step 9 (atomic
//! symlink swap of `/etc/boxpilot/active` via `rename(2)` on
//! `active.new`). The `ln -sfn` path is intentionally not used — it
//! unlinks first, leaving a window where `active` does not exist.
//!
//! These free functions predate the [`boxpilot_platform::traits::active::ActivePointer`]
//! trait introduced in PR 8. They remain in place because activate.rs and
//! rollback.rs rely on `read_active_target` returning a concrete `PathBuf`
//! that's compared with previous-target paths during the rollback dance;
//! migrating those call sites to `&dyn ActivePointer` is tracked as a
//! follow-up. The symlink + rename pattern is Linux-only — Windows builds
//! pull the trait + `MarkerFileActivePointer` instead.

#![cfg(target_os = "linux")]

use boxpilot_ipc::{HelperError, HelperResult};
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};

/// Move staging into releases. Both paths must live on the same
/// filesystem (they do: `/etc/boxpilot` is one mount in production).
pub fn promote_staging(staging: &Path, target: &Path) -> HelperResult<()> {
    if !staging.exists() {
        return Err(HelperError::Ipc {
            message: format!("staging {} missing", staging.display()),
        });
    }
    if target.exists() {
        return Err(HelperError::Ipc {
            message: format!("release dir {} already exists", target.display()),
        });
    }
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent).map_err(|e| HelperError::Ipc {
            message: format!("mkdir releases parent: {e}"),
        })?;
    }
    std::fs::rename(staging, target).map_err(|e| HelperError::Ipc {
        message: format!("promote {} -> {}: {e}", staging.display(), target.display()),
    })?;
    Ok(())
}

/// Atomic symlink replace via `rename(2)` on `active.new`. The kernel
/// guarantees `active` resolves at every instant.
pub fn swap_active_symlink(active: &Path, new_target: &Path) -> HelperResult<()> {
    let new_link = active.with_extension("new");
    if let Some(parent) = active.parent() {
        std::fs::create_dir_all(parent).map_err(|e| HelperError::Ipc {
            message: format!("mkdir active parent: {e}"),
        })?;
    }
    if new_link.symlink_metadata().is_ok() {
        std::fs::remove_file(&new_link).map_err(|e| HelperError::Ipc {
            message: format!("remove stale active.new: {e}"),
        })?;
    }
    symlink(new_target, &new_link).map_err(|e| HelperError::Ipc {
        message: format!("create active.new -> {}: {e}", new_target.display()),
    })?;
    std::fs::rename(&new_link, active).map_err(|e| HelperError::Ipc {
        message: format!("rename active.new -> active: {e}"),
    })?;
    Ok(())
}

/// Resolve `active` to its target. Returns `None` when `active` is
/// missing, dangling, or not a symlink.
pub fn read_active_target(active: &Path) -> Option<PathBuf> {
    let target = std::fs::read_link(active).ok()?;
    let resolved = if target.is_absolute() {
        target
    } else {
        active.parent()?.join(target)
    };
    if resolved.exists() {
        Some(resolved)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn promote_staging_moves_dir_atomically() {
        let dir = tempdir().unwrap();
        let staging = dir.path().join(".staging/abc");
        std::fs::create_dir_all(&staging).unwrap();
        std::fs::write(staging.join("config.json"), b"{}").unwrap();
        let target = dir.path().join("releases/abc");
        promote_staging(&staging, &target).unwrap();
        assert!(target.join("config.json").exists());
        assert!(!staging.exists());
    }

    #[test]
    fn promote_staging_refuses_existing_target() {
        let dir = tempdir().unwrap();
        let staging = dir.path().join(".staging/abc");
        std::fs::create_dir_all(&staging).unwrap();
        let target = dir.path().join("releases/abc");
        std::fs::create_dir_all(&target).unwrap();
        assert!(promote_staging(&staging, &target).is_err());
    }

    #[test]
    fn swap_active_symlink_creates_then_replaces() {
        let dir = tempdir().unwrap();
        let active = dir.path().join("active");
        let r1 = dir.path().join("releases/r1");
        let r2 = dir.path().join("releases/r2");
        std::fs::create_dir_all(&r1).unwrap();
        std::fs::create_dir_all(&r2).unwrap();
        swap_active_symlink(&active, &r1).unwrap();
        assert_eq!(std::fs::read_link(&active).unwrap(), r1);
        swap_active_symlink(&active, &r2).unwrap();
        assert_eq!(std::fs::read_link(&active).unwrap(), r2);
    }

    #[test]
    fn swap_active_symlink_clears_stale_active_new() {
        let dir = tempdir().unwrap();
        let active = dir.path().join("active");
        let stale = dir.path().join("active.new");
        let r1 = dir.path().join("r1");
        std::fs::create_dir_all(&r1).unwrap();
        std::os::unix::fs::symlink(&r1, &stale).unwrap();
        swap_active_symlink(&active, &r1).unwrap();
        assert!(stale.symlink_metadata().is_err());
        assert_eq!(std::fs::read_link(&active).unwrap(), r1);
    }

    #[test]
    fn read_active_target_returns_resolved_dir() {
        let dir = tempdir().unwrap();
        let active = dir.path().join("active");
        let r = dir.path().join("r");
        std::fs::create_dir_all(&r).unwrap();
        std::os::unix::fs::symlink(&r, &active).unwrap();
        assert_eq!(read_active_target(&active), Some(r));
    }

    #[test]
    fn read_active_target_returns_none_when_dangling() {
        let dir = tempdir().unwrap();
        let active = dir.path().join("active");
        std::os::unix::fs::symlink(dir.path().join("nope"), &active).unwrap();
        assert_eq!(read_active_target(&active), None);
    }

    #[test]
    fn read_active_target_returns_none_when_missing() {
        let dir = tempdir().unwrap();
        let active = dir.path().join("active");
        assert_eq!(read_active_target(&active), None);
    }
}
