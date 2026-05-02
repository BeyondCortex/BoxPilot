//! Adopt an existing root-owned sing-box binary into BoxPilot's managed
//! tree. Does NOT swing `current` (spec §5.2).

use crate::core::commit::{StateCommit, TomlUpdates};
use crate::core::install::{parse_singbox_version_pub, sha256_file_pub};
use crate::core::state::read_state;
use crate::core::trust::{
    default_allowed_prefixes, verify_executable_path, FsMetadataProvider, VersionChecker,
};
use crate::dispatch::ControllerWrites;
use boxpilot_platform::Paths;
use boxpilot_ipc::{
    AdoptedCoreEntry, CoreAdoptRequest, CoreInstallResponse, CoreKind, CoreSource, DiscoveredCore,
    HelperError, HelperResult, InstallSourceJson,
};
use chrono::Utc;

pub struct AdoptDeps<'a> {
    pub paths: Paths,
    pub fs: &'a dyn FsMetadataProvider,
    pub version_checker: &'a dyn VersionChecker,
}

pub async fn adopt(
    req: &CoreAdoptRequest,
    deps: &AdoptDeps<'_>,
    controller: Option<ControllerWrites>,
) -> HelperResult<CoreInstallResponse> {
    let prefixes = default_allowed_prefixes(); // §6.5; rejects /home etc.
    let resolved =
        verify_executable_path(deps.fs, std::path::Path::new(&req.source_path), &prefixes)
            .map_err(|e| HelperError::Ipc {
                message: format!("trust check failed: {e}"),
            })?;
    let stdout = deps
        .version_checker
        .check(&resolved)
        .map_err(|e| HelperError::Ipc {
            message: format!("version check failed: {e}"),
        })?;
    let reported = parse_singbox_version_pub(&stdout).ok_or_else(|| HelperError::Ipc {
        message: format!("could not parse version from: {stdout}"),
    })?;
    let label = format!("adopted-{}", Utc::now().format("%Y-%m-%dT%H-%M-%SZ"));

    // Stage
    let staging = deps
        .paths
        .cores_staging_dir()
        .join(format!("{label}-{}", random_suffix()));
    tokio::fs::create_dir_all(&staging)
        .await
        .map_err(|e| HelperError::Ipc {
            message: format!("mkdir staging: {e}"),
        })?;
    let bin_dest = staging.join("sing-box");
    tokio::fs::copy(&resolved, &bin_dest)
        .await
        .map_err(|e| HelperError::Ipc {
            message: format!("copy {resolved:?}: {e}"),
        })?;
    use std::os::unix::fs::PermissionsExt;
    let perms = std::fs::Permissions::from_mode(0o755);
    tokio::fs::set_permissions(&bin_dest, perms)
        .await
        .map_err(|e| HelperError::Ipc {
            message: format!("chmod: {e}"),
        })?;

    // §7.3 step 5 defense-in-depth: re-verify the staged binary now that
    // the kernel has it under a path we control. Closes the TOCTTOU
    // window between the source-path check above and the copy.
    let mut staging_prefixes = default_allowed_prefixes();
    staging_prefixes.push(deps.paths.cores_staging_dir());
    verify_executable_path(deps.fs, &bin_dest, &staging_prefixes).map_err(|e| {
        HelperError::Ipc {
            message: format!("staged trust check failed: {e}"),
        }
    })?;

    let bin_sha = sha256_file_pub(&bin_dest).await?;

    let install_source = InstallSourceJson {
        schema_version: 1,
        kind: CoreKind::ManagedAdopted,
        version: reported.clone(),
        architecture: detect_arch_label()?,
        url: None,
        source_path: Some(req.source_path.clone()),
        upstream_sha256_match: None,
        computed_sha256_tarball: None,
        computed_sha256_binary: bin_sha.clone(),
        installed_at: Utc::now().to_rfc3339(),
        user_agent_used: format!("boxpilot/{}", env!("CARGO_PKG_VERSION")),
    };
    tokio::fs::write(staging.join("sha256"), &bin_sha)
        .await
        .map_err(|e| HelperError::Ipc {
            message: format!("write sha256: {e}"),
        })?;
    tokio::fs::write(
        staging.join("install-source.json"),
        serde_json::to_string_pretty(&install_source).unwrap(),
    )
    .await
    .map_err(|e| HelperError::Ipc {
        message: format!("write install-source.json: {e}"),
    })?;

    // Promote
    tokio::fs::create_dir_all(deps.paths.cores_dir())
        .await
        .map_err(|e| HelperError::Ipc {
            message: format!("create cores_dir: {e}"),
        })?;
    let target_dir = deps.paths.cores_dir().join(&label);
    tokio::fs::rename(&staging, &target_dir)
        .await
        .map_err(|e| HelperError::Ipc {
            message: format!("promote {staging:?} -> {target_dir:?}: {e}"),
        })?;

    let mut state = read_state(&deps.paths.install_state_json()).await?;
    state.adopted_cores.push(AdoptedCoreEntry {
        label: label.clone(),
        path: target_dir.join("sing-box").to_string_lossy().to_string(),
        sha256: bin_sha.clone(),
        adopted_from: req.source_path.clone(),
        adopted_at: install_source.installed_at.clone(),
    });

    let claimed_controller = controller.is_some();
    let commit = StateCommit {
        paths: deps.paths.clone(),
        toml_updates: TomlUpdates::default(), // adopt does NOT change core_path/state
        controller,
        install_state: state.clone(),
        current_symlink_target: None,
    };
    commit.apply().await?;

    Ok(CoreInstallResponse {
        installed: DiscoveredCore {
            kind: CoreKind::ManagedAdopted,
            path: target_dir.join("sing-box").to_string_lossy().to_string(),
            version: reported,
            sha256: bin_sha.clone(),
            installed_at: Some(install_source.installed_at.clone()),
            source: Some(CoreSource {
                url: None,
                source_path: Some(req.source_path.clone()),
                upstream_sha256_match: None,
                computed_sha256: bin_sha,
            }),
            label,
        },
        became_current: false,
        upstream_sha256_match: None,
        claimed_controller,
    })
}

fn detect_arch_label() -> HelperResult<String> {
    let out = std::process::Command::new("uname")
        .arg("-m")
        .output()
        .map_err(|e| HelperError::Ipc {
            message: format!("uname: {e}"),
        })?;
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

fn random_suffix() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    format!(
        "{:08x}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .subsec_nanos()
    )
}

#[cfg(test)]
mod adopt_tests {
    // Single happy-path test using a real existing binary in a tempdir
    // is too slow / non-portable; instead rely on integration coverage
    // through the mocked install pipeline. Add a unit test for
    // detect_arch_label format.
    use super::detect_arch_label;

    #[test]
    fn detect_arch_label_returns_nonempty() {
        let s = detect_arch_label().unwrap();
        assert!(!s.is_empty());
    }
}
