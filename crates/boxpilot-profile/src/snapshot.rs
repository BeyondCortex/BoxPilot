use std::path::Path;

use crate::list::{ProfileStore, StoreError};
use crate::meta::{read_metadata, write_metadata};
use crate::store::{ensure_dir_0700, write_file_0600_atomic};

#[derive(Debug, thiserror::Error)]
pub enum SnapshotError {
    #[error("profile has no last-valid snapshot")]
    NoSnapshot,
    #[error(transparent)]
    Store(#[from] StoreError),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

/// Mirror the staged config + assets into `last-valid/`. Replaces any
/// existing snapshot. Idempotent — safe to call from plan #5 after each
/// successful activation. `staged_config` and `staged_assets_dir` are the
/// post-rename release contents (plan #5 reads them back from
/// `/etc/boxpilot/active/...` via `boxpilotd` and forwards the bytes).
///
/// In plan #4 unit tests we copy directly from the staging tempdir.
pub fn record_last_valid(
    store: &ProfileStore,
    profile_id: &str,
    activation_id: &str,
    staged_config: &[u8],
    staged_assets_dir: &Path,
) -> Result<(), SnapshotError> {
    let dst_root = store.paths().profile_last_valid_dir(profile_id);
    if dst_root.exists() {
        std::fs::remove_dir_all(&dst_root)?;
    }
    ensure_dir_0700(&dst_root)?;
    let dst_assets = store.paths().profile_last_valid_assets_dir(profile_id);
    ensure_dir_0700(&dst_assets)?;
    write_file_0600_atomic(&store.paths().profile_last_valid_config(profile_id), staged_config)?;
    if staged_assets_dir.exists() {
        copy_tree(staged_assets_dir, &dst_assets)?;
    }
    let mut meta = read_metadata(&store.paths().profile_metadata(profile_id))
        .map_err(SnapshotError::Io)?;
    meta.last_valid_activation_id = Some(activation_id.to_string());
    write_metadata(&store.paths().profile_metadata(profile_id), &meta)?;
    Ok(())
}

fn copy_tree(src: &Path, dst: &Path) -> std::io::Result<()> {
    let mut stack = vec![(src.to_path_buf(), dst.to_path_buf())];
    while let Some((s, d)) = stack.pop() {
        ensure_dir_0700(&d)?;
        for entry in std::fs::read_dir(&s)? {
            let entry = entry?;
            let p = entry.path();
            let ft = std::fs::symlink_metadata(&p)?.file_type();
            let dst_child = d.join(entry.file_name());
            if ft.is_dir() {
                stack.push((p, dst_child));
            } else if ft.is_file() {
                let bytes = std::fs::read(&p)?;
                write_file_0600_atomic(&dst_child, &bytes)?;
            }
            // ignore other file types; daemon-side rejected them on import
        }
    }
    Ok(())
}

/// Restore the editor's `source.json` (and asset tree) from `last-valid/`.
pub fn revert_to_last_valid(
    store: &ProfileStore,
    profile_id: &str,
) -> Result<(), SnapshotError> {
    let lv_config = store.paths().profile_last_valid_config(profile_id);
    if !lv_config.exists() { return Err(SnapshotError::NoSnapshot); }
    let bytes = std::fs::read(&lv_config)?;
    write_file_0600_atomic(&store.paths().profile_source(profile_id), &bytes)?;

    let lv_assets = store.paths().profile_last_valid_assets_dir(profile_id);
    let dst_assets = store.paths().profile_assets_dir(profile_id);
    if dst_assets.exists() { std::fs::remove_dir_all(&dst_assets)?; }
    ensure_dir_0700(&dst_assets)?;
    if lv_assets.exists() { copy_tree(&lv_assets, &dst_assets)?; }

    let mut meta = read_metadata(&store.paths().profile_metadata(profile_id))?;
    meta.updated_at = chrono::Utc::now().to_rfc3339();
    meta.config_sha256 = crate::import::sha256_hex(&bytes);
    write_metadata(&store.paths().profile_metadata(profile_id), &meta)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::import::import_local_dir;
    use crate::store::ProfileStorePaths;
    use pretty_assertions::assert_eq;

    fn fixture() -> (tempfile::TempDir, ProfileStore) {
        let tmp = tempfile::tempdir().unwrap();
        let s = ProfileStore::new(ProfileStorePaths::new(tmp.path().to_path_buf()));
        (tmp, s)
    }

    #[test]
    fn record_then_revert_round_trips_config_and_assets() {
        let (tmp, s) = fixture();
        let src = tmp.path().join("bundle");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(src.join("config.json"), br#"{"v":1}"#).unwrap();
        std::fs::write(src.join("a.db"), b"A").unwrap();
        let m = import_local_dir(&s, &src, "P").unwrap();

        record_last_valid(&s, &m.id, "act-1", br#"{"v":1}"#, &s.paths().profile_assets_dir(&m.id)).unwrap();
        // mutate the working copy
        crate::editor::save_edits(&s, &m.id, br#"{"v":99}"#).unwrap();
        std::fs::write(s.paths().profile_assets_dir(&m.id).join("a.db"), b"DIRTY").unwrap();

        revert_to_last_valid(&s, &m.id).unwrap();
        assert_eq!(std::fs::read(s.paths().profile_source(&m.id)).unwrap(), br#"{"v":1}"#);
        assert_eq!(std::fs::read(s.paths().profile_assets_dir(&m.id).join("a.db")).unwrap(), b"A");

        let m2 = s.get(&m.id).unwrap();
        assert_eq!(m2.last_valid_activation_id.as_deref(), Some("act-1"));
    }

    #[test]
    fn revert_without_snapshot_errors() {
        let (tmp, s) = fixture();
        let src = tmp.path().join("in.json");
        std::fs::write(&src, br#"{"v":1}"#).unwrap();
        let m = crate::import::import_local_file(&s, &src, "P").unwrap();
        let err = revert_to_last_valid(&s, &m.id).unwrap_err();
        assert!(matches!(err, SnapshotError::NoSnapshot));
    }
}
