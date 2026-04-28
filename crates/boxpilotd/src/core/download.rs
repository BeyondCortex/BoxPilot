//! Streaming download from GitHub releases. Writes to a tempfile while
//! computing SHA256 in a single pass.
#![allow(dead_code)] // scaffolding-only: task 16 adds callers

use async_trait::async_trait;
use boxpilot_ipc::{HelperError, HelperResult};
use futures_util::StreamExt;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use tokio::io::AsyncWriteExt;

#[async_trait]
pub trait Downloader: Send + Sync {
    /// Download `url` into `dest`. Returns the lowercase hex SHA256 of
    /// the downloaded bytes.
    async fn download_to_file(&self, url: &str, dest: &Path) -> HelperResult<String>;
}

const USER_AGENT: &str = concat!("boxpilot/", env!("CARGO_PKG_VERSION"));

pub struct ReqwestDownloader {
    client: reqwest::Client,
}

impl ReqwestDownloader {
    pub fn new() -> HelperResult<Self> {
        let client = reqwest::Client::builder()
            .user_agent(USER_AGENT)
            .redirect(reqwest::redirect::Policy::limited(8))
            .build()
            .map_err(|e| HelperError::Ipc {
                message: format!("reqwest build: {e}"),
            })?;
        Ok(Self { client })
    }
}

#[async_trait]
impl Downloader for ReqwestDownloader {
    async fn download_to_file(&self, url: &str, dest: &Path) -> HelperResult<String> {
        let resp = self
            .client
            .get(url)
            .send()
            .await
            .map_err(|e| HelperError::Ipc {
                message: format!("GET {url}: {e}"),
            })?
            .error_for_status()
            .map_err(|e| HelperError::Ipc {
                message: format!("status {url}: {e}"),
            })?;
        let mut f = tokio::fs::File::create(dest)
            .await
            .map_err(|e| HelperError::Ipc {
                message: format!("create {dest:?}: {e}"),
            })?;
        let mut hasher = Sha256::new();
        let mut stream = resp.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|e| HelperError::Ipc {
                message: format!("stream {url}: {e}"),
            })?;
            hasher.update(&chunk);
            f.write_all(&chunk).await.map_err(|e| HelperError::Ipc {
                message: format!("write {dest:?}: {e}"),
            })?;
        }
        f.sync_all().await.map_err(|e| HelperError::Ipc {
            message: format!("fsync {dest:?}: {e}"),
        })?;
        Ok(hex::encode(hasher.finalize()))
    }
}

#[allow(dead_code)] // suppress until install.rs wires this in
fn _unused_ref() -> PathBuf {
    PathBuf::new()
}

#[cfg(test)]
pub mod testing {
    use super::*;
    use std::sync::Mutex;

    /// Test double: writes a fixed payload to `dest` and returns the
    /// configured SHA256.
    pub struct FixedDownloader {
        pub payload: Vec<u8>,
        pub returned_sha: String,
        pub last_url: Mutex<Option<String>>,
    }

    impl FixedDownloader {
        pub fn new(payload: Vec<u8>) -> Self {
            let sha = hex::encode(sha2::Sha256::digest(&payload));
            Self {
                payload,
                returned_sha: sha,
                last_url: Mutex::new(None),
            }
        }
    }

    #[async_trait]
    impl Downloader for FixedDownloader {
        async fn download_to_file(&self, url: &str, dest: &Path) -> HelperResult<String> {
            *self.last_url.lock().unwrap() = Some(url.to_string());
            tokio::fs::write(dest, &self.payload)
                .await
                .map_err(|e| HelperError::Ipc {
                    message: format!("write: {e}"),
                })?;
            Ok(self.returned_sha.clone())
        }
    }

    #[tokio::test]
    async fn fixed_writes_payload_and_returns_sha() {
        let d = FixedDownloader::new(b"hello".to_vec());
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("payload");
        let sha = d.download_to_file("http://x/", &p).await.unwrap();
        assert_eq!(tokio::fs::read(&p).await.unwrap(), b"hello");
        assert_eq!(sha, d.returned_sha);
    }
}
