//! `service.install_managed` (§6.3): generate the unit text, write it
//! atomically to `/etc/systemd/system/boxpilot-sing-box.service`, then
//! `daemon-reload` so a subsequent `start_unit` finds it.
//!
//! This module assumes the caller (iface.rs) has already gone through
//! `dispatch::authorize` and is holding `/run/boxpilot/lock`.

use crate::core::trust::{
    default_allowed_prefixes, verify_executable_path, FsMetadataProvider, TrustError,
};
use crate::paths::Paths;
use crate::service::unit;
use crate::systemd::Systemd;
use boxpilot_ipc::{
    BoxpilotConfig, HelperError, HelperResult, ServiceInstallManagedResponse,
};
use std::path::PathBuf;

pub struct InstallDeps<'a> {
    pub paths: Paths,
    pub systemd: &'a dyn Systemd,
    pub fs: &'a dyn FsMetadataProvider,
}

pub async fn install_managed(
    cfg: &BoxpilotConfig,
    deps: &InstallDeps<'_>,
) -> HelperResult<ServiceInstallManagedResponse> {
    let core_path_str = cfg.core_path.as_deref().ok_or_else(|| HelperError::Ipc {
        message: "no core configured — install or adopt a core first".into(),
    })?;
    let core_path = PathBuf::from(core_path_str);

    // Trust check on the core path *before* baking it into a root-run unit.
    let prefixes = default_allowed_prefixes();
    verify_executable_path(deps.fs, &core_path, &prefixes).map_err(map_trust_err)?;

    let unit_text = unit::render(&core_path);
    let target = deps.paths.systemd_unit_path(&cfg.target_service);
    write_unit_atomic(&target, &unit_text).await?;

    deps.systemd.reload().await?;
    let unit_state = deps.systemd.unit_state(&cfg.target_service).await?;

    Ok(ServiceInstallManagedResponse {
        unit_state,
        generated_unit_path: target.to_string_lossy().to_string(),
        // The dispatch chokepoint owns the controller-claim decision; the
        // iface wrapper sets this field after install_managed returns.
        claimed_controller: false,
    })
}

async fn write_unit_atomic(target: &std::path::Path, text: &str) -> HelperResult<()> {
    if let Some(parent) = target.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|e| HelperError::Ipc {
                message: format!("mkdir unit parent: {e}"),
            })?;
    }
    let tmp = target.with_extension("service.new");
    tokio::fs::write(&tmp, text)
        .await
        .map_err(|e| HelperError::Ipc {
            message: format!("write unit: {e}"),
        })?;
    let f = tokio::fs::OpenOptions::new()
        .read(true)
        .open(&tmp)
        .await
        .map_err(|e| HelperError::Ipc {
            message: format!("open unit for fsync: {e}"),
        })?;
    f.sync_all().await.map_err(|e| HelperError::Ipc {
        message: format!("fsync unit: {e}"),
    })?;
    tokio::fs::rename(&tmp, target)
        .await
        .map_err(|e| HelperError::Ipc {
            message: format!("rename unit: {e}"),
        })?;
    Ok(())
}

fn map_trust_err(e: TrustError) -> HelperError {
    HelperError::Ipc {
        message: format!("trust check failed: {e}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::trust::{FileKind, FileStat};
    use crate::systemd::testing::{FixedSystemd, RecordingSystemd};
    use boxpilot_ipc::{CoreState, UnitState};
    use std::path::Path;
    use tempfile::tempdir;

    /// A permissive FS that says every probed file is a root-owned regular
    /// file with mode 0o755 — sufficient to pass the §6.5 trust check
    /// against the staged `cores/current/sing-box` symlink target.
    struct PermissiveFs;
    impl FsMetadataProvider for PermissiveFs {
        fn stat(&self, p: &Path) -> std::io::Result<FileStat> {
            let kind = if p.file_name().map(|n| n == "sing-box").unwrap_or(false) {
                FileKind::Regular
            } else {
                FileKind::Directory
            };
            Ok(FileStat { uid: 0, gid: 0, mode: 0o755, kind })
        }
        fn read_link(&self, _: &Path) -> std::io::Result<std::path::PathBuf> {
            Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "no symlinks",
            ))
        }
    }

    fn cfg_with_core(path: &str) -> BoxpilotConfig {
        BoxpilotConfig {
            schema_version: 1,
            target_service: "boxpilot-sing-box.service".into(),
            core_path: Some(path.into()),
            core_state: Some(CoreState::ManagedInstalled),
            controller_uid: Some(1000),
            active_profile_id: None,
            active_profile_name: None,
            active_profile_sha256: None,
            active_release_id: None,
            activated_at: None,
        }
    }

    #[tokio::test]
    async fn install_writes_unit_file_and_reloads() {
        let tmp = tempdir().unwrap();
        let paths = Paths::with_root(tmp.path());
        let systemd = RecordingSystemd::new(UnitState::Known {
            active_state: "inactive".into(),
            sub_state: "dead".into(),
            load_state: "loaded".into(),
            n_restarts: 0,
            exec_main_status: 0,
        });
        let fs = PermissiveFs;
        let cfg = cfg_with_core("/usr/bin/sing-box");
        let deps = InstallDeps { paths: paths.clone(), systemd: &systemd, fs: &fs };

        let resp = install_managed(&cfg, &deps).await.unwrap();

        let written = tokio::fs::read_to_string(paths.systemd_unit_path("boxpilot-sing-box.service")).await.unwrap();
        assert!(written.contains("ExecStart=/usr/bin/sing-box run -c config.json"));
        assert!(matches!(resp.unit_state, UnitState::Known { .. }));
        let calls = systemd.calls();
        assert!(
            calls.iter().any(|c| matches!(c, crate::systemd::testing::RecordedCall::Reload)),
            "expected daemon-reload, got {calls:?}"
        );
    }

    #[tokio::test]
    async fn install_without_core_path_returns_explicit_error() {
        let tmp = tempdir().unwrap();
        let paths = Paths::with_root(tmp.path());
        let systemd = FixedSystemd { answer: UnitState::NotFound };
        let fs = PermissiveFs;
        let mut cfg = cfg_with_core("/x");
        cfg.core_path = None;
        let deps = InstallDeps { paths, systemd: &systemd, fs: &fs };
        let r = install_managed(&cfg, &deps).await;
        assert!(matches!(r, Err(HelperError::Ipc { .. })));
    }

    #[tokio::test]
    async fn install_with_untrusted_core_path_aborts_before_writing() {
        let tmp = tempdir().unwrap();
        let paths = Paths::with_root(tmp.path());
        let systemd = FixedSystemd { answer: UnitState::NotFound };
        // Reject everything — simulates §6.5 failure.
        struct DenyFs;
        impl FsMetadataProvider for DenyFs {
            fn stat(&self, _: &Path) -> std::io::Result<FileStat> {
                Err(std::io::Error::new(std::io::ErrorKind::NotFound, "denied"))
            }
            fn read_link(&self, _: &Path) -> std::io::Result<std::path::PathBuf> {
                Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, "x"))
            }
        }
        let fs = DenyFs;
        let cfg = cfg_with_core("/home/evil/sing-box");
        let deps = InstallDeps { paths: paths.clone(), systemd: &systemd, fs: &fs };
        let r = install_managed(&cfg, &deps).await;
        assert!(matches!(r, Err(HelperError::Ipc { .. })));
        // Critical: the unit file must NOT have been written.
        assert!(!paths.systemd_unit_path("boxpilot-sing-box.service").exists());
    }
}
