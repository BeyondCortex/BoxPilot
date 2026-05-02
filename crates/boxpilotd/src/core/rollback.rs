//! Rollback: swing `current` symlink to a previously installed managed
//! version or adopted directory. No directories are deleted.

#![cfg(target_os = "linux")]

use crate::core::commit::{StateCommit, TomlUpdates};
use crate::core::install::parse_singbox_version_pub;
use crate::core::state::read_state;
use crate::core::trust::{
    default_allowed_prefixes, verify_executable_path, FsMetadataProvider, VersionChecker,
};
use crate::dispatch::ControllerWrites;
use boxpilot_platform::traits::current::CurrentPointer;
use boxpilot_platform::Paths;
use boxpilot_ipc::{
    CoreInstallResponse, CoreKind, CoreRollbackRequest, CoreSource, CoreState, DiscoveredCore,
    HelperError, HelperResult,
};
use std::sync::Arc;

pub struct RollbackDeps<'a> {
    pub paths: Paths,
    pub fs: &'a dyn FsMetadataProvider,
    pub version_checker: &'a dyn VersionChecker,
    pub current_pointer: Arc<dyn CurrentPointer>,
}

pub async fn rollback(
    req: &CoreRollbackRequest,
    deps: &RollbackDeps<'_>,
    controller: Option<ControllerWrites>,
) -> HelperResult<CoreInstallResponse> {
    let target_dir = deps.paths.cores_dir().join(&req.to_label);
    if !target_dir.is_dir() {
        return Err(HelperError::Ipc {
            message: format!("no such core: {}", req.to_label),
        });
    }
    let bin = target_dir.join("sing-box");
    let mut prefixes = default_allowed_prefixes();
    // The cores dir base path is already under /var/lib/boxpilot/cores
    // by virtue of default_allowed_prefixes(). For test roots, also
    // include the test's cores dir explicitly.
    prefixes.push(deps.paths.cores_dir());
    verify_executable_path(deps.fs, &bin, &prefixes).map_err(|e| HelperError::Ipc {
        message: format!("trust check failed: {e}"),
    })?;
    let stdout = deps
        .version_checker
        .check(&bin)
        .map_err(|e| HelperError::Ipc {
            message: format!("version check failed: {e}"),
        })?;
    let reported = parse_singbox_version_pub(&stdout).unwrap_or_default();

    let mut state = read_state(&deps.paths.install_state_json()).await?;
    let is_adopted = req.to_label.starts_with("adopted-");
    state.current_managed_core = Some(req.to_label.clone());

    let core_state = if is_adopted {
        CoreState::ManagedAdopted
    } else {
        CoreState::ManagedInstalled
    };

    let claimed_controller = controller.is_some();
    let commit = StateCommit {
        paths: deps.paths.clone(),
        toml_updates: TomlUpdates {
            core_path: Some(
                deps.paths
                    .cores_current_symlink()
                    .join("sing-box")
                    .to_string_lossy()
                    .to_string(),
            ),
            core_state: Some(core_state),
            ..TomlUpdates::default()
        },
        controller,
        install_state: state.clone(),
        current_core_update: Some((target_dir.clone(), deps.current_pointer.clone())),
    };
    commit.apply().await?;

    let bin_sha = tokio::fs::read_to_string(target_dir.join("sha256"))
        .await
        .map(|s| s.trim().to_string())
        .unwrap_or_default();

    Ok(CoreInstallResponse {
        installed: DiscoveredCore {
            kind: if is_adopted {
                CoreKind::ManagedAdopted
            } else {
                CoreKind::ManagedInstalled
            },
            path: bin.to_string_lossy().to_string(),
            version: reported,
            sha256: bin_sha.clone(),
            installed_at: None,
            source: Some(CoreSource {
                url: None,
                source_path: None,
                upstream_sha256_match: None,
                computed_sha256: bin_sha,
            }),
            label: req.to_label.clone(),
        },
        became_current: true,
        upstream_sha256_match: None,
        claimed_controller,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn missing_label_returns_no_such_core() {
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
        let deps = RollbackDeps {
            paths,
            fs: &fs,
            version_checker: &vc,
            current_pointer: std::sync::Arc::new(
                boxpilot_platform::fakes::current::InMemoryCurrent::new(),
            ),
        };
        let req = CoreRollbackRequest {
            to_label: "1.10.0".into(),
        };
        let r = rollback(&req, &deps, None).await;
        assert!(matches!(r, Err(HelperError::Ipc { .. })));
    }
}
