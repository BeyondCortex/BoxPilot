use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::Path;

use crate::store::write_file_0600_atomic;

pub const REMOTES_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoteEntry {
    /// Full URL with tokens. NEVER replicated to /etc/boxpilot.
    pub url: String,
    pub last_fetched_at: Option<String>,
    pub last_etag: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemotesFile {
    pub schema_version: u32,
    #[serde(default)]
    pub remotes: BTreeMap<String, RemoteEntry>,
}

impl Default for RemotesFile {
    fn default() -> Self {
        Self {
            schema_version: REMOTES_SCHEMA_VERSION,
            remotes: BTreeMap::new(),
        }
    }
}

pub fn read_remotes(path: &Path) -> std::io::Result<RemotesFile> {
    match std::fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(RemotesFile::default()),
        Err(e) => Err(e),
    }
}

pub fn write_remotes(path: &Path, file: &RemotesFile) -> std::io::Result<()> {
    let bytes = serde_json::to_vec_pretty(file)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    write_file_0600_atomic(path, &bytes)
}

/// Stable content-addressed remote id. Identical URL → identical id, so
/// re-adding the same URL is idempotent.
pub fn remote_id_for_url(url: &str) -> String {
    let mut h = Sha256::new();
    h.update(url.as_bytes());
    let digest = hex::encode(h.finalize());
    format!("r-{}", &digest[..16])
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    #[cfg(target_os = "linux")]
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn round_trip_default_and_populated() {
        let mut f = RemotesFile::default();
        f.remotes.insert(
            "r-abc".into(),
            RemoteEntry {
                url: "https://x?token=t".into(),
                last_fetched_at: None,
                last_etag: None,
            },
        );
        let s = serde_json::to_string(&f).unwrap();
        let back: RemotesFile = serde_json::from_str(&s).unwrap();
        assert_eq!(back, f);
    }

    #[test]
    fn read_missing_returns_empty() {
        let tmp = tempfile::tempdir().unwrap();
        let f = read_remotes(&tmp.path().join("remotes.json")).unwrap();
        assert!(f.remotes.is_empty());
        assert_eq!(f.schema_version, REMOTES_SCHEMA_VERSION);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn write_uses_0600() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("remotes.json");
        write_remotes(&path, &RemotesFile::default()).unwrap();
        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o7777;
        assert_eq!(mode, 0o600);
    }

    #[test]
    fn remote_id_is_stable_and_url_dependent() {
        let a = remote_id_for_url("https://host/p?token=AAA");
        let b = remote_id_for_url("https://host/p?token=AAA");
        let c = remote_id_for_url("https://host/p?token=BBB");
        assert_eq!(a, b);
        assert_ne!(a, c);
        assert!(a.starts_with("r-"));
    }
}
