use async_trait::async_trait;
use chrono::Utc;
use std::path::Path;

use boxpilot_ipc::SourceKind;

use crate::import::{new_profile_id, sha256_hex, SINGLE_JSON_MAX_BYTES};
use crate::list::ProfileStore;
use crate::meta::{write_metadata, ProfileMetadata, METADATA_SCHEMA_VERSION};
use crate::remotes::{read_remotes, remote_id_for_url, write_remotes, RemoteEntry};
use crate::store::{ensure_dir_0700, write_file_0600_atomic};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FetchedRemote {
    pub bytes: Vec<u8>,
    pub etag: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum FetchError {
    #[error("transport: {0}")]
    Transport(String),
    #[error("body too large: {size} > {limit}")]
    TooLarge { size: u64, limit: u64 },
    #[error("body is not JSON: {0}")]
    InvalidJson(serde_json::Error),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

#[async_trait]
pub trait RemoteFetcher: Send + Sync {
    async fn fetch(&self, url: &str) -> Result<FetchedRemote, FetchError>;
}

pub struct ReqwestFetcher {
    client: reqwest::Client,
}

impl Default for ReqwestFetcher {
    fn default() -> Self {
        Self {
            client: reqwest::Client::builder()
                .user_agent(concat!("boxpilot/", env!("CARGO_PKG_VERSION")))
                .build()
                .expect("reqwest client builder"),
        }
    }
}

#[async_trait]
impl RemoteFetcher for ReqwestFetcher {
    async fn fetch(&self, url: &str) -> Result<FetchedRemote, FetchError> {
        let resp = self.client.get(url).send().await
            .map_err(|e| FetchError::Transport(e.to_string()))?;
        let etag = resp.headers().get(reqwest::header::ETAG)
            .and_then(|v| v.to_str().ok()).map(str::to_string);
        if let Some(len) = resp.content_length() {
            if len > SINGLE_JSON_MAX_BYTES {
                return Err(FetchError::TooLarge { size: len, limit: SINGLE_JSON_MAX_BYTES });
            }
        }
        let bytes = resp.bytes().await
            .map_err(|e| FetchError::Transport(e.to_string()))?;
        if (bytes.len() as u64) > SINGLE_JSON_MAX_BYTES {
            return Err(FetchError::TooLarge {
                size: bytes.len() as u64, limit: SINGLE_JSON_MAX_BYTES,
            });
        }
        Ok(FetchedRemote { bytes: bytes.to_vec(), etag })
    }
}

fn read_remotes_or_recover(path: &Path) -> crate::remotes::RemotesFile {
    use crate::remotes::RemotesFile;
    match read_remotes(path) {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => RemotesFile::default(),
        Err(e) => {
            tracing::warn!(
                error = %e,
                "remotes.json is unreadable; recreating — existing entries will be lost",
            );
            RemotesFile::default()
        }
    }
}

pub async fn import_remote(
    store: &ProfileStore,
    fetcher: &dyn RemoteFetcher,
    name: &str,
    url: &str,
) -> Result<ProfileMetadata, FetchError> {
    let fetched = fetcher.fetch(url).await?;
    serde_json::from_slice::<serde_json::Value>(&fetched.bytes)
        .map_err(FetchError::InvalidJson)?;

    // Update remotes.json with the full URL (0600).
    let remotes_path = store.paths().remotes_json();
    let mut rfile = read_remotes_or_recover(&remotes_path);
    let rid = remote_id_for_url(url);
    let now = Utc::now();
    let entry = rfile.remotes.entry(rid.clone()).or_insert(RemoteEntry {
        url: url.to_string(),
        last_fetched_at: None,
        last_etag: None,
    });
    entry.url = url.to_string();
    entry.last_fetched_at = Some(now.to_rfc3339());
    entry.last_etag = fetched.etag.clone();
    ensure_dir_0700(store.paths().root())?;
    ensure_dir_0700(&store.paths().profiles_dir())?;
    write_remotes(&remotes_path, &rfile)?;

    let id = new_profile_id(name, now);
    ensure_dir_0700(&store.paths().profile_dir(&id))?;
    ensure_dir_0700(&store.paths().profile_assets_dir(&id))?;
    write_file_0600_atomic(&store.paths().profile_source(&id), &fetched.bytes)?;

    let now_str = now.to_rfc3339();
    let m = ProfileMetadata {
        schema_version: METADATA_SCHEMA_VERSION,
        id: id.clone(),
        name: name.to_string(),
        source_kind: SourceKind::Remote,
        remote_id: Some(rid),
        created_at: now_str.clone(),
        updated_at: now_str,
        last_valid_activation_id: None,
        config_sha256: sha256_hex(&fetched.bytes),
    };
    write_metadata(&store.paths().profile_metadata(&id), &m)?;
    Ok(m)
}

/// Re-fetch an existing remote profile and overwrite `source.json` in place.
pub async fn refresh_remote(
    store: &ProfileStore,
    fetcher: &dyn RemoteFetcher,
    profile_id: &str,
) -> Result<ProfileMetadata, FetchError> {
    let mut meta = store.get(profile_id)
        .map_err(|e| FetchError::Io(std::io::Error::new(std::io::ErrorKind::NotFound, e.to_string())))?;
    let remote_id = meta.remote_id.clone()
        .ok_or_else(|| FetchError::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidInput, "profile is not a remote profile",
        )))?;

    let remotes_path = store.paths().remotes_json();
    let mut rfile = read_remotes_or_recover(&remotes_path);
    let url = rfile.remotes.get(&remote_id)
        .ok_or_else(|| FetchError::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound, "remote entry missing from remotes.json",
        )))?
        .url.clone();
    let fetched = fetcher.fetch(&url).await?;
    serde_json::from_slice::<serde_json::Value>(&fetched.bytes)
        .map_err(FetchError::InvalidJson)?;
    let now = Utc::now();
    if let Some(e) = rfile.remotes.get_mut(&remote_id) {
        e.last_fetched_at = Some(now.to_rfc3339());
        e.last_etag = fetched.etag.clone();
    }
    write_remotes(&remotes_path, &rfile)?;

    write_file_0600_atomic(&store.paths().profile_source(profile_id), &fetched.bytes)?;
    meta.updated_at = now.to_rfc3339();
    meta.config_sha256 = sha256_hex(&fetched.bytes);
    write_metadata(&store.paths().profile_metadata(profile_id), &meta)?;
    Ok(meta)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::ProfileStorePaths;
    use pretty_assertions::assert_eq;

    struct FixedFetcher { reply: FetchedRemote }
    #[async_trait]
    impl RemoteFetcher for FixedFetcher {
        async fn fetch(&self, _url: &str) -> Result<FetchedRemote, FetchError> {
            Ok(self.reply.clone())
        }
    }

    fn store_in(tmp: &tempfile::TempDir) -> ProfileStore {
        ProfileStore::new(ProfileStorePaths::new(tmp.path().to_path_buf()))
    }

    #[tokio::test]
    async fn import_remote_writes_metadata_and_remotes_json() {
        let tmp = tempfile::tempdir().unwrap();
        let s = store_in(&tmp);
        let f = FixedFetcher {
            reply: FetchedRemote { bytes: br#"{"v":1}"#.to_vec(), etag: Some("\"abc\"".into()) },
        };
        let m = import_remote(&s, &f, "Sub", "https://h/p?token=AAA").await.unwrap();
        assert!(matches!(m.source_kind, SourceKind::Remote));
        assert!(m.remote_id.is_some());

        let rfile = read_remotes(&s.paths().remotes_json()).unwrap();
        assert_eq!(rfile.remotes.len(), 1);
        let entry = rfile.remotes.values().next().unwrap();
        assert_eq!(entry.url, "https://h/p?token=AAA");
        assert!(entry.last_etag.is_some());
    }

    #[tokio::test]
    async fn import_remote_rejects_non_json() {
        let tmp = tempfile::tempdir().unwrap();
        let s = store_in(&tmp);
        let f = FixedFetcher {
            reply: FetchedRemote { bytes: b"<html>".to_vec(), etag: None },
        };
        let err = import_remote(&s, &f, "Bad", "https://h/p").await.unwrap_err();
        assert!(matches!(err, FetchError::InvalidJson(_)));
    }

    #[tokio::test]
    async fn refresh_remote_updates_in_place() {
        let tmp = tempfile::tempdir().unwrap();
        let s = store_in(&tmp);
        let f1 = FixedFetcher { reply: FetchedRemote { bytes: br#"{"v":1}"#.to_vec(), etag: None } };
        let m = import_remote(&s, &f1, "Sub", "https://h/p").await.unwrap();
        let f2 = FixedFetcher { reply: FetchedRemote { bytes: br#"{"v":2}"#.to_vec(), etag: None } };
        let m2 = refresh_remote(&s, &f2, &m.id).await.unwrap();
        assert_eq!(m2.id, m.id);
        let on_disk = std::fs::read(s.paths().profile_source(&m.id)).unwrap();
        assert!(String::from_utf8_lossy(&on_disk).contains("\"v\":2"));
    }
}
