use chrono::Utc;
use sha2::{Digest, Sha256};
use std::path::Path;

use boxpilot_ipc::SourceKind;

use crate::list::ProfileStore;
use crate::meta::{write_metadata, ProfileMetadata, METADATA_SCHEMA_VERSION};
use crate::store::{ensure_dir_0700, write_file_0600_atomic};

#[derive(Debug, thiserror::Error)]
pub enum ImportError {
    #[error("source is not valid JSON: {0}")]
    InvalidJson(serde_json::Error),
    #[error("source file is too large ({size} bytes; limit {limit})")]
    TooLarge { size: u64, limit: u64 },
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

/// Cap an in-memory single-JSON import at the per-file limit so a huge
/// pasted blob can't OOM the GUI; matches §9.2's per-file cap.
pub const SINGLE_JSON_MAX_BYTES: u64 = boxpilot_ipc::BUNDLE_MAX_FILE_BYTES;

pub fn slugify(name: &str) -> String {
    let mut s: String = name
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '-' })
        .collect();
    while s.contains("--") { s = s.replace("--", "-"); }
    let trimmed = s.trim_matches('-');
    if trimmed.is_empty() { "profile".to_string() } else { trimmed.to_string() }
}

/// Stable-but-unique-on-this-machine. `name` only contributes the slug;
/// the timestamp + 8-hex random suffix guarantee no collisions across
/// repeated imports of profiles with the same name.
pub fn new_profile_id(name: &str, now: chrono::DateTime<Utc>) -> String {
    let ts = now.format("%Y%m%dT%H%M%SZ").to_string();
    let nanos = now.timestamp_subsec_nanos();
    let pid = std::process::id();
    let mut h = Sha256::new();
    h.update(ts.as_bytes());
    h.update(nanos.to_le_bytes());
    h.update(pid.to_le_bytes());
    h.update(name.as_bytes());
    let suffix = &hex::encode(h.finalize())[..8];
    format!("{}-{}-{}", slugify(name), ts, suffix)
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(bytes);
    hex::encode(h.finalize())
}

pub fn import_local_file(
    store: &ProfileStore,
    src_path: &Path,
    name: &str,
) -> Result<ProfileMetadata, ImportError> {
    let meta = std::fs::metadata(src_path)?;
    if meta.len() > SINGLE_JSON_MAX_BYTES {
        return Err(ImportError::TooLarge { size: meta.len(), limit: SINGLE_JSON_MAX_BYTES });
    }
    let bytes = std::fs::read(src_path)?;
    serde_json::from_slice::<serde_json::Value>(&bytes).map_err(ImportError::InvalidJson)?;

    let now = Utc::now();
    let id = new_profile_id(name, now);
    let dir = store.paths().profile_dir(&id);
    ensure_dir_0700(&dir)?;
    ensure_dir_0700(&store.paths().profile_assets_dir(&id))?;

    write_file_0600_atomic(&store.paths().profile_source(&id), &bytes)?;

    let now_str = now.to_rfc3339();
    let m = ProfileMetadata {
        schema_version: METADATA_SCHEMA_VERSION,
        id: id.clone(),
        name: name.to_string(),
        source_kind: SourceKind::Local,
        remote_id: None,
        created_at: now_str.clone(),
        updated_at: now_str,
        last_valid_activation_id: None,
        config_sha256: sha256_hex(&bytes),
    };
    write_metadata(&store.paths().profile_metadata(&id), &m)?;
    Ok(m)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::ProfileStorePaths;
    use pretty_assertions::assert_eq;
    use std::os::unix::fs::PermissionsExt;

    fn fixture() -> (tempfile::TempDir, ProfileStore) {
        let tmp = tempfile::tempdir().unwrap();
        let s = ProfileStore::new(ProfileStorePaths::new(tmp.path().to_path_buf()));
        (tmp, s)
    }

    #[test]
    fn slugify_handles_punctuation_and_unicode() {
        assert_eq!(slugify("My Profile!"), "my-profile");
        assert_eq!(slugify("一二三 abc"), "abc");
        assert_eq!(slugify("---"), "profile");
    }

    #[test]
    fn id_is_collision_resistant_for_same_name_different_times() {
        let t1 = chrono::Utc::now();
        let t2 = t1 + chrono::Duration::seconds(1);
        assert_ne!(new_profile_id("same", t1), new_profile_id("same", t2));
    }

    #[test]
    fn import_local_file_writes_layout_and_perms() {
        let (tmp, s) = fixture();
        let src = tmp.path().join("input.json");
        std::fs::write(&src, r#"{"hello":"world"}"#).unwrap();

        let m = import_local_file(&s, &src, "Hello").unwrap();
        assert!(matches!(m.source_kind, SourceKind::Local));
        assert!(m.id.starts_with("hello-"));

        // source.json mode 0600
        let src_mode = std::fs::metadata(s.paths().profile_source(&m.id)).unwrap()
            .permissions().mode() & 0o7777;
        assert_eq!(src_mode, 0o600);

        // assets/ mode 0700
        let assets_mode = std::fs::metadata(s.paths().profile_assets_dir(&m.id)).unwrap()
            .permissions().mode() & 0o7777;
        assert_eq!(assets_mode, 0o700);

        // metadata.json mode 0600
        let mm = std::fs::metadata(s.paths().profile_metadata(&m.id)).unwrap()
            .permissions().mode() & 0o7777;
        assert_eq!(mm, 0o600);
    }

    #[test]
    fn import_local_file_rejects_invalid_json() {
        let (tmp, s) = fixture();
        let src = tmp.path().join("bad.json");
        std::fs::write(&src, b"{not json").unwrap();
        assert!(matches!(import_local_file(&s, &src, "n"), Err(ImportError::InvalidJson(_))));
    }
}
