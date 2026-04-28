//! Atomic state-write transaction (spec §7.2 step 14e).
//!
//! Bundles boxpilot.toml + controller-name + install-state.json +
//! cores/current symlink updates so any mid-crash interleaving leaves a
//! consistent or self-recoverable state.

use crate::dispatch::ControllerWrites;
use crate::paths::Paths;
use boxpilot_ipc::{BoxpilotConfig, CoreState, HelperError, HelperResult, InstallState};
use std::path::PathBuf;

#[derive(Debug, Clone, Default)]
pub struct TomlUpdates {
    pub core_path: Option<String>,
    pub core_state: Option<CoreState>,
}

pub struct StateCommit {
    pub paths: Paths,
    pub toml_updates: TomlUpdates,
    pub controller: Option<ControllerWrites>,
    pub install_state: InstallState,
    pub current_symlink_target: Option<PathBuf>,
}

impl StateCommit {
    pub async fn apply(self) -> HelperResult<()> {
        // 1. Stage all .new files.
        let install_state_path = self.paths.install_state_json();
        let toml_path = self.paths.boxpilot_toml();
        let controller_name_path = self.paths.controller_name_file();
        let current_symlink = self.paths.cores_current_symlink();

        // 1a. install-state.json.new
        let install_state_tmp = install_state_path.with_extension("json.new");
        if let Some(parent) = install_state_path.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|e| HelperError::Ipc {
                    message: format!("mkdir install-state parent: {e}"),
                })?;
        }
        tokio::fs::write(&install_state_tmp, self.install_state.to_json())
            .await
            .map_err(|e| HelperError::Ipc {
                message: format!("stage install-state: {e}"),
            })?;

        // 1b. current.new (if changing)
        let current_tmp = if let Some(target) = &self.current_symlink_target {
            if let Some(parent) = current_symlink.parent() {
                tokio::fs::create_dir_all(parent)
                    .await
                    .map_err(|e| HelperError::Ipc {
                        message: format!("mkdir cores parent: {e}"),
                    })?;
            }
            let tmp = current_symlink.with_extension("new");
            // Best-effort cleanup of leftover .new from a prior crash.
            let _ = tokio::fs::remove_file(&tmp).await;
            // tokio::fs has no symlink helper; defer to std for the create.
            std::os::unix::fs::symlink(target, &tmp).map_err(|e| HelperError::Ipc {
                message: format!("stage current symlink: {e}"),
            })?;
            Some(tmp)
        } else {
            None
        };

        // 1c. controller-name.new
        let controller_name_tmp = if let Some(c) = &self.controller {
            if let Some(parent) = controller_name_path.parent() {
                tokio::fs::create_dir_all(parent)
                    .await
                    .map_err(|e| HelperError::Ipc {
                        message: format!("mkdir etc parent: {e}"),
                    })?;
            }
            let tmp = controller_name_path.with_extension("name.new");
            tokio::fs::write(&tmp, format!("{}\n", c.username))
                .await
                .map_err(|e| HelperError::Ipc {
                    message: format!("stage controller-name: {e}"),
                })?;
            Some(tmp)
        } else {
            None
        };

        // 1d. boxpilot.toml.new
        let mut cfg = match tokio::fs::read_to_string(&toml_path).await {
            Ok(text) => BoxpilotConfig::parse(&text)?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => BoxpilotConfig {
                schema_version: boxpilot_ipc::CURRENT_SCHEMA_VERSION,
                target_service: "boxpilot-sing-box.service".into(),
                core_path: None,
                core_state: None,
                controller_uid: None,
                active_profile_id: None,
                active_profile_name: None,
                active_profile_sha256: None,
                active_release_id: None,
                activated_at: None,
            },
            Err(e) => {
                return Err(HelperError::Ipc {
                    message: format!("read toml: {e}"),
                })
            }
        };
        if let Some(p) = self.toml_updates.core_path {
            cfg.core_path = Some(p);
        }
        if let Some(s) = self.toml_updates.core_state {
            cfg.core_state = Some(s);
        }
        if let Some(c) = &self.controller {
            cfg.controller_uid = Some(c.uid);
        }
        let toml_tmp = toml_path.with_extension("toml.new");
        if let Some(parent) = toml_path.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|e| HelperError::Ipc {
                    message: format!("mkdir toml parent: {e}"),
                })?;
        }
        tokio::fs::write(&toml_tmp, cfg.to_toml())
            .await
            .map_err(|e| HelperError::Ipc {
                message: format!("stage toml: {e}"),
            })?;

        // 2. Commit in spec §7.2 step 14e order.
        // 2a. install-state.json
        tokio::fs::rename(&install_state_tmp, &install_state_path)
            .await
            .map_err(|e| HelperError::Ipc {
                message: format!("rename install-state: {e}"),
            })?;
        // 2b. current
        if let Some(tmp) = current_tmp {
            tokio::fs::rename(&tmp, &current_symlink)
                .await
                .map_err(|e| HelperError::Ipc {
                    message: format!("rename current: {e}"),
                })?;
        }
        // 2c. controller-name
        if let Some(tmp) = controller_name_tmp {
            tokio::fs::rename(&tmp, &controller_name_path)
                .await
                .map_err(|e| HelperError::Ipc {
                    message: format!("rename controller-name: {e}"),
                })?;
        }
        // 2d. boxpilot.toml
        tokio::fs::rename(&toml_tmp, &toml_path)
            .await
            .map_err(|e| HelperError::Ipc {
                message: format!("rename toml: {e}"),
            })?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dispatch::ControllerWrites;
    use tempfile::tempdir;

    /// Add a few helpers to Paths in test context.
    fn paths_for(tmp: &tempfile::TempDir) -> Paths {
        std::fs::create_dir_all(tmp.path().join("etc/boxpilot")).unwrap();
        std::fs::create_dir_all(tmp.path().join("var/lib/boxpilot/cores")).unwrap();
        Paths::with_root(tmp.path())
    }

    #[tokio::test]
    async fn apply_writes_install_state_and_toml() {
        let tmp = tempdir().unwrap();
        let paths = paths_for(&tmp);
        let mut state = InstallState::empty();
        state.current_managed_core = Some("1.10.0".into());
        let commit = StateCommit {
            paths: paths.clone(),
            toml_updates: TomlUpdates {
                core_path: Some("/var/lib/boxpilot/cores/current/sing-box".into()),
                core_state: Some(CoreState::ManagedInstalled),
            },
            controller: None,
            install_state: state.clone(),
            current_symlink_target: None,
        };
        commit.apply().await.unwrap();
        let saved = tokio::fs::read_to_string(paths.install_state_json())
            .await
            .unwrap();
        assert!(saved.contains(r#""current_managed_core": "1.10.0""#));
        let toml = tokio::fs::read_to_string(paths.boxpilot_toml())
            .await
            .unwrap();
        assert!(toml.contains("core_state = \"managed-installed\""));
    }

    #[tokio::test]
    async fn apply_writes_controller_name_when_claiming() {
        let tmp = tempdir().unwrap();
        let paths = paths_for(&tmp);
        let commit = StateCommit {
            paths: paths.clone(),
            toml_updates: TomlUpdates::default(),
            controller: Some(ControllerWrites {
                uid: 1000,
                username: "alice".into(),
            }),
            install_state: InstallState::empty(),
            current_symlink_target: None,
        };
        commit.apply().await.unwrap();
        let name = tokio::fs::read_to_string(paths.controller_name_file())
            .await
            .unwrap();
        assert_eq!(name.trim(), "alice");
        let toml = tokio::fs::read_to_string(paths.boxpilot_toml())
            .await
            .unwrap();
        assert!(toml.contains("controller_uid = 1000"));
    }
}
