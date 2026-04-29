use serde_json::Value;

use crate::import::sha256_hex;
use crate::list::{ProfileStore, StoreError};
use crate::meta::{read_metadata, write_metadata};
use crate::store::write_file_0600_atomic;

/// Recursively merge `patch` into `target`. Object keys are merged;
/// non-object values (arrays, strings, numbers, null) replace.
pub fn apply_patch(target: &mut Value, patch: Value) {
    match (target, patch) {
        (Value::Object(t), Value::Object(p)) => {
            for (k, v) in p {
                if v.is_null() {
                    t.remove(&k);
                } else {
                    apply_patch(t.entry(k).or_insert(Value::Null), v);
                }
            }
        }
        (slot, other) => *slot = other,
    }
}

#[derive(Debug, thiserror::Error)]
pub enum EditError {
    #[error("source is not valid JSON: {0}")]
    InvalidJson(serde_json::Error),
    #[error(transparent)]
    Store(#[from] StoreError),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

pub fn save_edits(
    store: &ProfileStore,
    profile_id: &str,
    new_source_bytes: &[u8],
) -> Result<(), EditError> {
    serde_json::from_slice::<Value>(new_source_bytes).map_err(EditError::InvalidJson)?;
    write_file_0600_atomic(&store.paths().profile_source(profile_id), new_source_bytes)?;
    let mut meta = read_metadata(&store.paths().profile_metadata(profile_id))?;
    meta.updated_at = chrono::Utc::now().to_rfc3339();
    meta.config_sha256 = sha256_hex(new_source_bytes);
    write_metadata(&store.paths().profile_metadata(profile_id), &meta)?;
    Ok(())
}

/// Convenience: load source, apply a patch, and save.
pub fn patch_in_place(
    store: &ProfileStore,
    profile_id: &str,
    patch: Value,
) -> Result<(), EditError> {
    let bytes = std::fs::read(store.paths().profile_source(profile_id))?;
    let mut value: Value = serde_json::from_slice(&bytes).map_err(EditError::InvalidJson)?;
    apply_patch(&mut value, patch);
    let new_bytes = serde_json::to_vec_pretty(&value)
        .map_err(EditError::InvalidJson)?;
    save_edits(store, profile_id, &new_bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::import::import_local_file;
    use crate::store::ProfileStorePaths;
    use pretty_assertions::assert_eq;
    use serde_json::json;

    #[test]
    fn apply_patch_preserves_unknown_fields() {
        let mut t = json!({
            "log": {"level": "info", "_unknown_x": 42},
            "inbounds": [{"type": "tun", "_secret": true}],
            "future_top_level": "stays",
        });
        let p = json!({"log": {"level": "debug"}});
        apply_patch(&mut t, p);
        assert_eq!(t["log"]["level"], json!("debug"));
        assert_eq!(t["log"]["_unknown_x"], json!(42));
        assert_eq!(t["future_top_level"], json!("stays"));
        assert_eq!(t["inbounds"][0]["_secret"], json!(true));
    }

    #[test]
    fn apply_patch_array_replaces_wholesale() {
        let mut t = json!({"inbounds": [{"type":"tun"}]});
        apply_patch(&mut t, json!({"inbounds": [{"type":"mixed"}]}));
        assert_eq!(t["inbounds"], json!([{"type":"mixed"}]));
    }

    #[test]
    fn apply_patch_null_removes_key() {
        let mut t = json!({"keep":1, "drop": "x"});
        apply_patch(&mut t, json!({"drop": null}));
        assert_eq!(t, json!({"keep": 1}));
    }

    #[test]
    fn save_edits_updates_metadata_hash_and_timestamp() {
        let tmp = tempfile::tempdir().unwrap();
        let store = ProfileStore::new(ProfileStorePaths::new(tmp.path().to_path_buf()));
        let src = tmp.path().join("in.json");
        std::fs::write(&src, br#"{"v":1}"#).unwrap();
        let m = import_local_file(&store, &src, "P").unwrap();
        let original_hash = m.config_sha256.clone();

        save_edits(&store, &m.id, br#"{"v":2}"#).unwrap();
        let m2 = store.get(&m.id).unwrap();
        assert_ne!(m2.config_sha256, original_hash);
        assert_eq!(m2.config_sha256, sha256_hex(br#"{"v":2}"#));
    }

    #[test]
    fn save_edits_rejects_invalid_json() {
        let tmp = tempfile::tempdir().unwrap();
        let store = ProfileStore::new(ProfileStorePaths::new(tmp.path().to_path_buf()));
        let src = tmp.path().join("in.json");
        std::fs::write(&src, br#"{"v":1}"#).unwrap();
        let m = import_local_file(&store, &src, "P").unwrap();
        let err = save_edits(&store, &m.id, b"{not json").unwrap_err();
        assert!(matches!(err, EditError::InvalidJson(_)));
    }
}
