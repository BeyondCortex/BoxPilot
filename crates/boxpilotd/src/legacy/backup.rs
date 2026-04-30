//! Spec §5.4 — copy a unit fragment under `/var/lib/boxpilot/backups/units/`
//! before mutating it. Used by migrate-cutover.

use boxpilot_ipc::{HelperError, HelperResult};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

/// Copies `src` to `<backups_units_dir>/<unit_name>-<timestamp>` with mode
/// 0600 so a non-root reader can't see a config that may reference secret
/// material. Returns the absolute backup path. Caller is responsible for
/// supplying a timestamp string that's unique within this backup directory.
pub async fn backup_unit_file(
    src: &Path,
    backups_units_dir: &Path,
    unit_name: &str,
    timestamp: &str,
) -> HelperResult<PathBuf> {
    tokio::fs::create_dir_all(backups_units_dir)
        .await
        .map_err(|e| HelperError::Ipc {
            message: format!("mkdir backups: {e}"),
        })?;
    let dst = backups_units_dir.join(format!("{unit_name}-{timestamp}"));
    let bytes = tokio::fs::read(src).await.map_err(|e| HelperError::Ipc {
        message: format!("read fragment {}: {e}", src.display()),
    })?;
    let tmp = dst.with_extension("part");
    tokio::fs::write(&tmp, &bytes)
        .await
        .map_err(|e| HelperError::Ipc {
            message: format!("write backup tmp: {e}"),
        })?;
    let perm = std::fs::Permissions::from_mode(0o600);
    tokio::fs::set_permissions(&tmp, perm)
        .await
        .map_err(|e| HelperError::Ipc {
            message: format!("chmod backup tmp: {e}"),
        })?;
    let f = tokio::fs::OpenOptions::new()
        .read(true)
        .open(&tmp)
        .await
        .map_err(|e| HelperError::Ipc {
            message: format!("open backup for fsync: {e}"),
        })?;
    f.sync_all().await.map_err(|e| HelperError::Ipc {
        message: format!("fsync backup: {e}"),
    })?;
    tokio::fs::rename(&tmp, &dst)
        .await
        .map_err(|e| HelperError::Ipc {
            message: format!("rename backup: {e}"),
        })?;
    Ok(dst)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    use tempfile::tempdir;

    #[tokio::test]
    async fn backup_copies_bytes_and_sets_0600() {
        let tmp = tempdir().unwrap();
        let src = tmp.path().join("sing-box.service");
        tokio::fs::write(&src, b"[Service]\nExecStart=foo\n")
            .await
            .unwrap();
        let backups = tmp.path().join("backups/units");
        let out = backup_unit_file(&src, &backups, "sing-box.service", "2026-04-29T00-00-00Z")
            .await
            .unwrap();
        assert_eq!(
            tokio::fs::read(&out).await.unwrap(),
            b"[Service]\nExecStart=foo\n"
        );
        let mode = std::fs::metadata(&out).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        assert!(out.starts_with(&backups));
    }

    #[tokio::test]
    async fn backup_fails_when_source_missing() {
        let tmp = tempdir().unwrap();
        let r = backup_unit_file(
            &tmp.path().join("nope"),
            &tmp.path().join("b/u"),
            "x.service",
            "ts",
        )
        .await;
        assert!(matches!(r, Err(HelperError::Ipc { .. })));
    }
}
