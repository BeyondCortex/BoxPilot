//! GitHub API client for SagerNet/sing-box releases. Used by core::install
//! to resolve "latest" → version and to fetch sha256sum.txt.
#![allow(dead_code)] // scaffolding-only: tasks 15-16/20 add callers

use async_trait::async_trait;
use boxpilot_ipc::{HelperError, HelperResult};
use serde::Deserialize;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

const USER_AGENT: &str = concat!("boxpilot/", env!("CARGO_PKG_VERSION"));
const LATEST_URL: &str = "https://api.github.com/repos/SagerNet/sing-box/releases/latest";
const CACHE_TTL: Duration = Duration::from_secs(300);

#[async_trait]
pub trait GithubClient: Send + Sync {
    async fn resolve_latest(&self) -> HelperResult<String>;
    async fn fetch_sha256sums(&self, version: &str) -> HelperResult<Option<String>>;
}

#[derive(Deserialize)]
struct ReleaseResponse {
    tag_name: String,
}

#[derive(Default)]
struct LatestCache {
    value: Option<(String, Instant)>,
}

pub struct ReqwestGithubClient {
    client: reqwest::Client,
    cache: Arc<Mutex<LatestCache>>,
}

impl ReqwestGithubClient {
    #[allow(dead_code)]
    pub fn new() -> HelperResult<Self> {
        let client = reqwest::Client::builder()
            .user_agent(USER_AGENT)
            .build()
            .map_err(|e| HelperError::Ipc {
                message: format!("reqwest build: {e}"),
            })?;
        Ok(Self {
            client,
            cache: Arc::new(Mutex::new(LatestCache::default())),
        })
    }
}

#[async_trait]
impl GithubClient for ReqwestGithubClient {
    #[allow(dead_code)]
    async fn resolve_latest(&self) -> HelperResult<String> {
        {
            let cache = self.cache.lock().await;
            if let Some((v, at)) = &cache.value {
                if at.elapsed() < CACHE_TTL {
                    return Ok(v.clone());
                }
            }
        }
        let body = self
            .client
            .get(LATEST_URL)
            .send()
            .await
            .map_err(|e| HelperError::Ipc {
                message: format!("github GET: {e}"),
            })?
            .error_for_status()
            .map_err(|e| HelperError::Ipc {
                message: format!("github status: {e}"),
            })?
            .text()
            .await
            .map_err(|e| HelperError::Ipc {
                message: format!("github read: {e}"),
            })?;
        let resp: ReleaseResponse = serde_json::from_str(&body).map_err(|e| HelperError::Ipc {
            message: format!("github decode: {e}"),
        })?;
        let v = resp.tag_name.trim_start_matches('v').to_string();
        let mut cache = self.cache.lock().await;
        cache.value = Some((v.clone(), Instant::now()));
        Ok(v)
    }

    #[allow(dead_code)]
    async fn fetch_sha256sums(&self, version: &str) -> HelperResult<Option<String>> {
        let url = format!(
            "https://github.com/SagerNet/sing-box/releases/download/v{version}/sing-box-{version}-checksums.txt"
        );
        let r = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e| HelperError::Ipc {
                message: format!("checksum GET: {e}"),
            })?;
        if r.status().as_u16() == 404 {
            return Ok(None);
        }
        let r = r.error_for_status().map_err(|e| HelperError::Ipc {
            message: format!("checksum status: {e}"),
        })?;
        let body = r.text().await.map_err(|e| HelperError::Ipc {
            message: format!("checksum read: {e}"),
        })?;
        Ok(Some(body))
    }
}

/// Look up `tarball_filename` in a `sha256sum.txt`-formatted body. Each
/// line is `<hex-digest>  <filename>` (note: two spaces). Returns the
/// hex digest if found.
pub fn parse_sha256sums(body: &str, tarball_filename: &str) -> Option<String> {
    for line in body.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut parts = line.splitn(2, char::is_whitespace);
        let digest = parts.next()?.trim();
        let rest = parts.next()?.trim_start();
        if rest == tarball_filename {
            return Some(digest.to_string());
        }
    }
    None
}

#[cfg(test)]
pub mod testing {
    use super::*;

    #[allow(dead_code)]
    pub struct CannedGithubClient {
        pub latest: HelperResult<String>,
        pub sha256sums: HelperResult<Option<String>>,
    }

    #[async_trait]
    impl GithubClient for CannedGithubClient {
        async fn resolve_latest(&self) -> HelperResult<String> {
            self.latest.clone()
        }
        async fn fetch_sha256sums(&self, _version: &str) -> HelperResult<Option<String>> {
            self.sha256sums.clone()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_agent_includes_crate_version() {
        assert!(USER_AGENT.starts_with("boxpilot/"));
    }

    #[test]
    fn parse_sha256sums_finds_match() {
        let body = "abc123  sing-box-1.10.0-linux-amd64.tar.gz\nfff111  other.tar.gz\n";
        assert_eq!(
            parse_sha256sums(body, "sing-box-1.10.0-linux-amd64.tar.gz"),
            Some("abc123".to_string())
        );
    }

    #[test]
    fn parse_sha256sums_returns_none_when_missing() {
        let body = "abc123  other.tar.gz\n";
        assert!(parse_sha256sums(body, "sing-box-1.10.0-linux-amd64.tar.gz").is_none());
    }

    #[test]
    fn parse_sha256sums_skips_comments_and_blanks() {
        let body = "\n# comment\nabc123  sing-box-1.10.0-linux-amd64.tar.gz\n";
        assert_eq!(
            parse_sha256sums(body, "sing-box-1.10.0-linux-amd64.tar.gz"),
            Some("abc123".to_string())
        );
    }
}
