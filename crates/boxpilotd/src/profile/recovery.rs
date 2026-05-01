//! Spec §10 crash recovery. On every daemon startup, before binding
//! the D-Bus interface, sweep `.staging/*` (always invalid mid-call)
//! and validate `active` resolves under `releases/`.

use crate::paths::Paths;
use boxpilot_ipc::BoxpilotConfig;
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

    let status = check_active_status(paths).await;
    report.active_corrupt = status.corrupt;
    report.active_target = status.target;
    report
}

/// Result of evaluating `/etc/boxpilot/active` against `releases/` + the
/// `boxpilot.toml` `active_release_id` claim. Both the daemon-startup
/// recovery path (`reconcile`) and the GUI's `home.status` rely on this
/// shared predicate so they can never disagree about what "corrupt" means.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ActiveStatus {
    pub corrupt: bool,
    pub target: Option<PathBuf>,
}

/// Read-only check; does not mutate disk. Safe to call from a polling
/// `home.status` loop.
pub async fn check_active_status(paths: &Paths) -> ActiveStatus {
    let mut status = ActiveStatus::default();
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
                    status.target = Some(resolved);
                } else {
                    warn!(target = %resolved.display(), "active symlink corrupt");
                    status.corrupt = true;
                }
            }
            Err(e) => {
                warn!("read_link active: {e}");
                status.corrupt = true;
            }
        }
    } else if toml_claims_active(paths).await {
        // Symlink absent but boxpilot.toml records something as active —
        // the activation pipeline's no-previous failure paths remove the
        // symlink to surface this divergence here. Force the operator to
        // re-activate explicitly rather than silently accepting a
        // half-committed state.
        warn!("active symlink missing but toml has active_release_id; flagging corrupt");
        status.corrupt = true;
    }
    status
}

async fn toml_claims_active(paths: &Paths) -> bool {
    let cfg_text = match tokio::fs::read_to_string(paths.boxpilot_toml()).await {
        Ok(s) => s,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            // Fresh install before service.install_managed has run — no toml
            // is the expected state, not a diagnostic.
            return false;
        }
        Err(e) => {
            warn!("read boxpilot.toml during reconcile: {e}; assuming no active claim");
            return false;
        }
    };
    match BoxpilotConfig::parse(&cfg_text) {
        Ok(cfg) => cfg.active_release_id.is_some(),
        Err(e) => {
            // Includes UnsupportedSchemaVersion: stays safe (no false-positive
            // active_corrupt) but logs so a future schema rollover doesn't
            // silently disable this tripwire.
            warn!("parse boxpilot.toml during reconcile: {e}; assuming no active claim");
            false
        }
    }
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

    #[tokio::test]
    async fn missing_symlink_with_toml_active_is_corrupt() {
        // The activation pipeline's no-previous failure paths remove the
        // active symlink as a tripwire — reconcile must turn that into
        // active_corrupt so the next activation refuses to proceed silently.
        let tmp = tempdir().unwrap();
        let paths = Paths::with_root(tmp.path());
        std::fs::create_dir_all(paths.etc_dir()).unwrap();
        std::fs::write(
            paths.boxpilot_toml(),
            "schema_version = 1\nactive_release_id = \"orphan\"\n",
        )
        .unwrap();
        // No symlink exists at paths.active_symlink().
        let r = reconcile(&paths).await;
        assert!(r.active_corrupt);
        assert!(r.active_target.is_none());
    }

    #[tokio::test]
    async fn missing_symlink_with_no_toml_active_is_clean() {
        // Fresh install before any activation — toml has no
        // active_release_id and no symlink. Not corrupt.
        let tmp = tempdir().unwrap();
        let paths = Paths::with_root(tmp.path());
        std::fs::create_dir_all(paths.etc_dir()).unwrap();
        std::fs::write(paths.boxpilot_toml(), "schema_version = 1\n").unwrap();
        let r = reconcile(&paths).await;
        assert!(!r.active_corrupt);
    }
}
