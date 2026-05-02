//! Cross-platform fakes for [`CoreAssetNaming`] + [`CoreArchive`].
//!
//! [`LinuxAssetNaming`] mirrors the real Linux formula so unit tests on the
//! Windows runner can still assert filename math. [`StubExtractor`] writes
//! a tiny shell-script-shaped fake binary at the destination path so
//! pipeline tests can proceed past extraction without depending on the
//! real `tar`/`flate2` decoders.

use crate::traits::core_assets::{CoreArchive, CoreAssetNaming};
use async_trait::async_trait;
use boxpilot_ipc::HelperError;
use std::path::Path;

pub struct LinuxAssetNaming;

impl CoreAssetNaming for LinuxAssetNaming {
    fn asset_name(&self, version: &str, arch: &str) -> String {
        format!("sing-box-{version}-linux-{arch}.tar.gz")
    }
    fn binary_name(&self) -> &'static str {
        "sing-box"
    }
}

pub struct StubExtractor;

#[async_trait]
impl CoreArchive for StubExtractor {
    async fn extract(&self, _: &Path, dest_file_path: &Path) -> Result<(), HelperError> {
        if let Some(parent) = dest_file_path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| HelperError::Ipc {
                message: format!("create parent: {e}"),
            })?;
        }
        std::fs::write(dest_file_path, b"#!/bin/sh\necho 1.10.3-fake\n").map_err(|e| {
            HelperError::Ipc {
                message: format!("write fake binary: {e}"),
            }
        })?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn linux_asset_naming_matches_upstream() {
        assert_eq!(
            LinuxAssetNaming.asset_name("1.10.0", "amd64"),
            "sing-box-1.10.0-linux-amd64.tar.gz"
        );
        assert_eq!(LinuxAssetNaming.binary_name(), "sing-box");
    }

    #[tokio::test]
    async fn stub_extractor_writes_payload() {
        let tmp = tempfile::tempdir().unwrap();
        let dest = tmp.path().join("nested").join("sing-box");
        StubExtractor
            .extract(Path::new("ignored"), &dest)
            .await
            .unwrap();
        let body = std::fs::read(&dest).unwrap();
        assert!(body.starts_with(b"#!/bin/sh"));
    }
}
