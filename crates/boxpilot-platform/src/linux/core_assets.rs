//! Linux [`CoreAssetNaming`] + [`CoreArchive`] impls. The naming formula
//! mirrors the upstream sing-box release layout
//! (`sing-box-<version>-linux-<arch>.tar.gz`) and [`TarGzExtractor`]
//! streams the inner `sing-box` regular file into the caller-supplied
//! destination path with mode `0o755`.

use crate::traits::core_assets::{CoreArchive, CoreAssetNaming};
use async_trait::async_trait;
use boxpilot_ipc::HelperError;
use std::path::Path;

pub struct LinuxCoreAssetNaming;

impl CoreAssetNaming for LinuxCoreAssetNaming {
    fn asset_name(&self, version: &str, arch: &str) -> String {
        format!("sing-box-{version}-linux-{arch}.tar.gz")
    }
    fn binary_name(&self) -> &'static str {
        "sing-box"
    }
}

pub struct TarGzExtractor;

#[async_trait]
impl CoreArchive for TarGzExtractor {
    async fn extract(
        &self,
        archive_path: &Path,
        dest_file_path: &Path,
    ) -> Result<(), HelperError> {
        let tarball = archive_path.to_path_buf();
        let dest = dest_file_path.to_path_buf();
        // tar/flate2 are sync; do the work on a blocking thread.
        tokio::task::spawn_blocking(move || -> Result<(), HelperError> {
            let f = std::fs::File::open(&tarball).map_err(|e| HelperError::Ipc {
                message: format!("open tarball: {e}"),
            })?;
            let dec = flate2::read::GzDecoder::new(f);
            let mut ar = tar::Archive::new(dec);
            for entry in ar.entries().map_err(|e| HelperError::Ipc {
                message: format!("tar entries: {e}"),
            })? {
                let mut entry = entry.map_err(|e| HelperError::Ipc {
                    message: format!("tar entry: {e}"),
                })?;
                let is_singbox = entry
                    .path()
                    .map_err(|e| HelperError::Ipc {
                        message: format!("tar entry path: {e}"),
                    })?
                    .file_name()
                    .map(|n| n == "sing-box")
                    .unwrap_or(false);
                if is_singbox {
                    let mut out = std::fs::File::create(&dest).map_err(|e| HelperError::Ipc {
                        message: format!("create binary: {e}"),
                    })?;
                    std::io::copy(&mut entry, &mut out).map_err(|e| HelperError::Ipc {
                        message: format!("copy binary: {e}"),
                    })?;
                    use std::os::unix::fs::PermissionsExt;
                    std::fs::set_permissions(&dest, std::fs::Permissions::from_mode(0o755))
                        .map_err(|e| HelperError::Ipc {
                            message: format!("chmod binary: {e}"),
                        })?;
                    return Ok(());
                }
            }
            Err(HelperError::Ipc {
                message: "tarball did not contain a sing-box binary".into(),
            })
        })
        .await
        .map_err(|e| HelperError::Ipc {
            message: format!("spawn_blocking join: {e}"),
        })?
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fake_singbox_tarball() -> Vec<u8> {
        let mut tar_bytes = Vec::new();
        {
            let enc =
                flate2::write::GzEncoder::new(&mut tar_bytes, flate2::Compression::default());
            let mut builder = tar::Builder::new(enc);
            let mut header = tar::Header::new_gnu();
            let payload = b"stub\n";
            header.set_size(payload.len() as u64);
            header.set_mode(0o755);
            header.set_cksum();
            builder
                .append_data(&mut header, "sing-box", &payload[..])
                .unwrap();
            builder.finish().unwrap();
            let inner = builder.into_inner().unwrap();
            inner.finish().unwrap();
        }
        tar_bytes
    }

    #[test]
    fn naming_formula_matches_upstream_layout() {
        assert_eq!(
            LinuxCoreAssetNaming.asset_name("1.10.0", "amd64"),
            "sing-box-1.10.0-linux-amd64.tar.gz"
        );
        assert_eq!(LinuxCoreAssetNaming.binary_name(), "sing-box");
    }

    #[tokio::test]
    async fn tar_gz_extractor_writes_singbox_with_0755() {
        let tmp = tempfile::tempdir().unwrap();
        let tarball_path = tmp.path().join("sing-box.tar.gz");
        std::fs::write(&tarball_path, fake_singbox_tarball()).unwrap();
        let dest = tmp.path().join("sing-box");
        TarGzExtractor.extract(&tarball_path, &dest).await.unwrap();
        assert_eq!(std::fs::read(&dest).unwrap(), b"stub\n");
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&dest).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o755);
    }

    #[tokio::test]
    async fn tar_gz_extractor_rejects_archive_without_singbox() {
        let tmp = tempfile::tempdir().unwrap();
        let tarball_path = tmp.path().join("empty.tar.gz");
        // Build a tar.gz that contains a different file.
        let mut bytes = Vec::new();
        {
            let enc = flate2::write::GzEncoder::new(&mut bytes, flate2::Compression::default());
            let mut builder = tar::Builder::new(enc);
            let mut header = tar::Header::new_gnu();
            let payload = b"not-singbox\n";
            header.set_size(payload.len() as u64);
            header.set_mode(0o755);
            header.set_cksum();
            builder
                .append_data(&mut header, "other", &payload[..])
                .unwrap();
            builder.finish().unwrap();
            let inner = builder.into_inner().unwrap();
            inner.finish().unwrap();
        }
        std::fs::write(&tarball_path, &bytes).unwrap();
        let dest = tmp.path().join("sing-box");
        let r = TarGzExtractor.extract(&tarball_path, &dest).await;
        assert!(matches!(r, Err(HelperError::Ipc { .. })));
    }
}
