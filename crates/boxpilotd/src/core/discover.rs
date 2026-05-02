//! Read-only enumeration of installed managed cores, adopted cores, and
//! external cores under a fixed list of canonical paths.

use crate::core::state::read_state;
use crate::core::trust::{
    default_allowed_prefixes, verify_executable_path, FsMetadataProvider, VersionChecker,
};
use boxpilot_platform::Paths;
use boxpilot_ipc::{
    CoreDiscoverResponse, CoreKind, CoreSource, DiscoveredCore, HelperError, HelperResult,
    InstallSourceJson,
};

pub struct DiscoverDeps<'a> {
    pub paths: Paths,
    pub fs: &'a dyn FsMetadataProvider,
    pub version_checker: &'a dyn VersionChecker,
}

const EXTERNAL_PROBES: &[&str] = &["/usr/bin/sing-box", "/usr/local/bin/sing-box"];

pub async fn discover(deps: &DiscoverDeps<'_>) -> HelperResult<CoreDiscoverResponse> {
    let mut cores = Vec::new();
    let cores_dir = deps.paths.cores_dir();
    if cores_dir.exists() {
        let mut entries = tokio::fs::read_dir(&cores_dir)
            .await
            .map_err(|e| HelperError::Ipc {
                message: format!("read_dir cores: {e}"),
            })?;
        while let Some(entry) = entries.next_entry().await.map_err(|e| HelperError::Ipc {
            message: format!("next_entry: {e}"),
        })? {
            let name = entry.file_name().to_string_lossy().to_string();
            if name == "current" {
                continue;
            }
            let dir = entry.path();
            if !dir.is_dir() {
                continue;
            }
            let bin = dir.join("sing-box");
            if !bin.exists() {
                continue;
            }
            let kind = if name.starts_with("adopted-") {
                CoreKind::ManagedAdopted
            } else {
                CoreKind::ManagedInstalled
            };
            let sha256 = tokio::fs::read_to_string(dir.join("sha256"))
                .await
                .map(|s| s.trim().to_string())
                .unwrap_or_default();
            let source = match tokio::fs::read_to_string(dir.join("install-source.json")).await {
                Ok(text) => serde_json::from_str::<InstallSourceJson>(&text).ok(),
                Err(_) => None,
            };
            let version = source
                .as_ref()
                .map(|s| s.version.clone())
                .unwrap_or_else(|| {
                    deps.version_checker
                        .check(&bin)
                        .ok()
                        .and_then(|s| crate::core::install::parse_singbox_version_pub(&s))
                        .unwrap_or_default()
                });
            cores.push(DiscoveredCore {
                kind,
                path: bin.to_string_lossy().to_string(),
                version,
                sha256: sha256.clone(),
                installed_at: source.as_ref().map(|s| s.installed_at.clone()),
                source: source.as_ref().map(|s| CoreSource {
                    url: s.url.clone(),
                    source_path: s.source_path.clone(),
                    upstream_sha256_match: s.upstream_sha256_match,
                    computed_sha256: s.computed_sha256_binary.clone(),
                }),
                label: name,
            });
        }
    }

    // Probe externals
    for path in EXTERNAL_PROBES {
        let p = std::path::Path::new(path);
        if !p.exists() {
            continue;
        }
        if verify_executable_path(deps.fs, p, &default_allowed_prefixes()).is_err() {
            continue;
        }
        let version = deps
            .version_checker
            .check(p)
            .ok()
            .and_then(|s| crate::core::install::parse_singbox_version_pub(&s))
            .unwrap_or_default();
        cores.push(DiscoveredCore {
            kind: CoreKind::External,
            path: path.to_string(),
            version,
            sha256: String::new(),
            installed_at: None,
            source: None,
            label: path.to_string(),
        });
    }

    let state = read_state(&deps.paths.install_state_json())
        .await
        .unwrap_or_default();
    Ok(CoreDiscoverResponse {
        cores,
        current: state.current_managed_core,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn empty_returns_empty_list() {
        struct PermissiveFs;
        impl FsMetadataProvider for PermissiveFs {
            fn stat(&self, _: &std::path::Path) -> std::io::Result<crate::core::trust::FileStat> {
                Err(std::io::Error::new(std::io::ErrorKind::NotFound, "x"))
            }
            fn read_link(&self, _: &std::path::Path) -> std::io::Result<std::path::PathBuf> {
                Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, "x"))
            }
        }
        let fs = PermissiveFs;
        let vc =
            crate::core::trust::version_testing::FixedVersionChecker::ok("sing-box version 1.10.0");
        let tmp = tempfile::tempdir().unwrap();
        let paths = Paths::with_root(tmp.path());
        let deps = DiscoverDeps {
            paths,
            fs: &fs,
            version_checker: &vc,
        };
        let resp = discover(&deps).await.unwrap();
        // /usr/bin/sing-box may exist on the host running tests; the
        // only assertion is that no managed/adopted entries appear.
        assert!(resp.cores.iter().all(|c| !matches!(
            c.kind,
            CoreKind::ManagedInstalled | CoreKind::ManagedAdopted
        )));
    }
}
