//! Spec §5.4 — copy a unit fragment under `/var/lib/boxpilot/backups/units/`
//! before mutating it. Used by migrate-cutover.

use boxpilot_ipc::{HelperError, HelperResult};
use boxpilot_platform::traits::fs_perms::{FsPermissions, PathKind};
use std::path::{Path, PathBuf};

/// Copies `src` to `<backups_units_dir>/<unit_name>-<timestamp>` with
/// owner-only permissions so a non-root reader can't see a config that may
/// reference secret material. Returns the absolute backup path. Caller is
/// responsible for supplying a timestamp string that's unique within this
/// backup directory.
pub async fn backup_unit_file(
    src: &Path,
    backups_units_dir: &Path,
    unit_name: &str,
    timestamp: &str,
    fs_perms: &dyn FsPermissions,
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
    fs_perms
        .restrict_to_owner(&tmp, PathKind::File)
        .await
        .map_err(|e| HelperError::Ipc {
            message: format!("backup chmod: {e}"),
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
    use boxpilot_platform::fakes::fs_perms::RecordingFsPermissions;
    use tempfile::tempdir;

    #[tokio::test]
    async fn backup_copies_bytes_and_calls_restrict_to_owner() {
        let tmp = tempdir().unwrap();
        let src = tmp.path().join("sing-box.service");
        tokio::fs::write(&src, b"[Service]\nExecStart=foo\n")
            .await
            .unwrap();
        let backups = tmp.path().join("backups/units");
        let fs_perms = RecordingFsPermissions::new();
        let out = backup_unit_file(
            &src,
            &backups,
            "sing-box.service",
            "2026-04-29T00-00-00Z",
            &fs_perms,
        )
        .await
        .unwrap();
        assert_eq!(
            tokio::fs::read(&out).await.unwrap(),
            b"[Service]\nExecStart=foo\n"
        );
        assert!(out.starts_with(&backups));
        // The recording fake must have been called on the .part temp file
        // (before rename) — confirm at least one call was recorded with
        // PathKind::File. The path recorded is the .part path; after rename
        // the final `out` path won't match, which is fine — the chmod fires
        // before rename, matching the original atomic temp-file pattern.
        let calls = fs_perms.calls();
        assert!(
            calls.iter().any(|(_, kind)| *kind == PathKind::File),
            "restrict_to_owner must be called with PathKind::File; got {calls:?}"
        );
    }

    #[tokio::test]
    async fn backup_unit_file_records_owner_only_on_backup_path_prefix() {
        let tmp = tempdir().unwrap();
        let src = tmp.path().join("sing-box.service");
        tokio::fs::write(&src, b"[Service]\nExecStart=foo\n")
            .await
            .unwrap();
        let backups = tmp.path().join("backups/units");
        let fs_perms = RecordingFsPermissions::new();
        let out = backup_unit_file(
            &src,
            &backups,
            "sing-box.service",
            "2026-04-29T00-00-00Z",
            &fs_perms,
        )
        .await
        .unwrap();
        let calls = fs_perms.calls();
        // The .part temp path lives in the same directory as the final output.
        assert!(
            calls
                .iter()
                .any(|(p, _)| p.parent() == out.parent()),
            "restrict_to_owner must be called on a path inside the backups dir"
        );
    }

    #[tokio::test]
    async fn backup_fails_when_source_missing() {
        let tmp = tempdir().unwrap();
        let fs_perms = RecordingFsPermissions::new();
        let r = backup_unit_file(
            &tmp.path().join("nope"),
            &tmp.path().join("b/u"),
            "x.service",
            "ts",
            &fs_perms,
        )
        .await;
        assert!(matches!(r, Err(HelperError::Ipc { .. })));
    }
}
