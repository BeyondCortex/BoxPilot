//! Spec §10 manual rollback. Differs from auto-rollback in two ways:
//! (1) caller picks a specific historical activation_id, (2) on
//! verify success the toml swap is symmetric — what was active becomes
//! previous. GC does not run inside this verb.

use crate::core::commit::{ActiveFields, PreviousFields, StateCommit, TomlUpdates};
use crate::lock;
use crate::paths::Paths;
use crate::profile::recovery;
use crate::profile::release::{read_active_target, swap_active_symlink};
use crate::profile::verifier::ServiceVerifier;
use crate::service::control::{self, Verb};
use crate::service::verify::VerifyOutcome;
use crate::systemd::Systemd;
use boxpilot_ipc::{
    ActivateBundleResponse, ActivateOutcome, ActivationManifest, BoxpilotConfig, HelperError,
    HelperResult, RollbackRequest, UnitState, VerifySummary,
};
use chrono::Utc;
use std::time::{Duration, Instant};

pub struct RollbackDeps<'a> {
    pub paths: Paths,
    pub systemd: &'a dyn Systemd,
    pub verifier: &'a dyn ServiceVerifier,
}

pub async fn rollback_release(
    req: RollbackRequest,
    deps: &RollbackDeps<'_>,
) -> HelperResult<ActivateBundleResponse> {
    let _guard = lock::try_acquire(&deps.paths.run_lock())?;

    if recovery::reconcile(&deps.paths).await.active_corrupt {
        return Err(HelperError::ActiveCorrupt);
    }

    let cfg_text = tokio::fs::read_to_string(deps.paths.boxpilot_toml())
        .await
        .map_err(|e| HelperError::Ipc {
            message: format!("read toml: {e}"),
        })?;
    let cfg = BoxpilotConfig::parse(&cfg_text)?;
    let target_service = cfg.target_service.clone();

    let target_id = &req.target_activation_id;
    if cfg.active_release_id.as_deref() == Some(target_id.as_str()) {
        return Err(HelperError::ReleaseAlreadyActive);
    }
    let target_dir = deps.paths.release_dir(target_id);
    if !target_dir.exists() {
        return Err(HelperError::ReleaseNotFound {
            activation_id: target_id.clone(),
        });
    }
    let manifest_path = target_dir.join("manifest.json");
    let manifest_bytes = tokio::fs::read(&manifest_path)
        .await
        .map_err(|e| HelperError::Ipc {
            message: format!("read target manifest: {e}"),
        })?;
    let target_manifest: ActivationManifest = serde_json::from_slice(&manifest_bytes)
        .map_err(|e| HelperError::Ipc {
            message: format!("parse target manifest: {e}"),
        })?;

    let prev_active_target = read_active_target(&deps.paths.active_symlink());

    let pre_state = deps.systemd.unit_state(&target_service).await?;
    let n_restarts_pre = match &pre_state {
        UnitState::Known { n_restarts, .. } => *n_restarts,
        UnitState::NotFound => 0,
    };

    swap_active_symlink(&deps.paths.active_symlink(), &target_dir)?;
    let _ = control::run(Verb::Restart, &target_service, deps.systemd).await;

    let window = window_from_request(req.verify_window_secs);
    let started = Instant::now();
    let outcome = deps
        .verifier
        .wait_for_running(&target_service, n_restarts_pre, window, deps.systemd)
        .await?;
    let elapsed_ms = started.elapsed().as_millis() as u64;
    let post_state = deps.systemd.unit_state(&target_service).await.ok();
    let n_restarts_post = match post_state.as_ref() {
        Some(UnitState::Known { n_restarts, .. }) => *n_restarts,
        _ => n_restarts_pre,
    };
    let summary = VerifySummary {
        window_used_ms: elapsed_ms,
        n_restarts_pre,
        n_restarts_post,
        final_unit_state: post_state.clone(),
    };

    match outcome {
        VerifyOutcome::Running => {
            let active = ActiveFields {
                release_id: target_id.clone(),
                profile_id: target_manifest.profile_id.clone(),
                profile_name: None,
                profile_sha256: target_manifest.profile_sha256.clone(),
                activated_at: Utc::now().to_rfc3339(),
            };
            let previous = if let (Some(rid), Some(pid), Some(psha), Some(at)) = (
                cfg.active_release_id.clone(),
                cfg.active_profile_id.clone(),
                cfg.active_profile_sha256.clone(),
                cfg.activated_at.clone(),
            ) {
                Some(PreviousFields {
                    release_id: rid,
                    profile_id: pid,
                    profile_sha256: psha,
                    activated_at: at,
                })
            } else {
                None
            };
            let commit = StateCommit {
                paths: deps.paths.clone(),
                toml_updates: TomlUpdates {
                    active: Some(active),
                    previous,
                    ..TomlUpdates::default()
                },
                controller: None,
                install_state: boxpilot_ipc::InstallState::empty(),
                current_symlink_target: None,
            };
            commit.apply().await?;
            Ok(ActivateBundleResponse {
                outcome: ActivateOutcome::Active,
                activation_id: target_id.clone(),
                previous_activation_id: cfg.active_release_id.clone(),
                verify: summary,
            })
        }
        VerifyOutcome::Stuck { .. } | VerifyOutcome::NotFound => {
            let restore_target = match prev_active_target {
                Some(p) => p,
                None => {
                    let _ = control::run(Verb::Stop, &target_service, deps.systemd).await;
                    return Ok(ActivateBundleResponse {
                        outcome: ActivateOutcome::RollbackTargetMissing,
                        activation_id: target_id.clone(),
                        previous_activation_id: cfg.active_release_id.clone(),
                        verify: summary,
                    });
                }
            };
            swap_active_symlink(&deps.paths.active_symlink(), &restore_target)?;
            let _ = control::run(Verb::Restart, &target_service, deps.systemd).await;
            let started2 = Instant::now();
            let restore_outcome = deps
                .verifier
                .wait_for_running(&target_service, n_restarts_post, window, deps.systemd)
                .await?;
            let elapsed2 = started2.elapsed().as_millis() as u64;
            let post2 = deps.systemd.unit_state(&target_service).await.ok();
            let final_summary = VerifySummary {
                window_used_ms: elapsed2,
                n_restarts_pre: n_restarts_post,
                n_restarts_post: match post2.as_ref() {
                    Some(UnitState::Known { n_restarts, .. }) => *n_restarts,
                    _ => n_restarts_post,
                },
                final_unit_state: post2,
            };
            if !matches!(restore_outcome, VerifyOutcome::Running) {
                let _ = control::run(Verb::Stop, &target_service, deps.systemd).await;
            }
            Ok(ActivateBundleResponse {
                outcome: ActivateOutcome::RollbackUnstartable,
                activation_id: target_id.clone(),
                previous_activation_id: cfg.active_release_id.clone(),
                verify: final_summary,
            })
        }
    }
}

fn window_from_request(secs: Option<u32>) -> Duration {
    use crate::service::verify::{DEFAULT_WINDOW, MAX_WINDOW};
    match secs {
        None | Some(0) => DEFAULT_WINDOW,
        Some(s) => Duration::from_secs(s as u64).min(MAX_WINDOW),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profile::verifier::testing::ScriptedVerifier;
    use crate::systemd::testing::FixedSystemd;
    use boxpilot_ipc::{
        ActivationManifest, SourceKind, ACTIVATION_MANIFEST_SCHEMA_VERSION,
    };
    use std::sync::Arc;
    use tempfile::tempdir;

    fn write_release(paths: &Paths, id: &str, profile_id: &str) {
        let dir = paths.release_dir(id);
        std::fs::create_dir_all(&dir).unwrap();
        let m = ActivationManifest {
            schema_version: ACTIVATION_MANIFEST_SCHEMA_VERSION,
            activation_id: id.into(),
            profile_id: profile_id.into(),
            profile_sha256: "psha".into(),
            config_sha256: "csha".into(),
            source_kind: SourceKind::Local,
            source_url_redacted: None,
            core_path_at_activation: "/usr/bin/sing-box".into(),
            core_version_at_activation: "1.10.0".into(),
            created_at: "2026-04-30T00:00:00-07:00".into(),
            assets: Vec::new(),
        };
        std::fs::write(
            dir.join("manifest.json"),
            serde_json::to_vec_pretty(&m).unwrap(),
        )
        .unwrap();
    }

    #[tokio::test]
    async fn rollback_target_missing_returns_release_not_found() {
        let tmp = tempdir().unwrap();
        let paths = Paths::with_root(tmp.path());
        std::fs::create_dir_all(paths.etc_dir()).unwrap();
        std::fs::write(
            paths.boxpilot_toml(),
            "schema_version = 1\nactive_release_id = \"cur\"\n",
        )
        .unwrap();
        std::fs::create_dir_all(paths.run_dir()).unwrap();
        let systemd = Arc::new(FixedSystemd {
            answer: UnitState::NotFound,
        });
        let verifier = ScriptedVerifier::new(vec![]);
        let deps = RollbackDeps {
            paths: paths.clone(),
            systemd: &*systemd,
            verifier: &verifier,
        };
        let err = rollback_release(
            RollbackRequest {
                target_activation_id: "ghost".into(),
                verify_window_secs: None,
            },
            &deps,
        )
        .await
        .unwrap_err();
        assert!(matches!(err, HelperError::ReleaseNotFound { .. }));
    }

    #[tokio::test]
    async fn rollback_to_already_active_release_is_refused() {
        let tmp = tempdir().unwrap();
        let paths = Paths::with_root(tmp.path());
        std::fs::create_dir_all(paths.etc_dir()).unwrap();
        std::fs::write(
            paths.boxpilot_toml(),
            "schema_version = 1\nactive_release_id = \"cur\"\n",
        )
        .unwrap();
        std::fs::create_dir_all(paths.run_dir()).unwrap();
        write_release(&paths, "cur", "pcur");
        let systemd = Arc::new(FixedSystemd {
            answer: UnitState::NotFound,
        });
        let verifier = ScriptedVerifier::new(vec![]);
        let deps = RollbackDeps {
            paths: paths.clone(),
            systemd: &*systemd,
            verifier: &verifier,
        };
        let err = rollback_release(
            RollbackRequest {
                target_activation_id: "cur".into(),
                verify_window_secs: None,
            },
            &deps,
        )
        .await
        .unwrap_err();
        assert!(matches!(err, HelperError::ReleaseAlreadyActive));
    }

    #[tokio::test]
    async fn rollback_happy_path_swaps_active_and_writes_toml() {
        let tmp = tempdir().unwrap();
        let paths = Paths::with_root(tmp.path());
        std::fs::create_dir_all(paths.etc_dir()).unwrap();
        std::fs::write(
            paths.boxpilot_toml(),
            "schema_version = 1\nactive_release_id = \"cur\"\nactive_profile_id = \"pcur\"\nactive_profile_sha256 = \"sha-cur\"\nactivated_at = \"2026-04-29T00:00:00-07:00\"\n",
        )
        .unwrap();
        std::fs::create_dir_all(paths.run_dir()).unwrap();
        std::fs::create_dir_all(paths.active_symlink().parent().unwrap()).unwrap();
        write_release(&paths, "cur", "pcur");
        write_release(&paths, "tgt", "ptgt");
        std::os::unix::fs::symlink(paths.release_dir("cur"), paths.active_symlink()).unwrap();

        let systemd = Arc::new(FixedSystemd {
            answer: UnitState::Known {
                active_state: "active".into(),
                sub_state: "running".into(),
                load_state: "loaded".into(),
                n_restarts: 0,
                exec_main_status: 0,
            },
        });
        let verifier = ScriptedVerifier::new(vec![VerifyOutcome::Running]);
        let deps = RollbackDeps {
            paths: paths.clone(),
            systemd: &*systemd,
            verifier: &verifier,
        };
        let resp = rollback_release(
            RollbackRequest {
                target_activation_id: "tgt".into(),
                verify_window_secs: Some(2),
            },
            &deps,
        )
        .await
        .unwrap();
        assert_eq!(resp.outcome, ActivateOutcome::Active);
        assert_eq!(resp.activation_id, "tgt");
        let cfg = boxpilot_ipc::BoxpilotConfig::parse(
            &std::fs::read_to_string(paths.boxpilot_toml()).unwrap(),
        )
        .unwrap();
        assert_eq!(cfg.active_release_id.as_deref(), Some("tgt"));
        assert_eq!(cfg.previous_release_id.as_deref(), Some("cur"));
    }
}
