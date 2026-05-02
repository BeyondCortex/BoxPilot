//! Naming and extraction of upstream sing-box release archives.
//!
//! Linux: `sing-box-<version>-linux-<arch>.tar.gz` extracted to a flat dir.
//! Windows: `sing-box-<version>-windows-<arch>.zip` (Sub-project #2).
//! Per spec §11.3 + §5.

use async_trait::async_trait;
use boxpilot_ipc::HelperError;
use std::path::Path;

pub trait CoreAssetNaming: Send + Sync {
    fn asset_name(&self, version: &str, arch: &str) -> String;
    fn binary_name(&self) -> &'static str;
}

#[async_trait]
pub trait CoreArchive: Send + Sync {
    /// Extract the core binary from `archive_path` into `dest_file_path`
    /// (the exact destination filename — caller has created the parent
    /// dir and is responsible for any post-extraction fsync/promotion).
    async fn extract(
        &self,
        archive_path: &Path,
        dest_file_path: &Path,
    ) -> Result<(), HelperError>;
}
