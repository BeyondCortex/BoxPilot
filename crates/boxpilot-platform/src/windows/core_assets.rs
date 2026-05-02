//! Windows [`CoreAssetNaming`] + [`CoreArchive`] stubs. The real
//! `ZipExtractor` arrives in Sub-project #2; here it returns
//! `HelperError::NotImplemented` so callers can compile and fail
//! gracefully on Sub-project #1 Windows builds.

use crate::traits::core_assets::{CoreArchive, CoreAssetNaming};
use async_trait::async_trait;
use boxpilot_ipc::HelperError;
use std::path::Path;

pub struct WindowsCoreAssetNaming;

impl CoreAssetNaming for WindowsCoreAssetNaming {
    fn asset_name(&self, version: &str, arch: &str) -> String {
        format!("sing-box-{version}-windows-{arch}.zip")
    }
    fn binary_name(&self) -> &'static str {
        "sing-box.exe"
    }
}

pub struct ZipExtractor;

#[async_trait]
impl CoreArchive for ZipExtractor {
    async fn extract(&self, _: &Path, _: &Path) -> Result<(), HelperError> {
        Err(HelperError::NotImplemented)
    }
}
