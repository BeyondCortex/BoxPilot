use boxpilot_ipc::SourceKind;
use serde::{Deserialize, Serialize};
use std::path::Path;

use crate::store::write_file_0600_atomic;

pub const METADATA_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProfileMetadata {
    pub schema_version: u32,
    pub id: String,
    pub name: String,
    pub source_kind: SourceKind,
    /// Set only when `source_kind == Remote`; key into `remotes.json`.
    pub remote_id: Option<String>,
    pub created_at: String, // RFC3339
    pub updated_at: String, // RFC3339
    pub last_valid_activation_id: Option<String>,
    /// SHA-256 hex of the bytes currently on disk in `source.json`.
    pub config_sha256: String,
}

impl ProfileMetadata {
    pub fn new_local(id: &str, name: &str, now_rfc3339: &str, config_sha256: &str) -> Self {
        Self {
            schema_version: METADATA_SCHEMA_VERSION,
            id: id.to_string(),
            name: name.to_string(),
            source_kind: SourceKind::Local,
            remote_id: None,
            created_at: now_rfc3339.to_string(),
            updated_at: now_rfc3339.to_string(),
            last_valid_activation_id: None,
            config_sha256: config_sha256.to_string(),
        }
    }
}

pub fn read_metadata(path: &Path) -> std::io::Result<ProfileMetadata> {
    let bytes = std::fs::read(path)?;
    serde_json::from_slice(&bytes)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
}

pub fn write_metadata(path: &Path, meta: &ProfileMetadata) -> std::io::Result<()> {
    let bytes = serde_json::to_vec_pretty(meta)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    write_file_0600_atomic(path, &bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn round_trip_local() {
        let m = ProfileMetadata::new_local("p1", "My Profile", "2026-04-30T00:00:00-07:00", "abc");
        let s = serde_json::to_string(&m).unwrap();
        let back: ProfileMetadata = serde_json::from_str(&s).unwrap();
        assert_eq!(back, m);
        assert!(matches!(back.source_kind, SourceKind::Local));
        assert!(back.remote_id.is_none());
    }

    #[test]
    fn write_then_read() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("metadata.json");
        let m = ProfileMetadata::new_local("p1", "n", "t", "h");
        write_metadata(&path, &m).unwrap();
        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o7777;
        assert_eq!(mode, 0o600);
        assert_eq!(read_metadata(&path).unwrap(), m);
    }
}
