//! Spec §10 crash recovery. On every daemon startup, before binding
//! the D-Bus interface, sweep `.staging/*` (always invalid mid-call)
//! and validate `active` resolves under `releases/`.

use crate::paths::Paths;
use std::path::{Path, PathBuf};
use tracing::{info, warn};

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RecoveryReport {
    pub staging_dirs_swept: u32,
    pub active_corrupt: bool,
    pub active_target: Option<PathBuf>,
}

pub async fn reconcile(paths: &Paths) -> RecoveryReport {
    let mut report = RecoveryReport::default();

    let staging = paths.staging_dir();
    if staging.exists() {
        match tokio::fs::read_dir(&staging).await {
            Ok(mut entries) => loop {
                match entries.next_entry().await {
                    Ok(Some(e)) => {
                        let p = e.path();
                        match tokio::fs::remove_dir_all(&p).await {
                            Ok(()) => {
                                report.staging_dirs_swept =
                                    report.staging_dirs_swept.saturating_add(1);
                                info!(path = %p.display(), "swept stale activation staging dir");
                            }
                            Err(e) => warn!(path = %p.display(), "stage sweep failed: {e}"),
                        }
                    }
                    Ok(None) => break,
                    Err(e) => {
                        warn!("read_dir entry: {e}");
                        break;
                    }
                }
            },
            Err(e) => warn!("read_dir staging: {e}"),
        }
    }

    let active = paths.active_symlink();
    if active.symlink_metadata().is_ok() {
        match tokio::fs::read_link(&active).await {
            Ok(target) => {
                let resolved = if target.is_absolute() {
                    target
                } else {
                    active.parent().unwrap_or(Path::new("/")).join(target)
                };
                let resolves_under_releases = resolved.starts_with(paths.releases_dir());
                let target_exists = tokio::fs::metadata(&resolved).await.is_ok();
                if resolves_under_releases && target_exists {
                    report.active_target = Some(resolved);
                } else {
                    warn!(target = %resolved.display(), "active symlink corrupt");
                    report.active_corrupt = true;
                }
            }
            Err(e) => {
                warn!("read_link active: {e}");
                report.active_corrupt = true;
            }
        }
    }
    report
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn no_staging_dir_means_zero_sweeps() {
        let tmp = tempdir().unwrap();
        let paths = Paths::with_root(tmp.path());
        let r = reconcile(&paths).await;
        assert_eq!(r.staging_dirs_swept, 0);
        assert!(!r.active_corrupt);
    }

    #[tokio::test]
    async fn sweeps_stale_staging_subdirs() {
        let tmp = tempdir().unwrap();
        let paths = Paths::with_root(tmp.path());
        std::fs::create_dir_all(paths.staging_subdir("old1")).unwrap();
        std::fs::create_dir_all(paths.staging_subdir("old2")).unwrap();
        let r = reconcile(&paths).await;
        assert_eq!(r.staging_dirs_swept, 2);
        assert!(!paths.staging_subdir("old1").exists());
    }

    #[tokio::test]
    async fn active_pointing_under_releases_is_ok() {
        let tmp = tempdir().unwrap();
        let paths = Paths::with_root(tmp.path());
        let r1 = paths.release_dir("r1");
        std::fs::create_dir_all(&r1).unwrap();
        std::fs::create_dir_all(paths.active_symlink().parent().unwrap()).unwrap();
        std::os::unix::fs::symlink(&r1, paths.active_symlink()).unwrap();
        let r = reconcile(&paths).await;
        assert!(!r.active_corrupt);
        assert_eq!(r.active_target.as_deref(), Some(r1.as_path()));
    }

    #[tokio::test]
    async fn active_pointing_outside_releases_is_corrupt() {
        let tmp = tempdir().unwrap();
        let paths = Paths::with_root(tmp.path());
        let elsewhere = tmp.path().join("elsewhere");
        std::fs::create_dir_all(&elsewhere).unwrap();
        std::fs::create_dir_all(paths.active_symlink().parent().unwrap()).unwrap();
        std::os::unix::fs::symlink(&elsewhere, paths.active_symlink()).unwrap();
        let r = reconcile(&paths).await;
        assert!(r.active_corrupt);
    }

    #[tokio::test]
    async fn active_pointing_at_missing_target_is_corrupt() {
        let tmp = tempdir().unwrap();
        let paths = Paths::with_root(tmp.path());
        let missing = paths.release_dir("ghost");
        std::fs::create_dir_all(paths.active_symlink().parent().unwrap()).unwrap();
        std::os::unix::fs::symlink(&missing, paths.active_symlink()).unwrap();
        let r = reconcile(&paths).await;
        assert!(r.active_corrupt);
    }
}
