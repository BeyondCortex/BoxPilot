//! Linux [`ActivePointer`] backed by a symlink at `active`. The atomic
//! swap goes through `active.new` + `rename(2)`; the kernel guarantees
//! `active` resolves at every instant. Mirrors the original
//! `boxpilotd::profile::release::swap_active_symlink` algorithm.

use crate::traits::active::ActivePointer;
use async_trait::async_trait;
use boxpilot_ipc::HelperError;
use std::os::unix::fs::symlink;
use std::path::PathBuf;

pub struct SymlinkActivePointer {
    pub active: PathBuf,
    pub releases_dir: PathBuf,
}

#[async_trait]
impl ActivePointer for SymlinkActivePointer {
    async fn read(&self) -> Result<Option<String>, HelperError> {
        match tokio::fs::read_link(&self.active).await {
            Ok(target) => Ok(target
                .file_name()
                .and_then(|n| n.to_str().map(|s| s.to_string()))),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(HelperError::Ipc {
                message: format!("read_link {}: {e}", self.active.display()),
            }),
        }
    }

    async fn set(&self, release_id: &str) -> Result<(), HelperError> {
        let target = self.releases_dir.join(release_id);
        let active = self.active.clone();
        let new_link = active.with_extension("new");

        if let Some(parent) = active.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|e| HelperError::Ipc {
                    message: format!("mkdir active parent: {e}"),
                })?;
        }

        let new_link_for_blocking = new_link.clone();
        let target_for_blocking = target.clone();
        tokio::task::spawn_blocking(move || -> Result<(), HelperError> {
            if new_link_for_blocking.symlink_metadata().is_ok() {
                std::fs::remove_file(&new_link_for_blocking).map_err(|e| HelperError::Ipc {
                    message: format!("remove stale active.new: {e}"),
                })?;
            }
            symlink(&target_for_blocking, &new_link_for_blocking).map_err(|e| HelperError::Ipc {
                message: format!(
                    "create active.new -> {}: {e}",
                    target_for_blocking.display()
                ),
            })?;
            Ok(())
        })
        .await
        .map_err(|e| HelperError::Ipc {
            message: format!("symlink spawn join: {e}"),
        })??;

        tokio::fs::rename(&new_link, &active)
            .await
            .map_err(|e| HelperError::Ipc {
                message: format!("rename active.new -> active: {e}"),
            })?;
        Ok(())
    }

    async fn active_resolved(&self) -> Result<Option<PathBuf>, HelperError> {
        match tokio::fs::read_link(&self.active).await {
            Ok(target) => Ok(Some(target)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(HelperError::Ipc {
                message: format!("read_link {}: {e}", self.active.display()),
            }),
        }
    }

    fn release_dir(&self, release_id: &str) -> PathBuf {
        self.releases_dir.join(release_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn set_then_read_round_trips() {
        let dir = tempdir().unwrap();
        let releases = dir.path().join("releases");
        std::fs::create_dir_all(releases.join("r1")).unwrap();
        let active = SymlinkActivePointer {
            active: dir.path().join("active"),
            releases_dir: releases,
        };
        active.set("r1").await.unwrap();
        assert_eq!(active.read().await.unwrap().as_deref(), Some("r1"));
    }

    #[tokio::test]
    async fn set_replaces_existing() {
        let dir = tempdir().unwrap();
        let releases = dir.path().join("releases");
        std::fs::create_dir_all(releases.join("r1")).unwrap();
        std::fs::create_dir_all(releases.join("r2")).unwrap();
        let active = SymlinkActivePointer {
            active: dir.path().join("active"),
            releases_dir: releases,
        };
        active.set("r1").await.unwrap();
        active.set("r2").await.unwrap();
        assert_eq!(active.read().await.unwrap().as_deref(), Some("r2"));
    }

    #[tokio::test]
    async fn read_missing_returns_none() {
        let dir = tempdir().unwrap();
        let active = SymlinkActivePointer {
            active: dir.path().join("active"),
            releases_dir: dir.path().join("releases"),
        };
        assert!(active.read().await.unwrap().is_none());
    }

    #[tokio::test]
    async fn set_clears_stale_active_new() {
        let dir = tempdir().unwrap();
        let releases = dir.path().join("releases");
        std::fs::create_dir_all(releases.join("r1")).unwrap();
        let stale = dir.path().join("active.new");
        std::os::unix::fs::symlink(releases.join("r1"), &stale).unwrap();
        let active = SymlinkActivePointer {
            active: dir.path().join("active"),
            releases_dir: releases,
        };
        active.set("r1").await.unwrap();
        assert!(stale.symlink_metadata().is_err());
        assert_eq!(active.read().await.unwrap().as_deref(), Some("r1"));
    }

    #[tokio::test]
    async fn active_resolved_returns_symlink_target() {
        let dir = tempdir().unwrap();
        let releases = dir.path().join("releases");
        std::fs::create_dir_all(releases.join("r1")).unwrap();
        let active = SymlinkActivePointer {
            active: dir.path().join("active"),
            releases_dir: releases.clone(),
        };
        active.set("r1").await.unwrap();
        assert_eq!(
            active.active_resolved().await.unwrap(),
            Some(releases.join("r1"))
        );
    }
}
