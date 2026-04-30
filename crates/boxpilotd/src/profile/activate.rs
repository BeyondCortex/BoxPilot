//! Spec §10 activation pipeline state machine. Orchestrates: lock,
//! unpack, sing-box check, atomic rename to releases/<id>, atomic
//! active-symlink swap, service restart, verify, toml commit. On
//! verify failure: rollback to previous release with second verify.
//! Surfaces four explicit terminal outcomes.

use crate::core::commit::{ActiveFields, PreviousFields, StateCommit, TomlUpdates};
use crate::dispatch::ControllerWrites;
use crate::lock;
use crate::paths::Paths;
use crate::profile::checker::SingboxChecker;
use crate::profile::gc;
use crate::profile::recovery;
use crate::profile::release::{promote_staging, read_active_target, swap_active_symlink};
use crate::profile::unpack::unpack_into;
use crate::profile::verifier::ServiceVerifier;
use crate::service::control::{self, Verb};
use crate::service::verify::{VerifyOutcome, DEFAULT_WINDOW, MAX_WINDOW};
use crate::systemd::Systemd;
use boxpilot_ipc::{
    ActivateBundleRequest, ActivateBundleResponse, ActivateOutcome, BoxpilotConfig, HelperError,
    HelperResult, UnitState, VerifySummary,
};
use chrono::Utc;
use std::os::fd::OwnedFd;
use std::path::Path;
use std::time::{Duration, Instant};
use tracing::{info, warn};

pub struct ActivateDeps<'a> {
    pub paths: Paths,
    pub systemd: &'a dyn Systemd,
    pub verifier: &'a dyn ServiceVerifier,
    pub checker: &'a dyn SingboxChecker,
}

pub async fn activate_bundle(
    req: ActivateBundleRequest,
    fd: OwnedFd,
    controller: Option<ControllerWrites>,
    deps: &ActivateDeps<'_>,
) -> HelperResult<ActivateBundleResponse> {
    let _guard = lock::try_acquire(&deps.paths.run_lock())?;

    let pre_recovery = recovery::reconcile(&deps.paths).await;
    if pre_recovery.active_corrupt {
        return Err(HelperError::ActiveCorrupt);
    }

    let cfg_text = tokio::fs::read_to_string(deps.paths.boxpilot_toml())
        .await
        .map_err(|e| HelperError::Ipc {
            message: format!("read toml: {e}"),
        })?;
    let cfg = BoxpilotConfig::parse(&cfg_text)?;
    let core_path = cfg.core_path.clone().ok_or_else(|| HelperError::Ipc {
        message: "core_path not set; install_managed first".into(),
    })?;
    let target_service = cfg.target_service.clone();

    // Use a temp nonce dir for the unpack since we only know the
    // activation_id after parsing manifest.json.
    let nonce = format!(".unpack-{}", Utc::now().format("%Y%m%dT%H%M%S%fZ"));
    let staging_root = deps.paths.staging_dir();
    tokio::fs::create_dir_all(&staging_root)
        .await
        .map_err(|e| HelperError::Ipc {
            message: format!("mkdir staging root: {e}"),
        })?;
    let temp_staging = staging_root.join(&nonce);

    let report = unpack_into(fd, &temp_staging, req.expected_total_bytes)?;
    let activation_id = report.manifest.activation_id.clone();
    let staging_path = deps.paths.staging_subdir(&activation_id);
    if staging_path.exists() {
        tokio::fs::remove_dir_all(&staging_path)
            .await
            .map_err(|e| HelperError::Ipc {
                message: format!("clean staging collision: {e}"),
            })?;
    }
    tokio::fs::rename(&temp_staging, &staging_path)
        .await
        .map_err(|e| HelperError::Ipc {
            message: format!("rename staging nonce: {e}"),
        })?;

    let check = deps
        .checker
        .check(Path::new(&core_path), &staging_path)
        .await?;
    if check.exit != 0 {
        let _ = tokio::fs::remove_dir_all(&staging_path).await;
        return Err(HelperError::SingboxCheckFailed {
            exit: check.exit,
            stderr_tail: check.stderr_tail,
        });
    }

    let release_dir = deps.paths.release_dir(&activation_id);
    promote_staging(&staging_path, &release_dir)?;

    let pre_state = deps.systemd.unit_state(&target_service).await?;
    let n_restarts_pre = match &pre_state {
        UnitState::Known { n_restarts, .. } => *n_restarts,
        UnitState::NotFound => 0,
    };

    let prev_active_target = read_active_target(&deps.paths.active_symlink());
    swap_active_symlink(&deps.paths.active_symlink(), &release_dir)?;

    if let Err(e) = control::run(Verb::Restart, &target_service, deps.systemd).await {
        warn!("restart after swap failed: {e:?}");
        return rollback_path(
            deps,
            &target_service,
            &activation_id,
            prev_active_target.as_deref(),
            cfg.active_release_id.clone(),
            req.verify_window_secs,
        )
        .await;
    }

    let window = window_from_request(req.verify_window_secs);
    let started = Instant::now();
    let verify_outcome = deps
        .verifier
        .wait_for_running(&target_service, n_restarts_pre, window, deps.systemd)
        .await?;
    let elapsed_ms = started.elapsed().as_millis() as u64;

    let post_state = deps.systemd.unit_state(&target_service).await.ok();
    let n_restarts_post = match post_state.as_ref() {
        Some(UnitState::Known { n_restarts, .. }) => *n_restarts,
        _ => n_restarts_pre,
    };
    let verify_summary = VerifySummary {
        window_used_ms: elapsed_ms,
        n_restarts_pre,
        n_restarts_post,
        final_unit_state: post_state.clone(),
    };

    match verify_outcome {
        VerifyOutcome::Running => {
            let manifest = report.manifest.clone();
            let active = ActiveFields {
                release_id: activation_id.clone(),
                profile_id: manifest.profile_id.clone(),
                profile_name: manifest.profile_name.clone(),
                profile_sha256: manifest.profile_sha256.clone(),
                activated_at: manifest.created_at.clone(),
            };
            let previous = build_previous_from_cfg(&cfg);
            let commit = StateCommit {
                paths: deps.paths.clone(),
                toml_updates: TomlUpdates {
                    active: Some(active),
                    previous,
                    ..TomlUpdates::default()
                },
                controller,
                install_state: boxpilot_ipc::InstallState::empty(),
                current_symlink_target: None,
            };
            commit.apply().await?;

            let new_cfg_text = tokio::fs::read_to_string(deps.paths.boxpilot_toml())
                .await
                .ok();
            let new_cfg = new_cfg_text
                .as_deref()
                .and_then(|t| BoxpilotConfig::parse(t).ok());
            let prev_id = new_cfg.as_ref().and_then(|c| c.previous_release_id.clone());
            let paths_for_gc = deps.paths.clone();
            let activation_id_for_gc = activation_id.clone();
            // gc::run uses blocking std::fs (recursive walks over up to 2 GiB
            // of release data) — keep it off the async executor.
            let report_gc = tokio::task::spawn_blocking(move || {
                gc::run(
                    &paths_for_gc,
                    Some(&activation_id_for_gc),
                    prev_id.as_deref(),
                )
            })
            .await
            .map_err(|e| HelperError::Ipc {
                message: format!("gc join error: {e}"),
            })?;
            if !report_gc.deleted.is_empty() {
                info!(deleted = ?report_gc.deleted, "gc completed after activation");
            }
            if report_gc.errors > 0 {
                warn!(errors = report_gc.errors, "gc had errors after activation");
            }

            Ok(ActivateBundleResponse {
                outcome: ActivateOutcome::Active,
                activation_id,
                previous_activation_id: cfg.active_release_id.clone(),
                verify: verify_summary,
            })
        }
        VerifyOutcome::NotFound => {
            // Spec §7: NotFound means the unit isn't installed — rolling back
            // would just hit the same condition. Surface the missing unit
            // honestly, but first undo the half-applied symlink swap from
            // step 7 so we don't leave toml and symlink diverged.
            // - prev_active_target = Some(p): restore to p — toml's
            //   active_release_id (unchanged, no StateCommit ran) still
            //   matches p's release id.
            // - None: this branch is only reachable on a fresh-install
            //   first activation (the reconcile pre-check enforces that
            //   toml+symlink can't be divergent at entry). Drop the symlink
            //   so we end up back in the clean fresh-install state.
            match prev_active_target.as_ref() {
                Some(p) => {
                    let _ = swap_active_symlink(&deps.paths.active_symlink(), p);
                }
                None => {
                    let _ = std::fs::remove_file(deps.paths.active_symlink());
                }
            }
            Err(HelperError::Systemd {
                message: format!(
                    "unit {target_service} not found during verify; \
                     install_managed has not run"
                ),
            })
        }
        VerifyOutcome::Stuck { .. } => {
            rollback_after_verify_failure(
                deps,
                &target_service,
                &activation_id,
                prev_active_target.as_deref(),
                cfg.active_release_id.clone(),
                verify_summary,
                req.verify_window_secs,
            )
            .await
        }
    }
}

fn window_from_request(secs: Option<u32>) -> Duration {
    match secs {
        None | Some(0) => DEFAULT_WINDOW,
        Some(s) => Duration::from_secs(s as u64).min(MAX_WINDOW),
    }
}

fn build_previous_from_cfg(cfg: &BoxpilotConfig) -> Option<PreviousFields> {
    let release_id = cfg.active_release_id.clone()?;
    let profile_id = cfg.active_profile_id.clone()?;
    let profile_sha256 = cfg.active_profile_sha256.clone()?;
    let activated_at = cfg.activated_at.clone()?;
    Some(PreviousFields {
        release_id,
        profile_id,
        profile_sha256,
        activated_at,
    })
}

async fn rollback_path(
    deps: &ActivateDeps<'_>,
    target_service: &str,
    new_id: &str,
    prev_target: Option<&Path>,
    prev_release_id: Option<String>,
    window_secs: Option<u32>,
) -> HelperResult<ActivateBundleResponse> {
    let summary = VerifySummary {
        window_used_ms: 0,
        n_restarts_pre: 0,
        n_restarts_post: 0,
        final_unit_state: None,
    };
    rollback_after_verify_failure(
        deps,
        target_service,
        new_id,
        prev_target,
        prev_release_id,
        summary,
        window_secs,
    )
    .await
}

async fn rollback_after_verify_failure(
    deps: &ActivateDeps<'_>,
    target_service: &str,
    failed_id: &str,
    prev_target: Option<&Path>,
    prev_release_id: Option<String>,
    failed_verify_summary: VerifySummary,
    window_secs: Option<u32>,
) -> HelperResult<ActivateBundleResponse> {
    let prev_target = match prev_target {
        Some(p) => p,
        None => {
            let _ = control::run(Verb::Stop, target_service, deps.systemd).await;
            // First-activation Stuck with no prior to fall back to. Drop the
            // half-applied symlink (still pointing at the failed release from
            // step 7) so we end up back in the clean fresh-install state
            // rather than leaving an orphan symlink at a non-running release.
            // The reconcile pre-check guarantees toml.active_release_id was
            // None at entry, so toml is already consistent with this end
            // state.
            let _ = std::fs::remove_file(deps.paths.active_symlink());
            return Ok(ActivateBundleResponse {
                outcome: ActivateOutcome::RollbackTargetMissing,
                activation_id: failed_id.into(),
                previous_activation_id: None,
                verify: failed_verify_summary,
            });
        }
    };

    swap_active_symlink(&deps.paths.active_symlink(), prev_target)?;
    let n_restarts_pre = match deps.systemd.unit_state(target_service).await? {
        UnitState::Known { n_restarts, .. } => n_restarts,
        UnitState::NotFound => 0,
    };
    let _ = control::run(Verb::Restart, target_service, deps.systemd).await;
    let window = window_from_request(window_secs);
    let started = Instant::now();
    let outcome = deps
        .verifier
        .wait_for_running(target_service, n_restarts_pre, window, deps.systemd)
        .await?;
    let elapsed_ms = started.elapsed().as_millis() as u64;
    let post_state = deps.systemd.unit_state(target_service).await.ok();
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
        VerifyOutcome::Running => Ok(ActivateBundleResponse {
            outcome: ActivateOutcome::RolledBack,
            activation_id: failed_id.into(),
            previous_activation_id: prev_release_id,
            verify: summary,
        }),
        VerifyOutcome::NotFound => Err(HelperError::Systemd {
            message: format!(
                "unit {target_service} not found during rollback verify; \
                 install_managed has not run"
            ),
        }),
        VerifyOutcome::Stuck { .. } => {
            let _ = control::run(Verb::Stop, target_service, deps.systemd).await;
            Ok(ActivateBundleResponse {
                outcome: ActivateOutcome::RollbackUnstartable,
                activation_id: failed_id.into(),
                previous_activation_id: prev_release_id,
                verify: summary,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profile::checker::testing::FakeChecker;
    use crate::profile::verifier::testing::ScriptedVerifier;
    use crate::systemd::testing::FixedSystemd;
    use boxpilot_ipc::{ActivationManifest, SourceKind, ACTIVATION_MANIFEST_SCHEMA_VERSION};
    use std::sync::Arc;
    use tempfile::tempdir;

    fn write_default_toml(paths: &Paths, with_active: bool) {
        std::fs::create_dir_all(paths.etc_dir()).unwrap();
        let mut text = String::from(
            "schema_version = 1\ncontroller_uid = 1000\ncore_path = \"/usr/bin/sing-box\"\n",
        );
        if with_active {
            text.push_str("active_release_id = \"old-id\"\nactive_profile_id = \"old-p\"\nactive_profile_sha256 = \"old-sha\"\nactivated_at = \"2026-04-29T00:00:00-07:00\"\n");
        }
        std::fs::write(paths.boxpilot_toml(), text).unwrap();
    }

    fn make_bundle_memfd(activation_id: &str) -> OwnedFd {
        use std::ffi::CString;
        let fd = nix::sys::memfd::memfd_create(
            CString::new("test").unwrap().as_c_str(),
            nix::sys::memfd::MemFdCreateFlag::MFD_CLOEXEC,
        )
        .expect("memfd_create");
        let manifest = ActivationManifest {
            schema_version: ACTIVATION_MANIFEST_SCHEMA_VERSION,
            activation_id: activation_id.into(),
            profile_id: "p".into(),
            profile_name: Some("TestProfile".into()),
            profile_sha256: "sha".into(),
            config_sha256: "cfgsha".into(),
            source_kind: SourceKind::Local,
            source_url_redacted: None,
            core_path_at_activation: "/usr/bin/sing-box".into(),
            core_version_at_activation: "1.10.0".into(),
            created_at: "2026-04-30T00:00:00-07:00".into(),
            assets: Vec::new(),
        };
        let mbytes = serde_json::to_vec_pretty(&manifest).unwrap();
        let mut f = std::fs::File::from(fd.try_clone().unwrap());
        let mut b = tar::Builder::new(&mut f);
        let mut h = tar::Header::new_ustar();
        h.set_size(2);
        h.set_mode(0o600);
        h.set_entry_type(tar::EntryType::Regular);
        h.set_cksum();
        b.append_data(&mut h, "config.json", &b"{}"[..]).unwrap();
        let mut h2 = tar::Header::new_ustar();
        h2.set_size(mbytes.len() as u64);
        h2.set_mode(0o600);
        h2.set_entry_type(tar::EntryType::Regular);
        h2.set_cksum();
        b.append_data(&mut h2, "manifest.json", mbytes.as_slice())
            .unwrap();
        b.finish().unwrap();
        fd
    }

    #[tokio::test]
    async fn happy_path_returns_active_and_writes_active_toml() {
        let tmp = tempdir().unwrap();
        let paths = Paths::with_root(tmp.path());
        write_default_toml(&paths, false);
        std::fs::create_dir_all(paths.run_dir()).unwrap();

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
        let checker = FakeChecker::ok();
        let deps = ActivateDeps {
            paths: paths.clone(),
            systemd: &*systemd,
            verifier: &verifier,
            checker: &checker,
        };
        let fd = make_bundle_memfd("act-1");
        let req = ActivateBundleRequest::default();
        let resp = activate_bundle(req, fd, None, &deps).await.unwrap();
        assert_eq!(resp.outcome, ActivateOutcome::Active);
        assert_eq!(resp.activation_id, "act-1");
        assert!(paths.release_dir("act-1").exists());
        assert!(!paths.staging_subdir("act-1").exists());
        let cfg = boxpilot_ipc::BoxpilotConfig::parse(
            &std::fs::read_to_string(paths.boxpilot_toml()).unwrap(),
        )
        .unwrap();
        assert_eq!(cfg.active_release_id.as_deref(), Some("act-1"));
    }

    #[tokio::test]
    async fn singbox_check_failure_aborts_and_cleans_staging() {
        let tmp = tempdir().unwrap();
        let paths = Paths::with_root(tmp.path());
        write_default_toml(&paths, false);
        std::fs::create_dir_all(paths.run_dir()).unwrap();
        let systemd = Arc::new(FixedSystemd {
            answer: UnitState::NotFound,
        });
        let verifier = ScriptedVerifier::new(vec![]);
        let checker = FakeChecker::fail();
        let deps = ActivateDeps {
            paths: paths.clone(),
            systemd: &*systemd,
            verifier: &verifier,
            checker: &checker,
        };
        let fd = make_bundle_memfd("act-2");
        let err = activate_bundle(ActivateBundleRequest::default(), fd, None, &deps)
            .await
            .unwrap_err();
        assert!(matches!(err, HelperError::SingboxCheckFailed { .. }));
        assert!(!paths.staging_subdir("act-2").exists());
        assert!(!paths.release_dir("act-2").exists());
    }

    #[tokio::test]
    async fn verify_stuck_with_no_previous_returns_rollback_target_missing() {
        let tmp = tempdir().unwrap();
        let paths = Paths::with_root(tmp.path());
        write_default_toml(&paths, false);
        std::fs::create_dir_all(paths.run_dir()).unwrap();
        let stuck_state = UnitState::Known {
            active_state: "activating".into(),
            sub_state: "auto-restart".into(),
            load_state: "loaded".into(),
            n_restarts: 5,
            exec_main_status: 1,
        };
        let systemd = Arc::new(FixedSystemd {
            answer: stuck_state.clone(),
        });
        let verifier = ScriptedVerifier::new(vec![VerifyOutcome::Stuck {
            final_state: stuck_state,
        }]);
        let checker = FakeChecker::ok();
        let deps = ActivateDeps {
            paths: paths.clone(),
            systemd: &*systemd,
            verifier: &verifier,
            checker: &checker,
        };
        let fd = make_bundle_memfd("act-3");
        let resp = activate_bundle(ActivateBundleRequest::default(), fd, None, &deps)
            .await
            .unwrap();
        assert_eq!(resp.outcome, ActivateOutcome::RollbackTargetMissing);
        assert!(paths.release_dir("act-3").exists());
        // active symlink must be removed so recovery::reconcile flags
        // active_corrupt on next startup rather than masking the dead release.
        assert!(
            !paths.active_symlink().exists()
                && std::fs::symlink_metadata(paths.active_symlink()).is_err(),
            "active symlink should not exist after RollbackTargetMissing",
        );
    }

    #[tokio::test]
    async fn verify_stuck_with_previous_rolls_back_and_returns_rolled_back() {
        let tmp = tempdir().unwrap();
        let paths = Paths::with_root(tmp.path());
        let prev = paths.release_dir("prev-id");
        std::fs::create_dir_all(&prev).unwrap();
        std::fs::create_dir_all(paths.active_symlink().parent().unwrap()).unwrap();
        std::os::unix::fs::symlink(&prev, paths.active_symlink()).unwrap();
        write_default_toml(&paths, true);
        std::fs::create_dir_all(paths.run_dir()).unwrap();

        let systemd = Arc::new(FixedSystemd {
            answer: UnitState::Known {
                active_state: "active".into(),
                sub_state: "running".into(),
                load_state: "loaded".into(),
                n_restarts: 0,
                exec_main_status: 0,
            },
        });
        let verifier = ScriptedVerifier::new(vec![
            VerifyOutcome::Stuck {
                final_state: UnitState::NotFound,
            },
            VerifyOutcome::Running,
        ]);
        let checker = FakeChecker::ok();
        let deps = ActivateDeps {
            paths: paths.clone(),
            systemd: &*systemd,
            verifier: &verifier,
            checker: &checker,
        };
        let fd = make_bundle_memfd("new-id");
        let resp = activate_bundle(ActivateBundleRequest::default(), fd, None, &deps)
            .await
            .unwrap();
        assert_eq!(resp.outcome, ActivateOutcome::RolledBack);
        assert_eq!(
            std::fs::read_link(paths.active_symlink()).unwrap(),
            prev,
            "active should be restored to previous after rollback",
        );
    }

    #[tokio::test]
    async fn rollback_unstartable_when_previous_also_fails() {
        let tmp = tempdir().unwrap();
        let paths = Paths::with_root(tmp.path());
        let prev = paths.release_dir("prev-id");
        std::fs::create_dir_all(&prev).unwrap();
        std::fs::create_dir_all(paths.active_symlink().parent().unwrap()).unwrap();
        std::os::unix::fs::symlink(&prev, paths.active_symlink()).unwrap();
        write_default_toml(&paths, true);
        std::fs::create_dir_all(paths.run_dir()).unwrap();

        let systemd = Arc::new(FixedSystemd {
            answer: UnitState::Known {
                active_state: "active".into(),
                sub_state: "running".into(),
                load_state: "loaded".into(),
                n_restarts: 0,
                exec_main_status: 0,
            },
        });
        let verifier = ScriptedVerifier::new(vec![
            VerifyOutcome::Stuck {
                final_state: UnitState::NotFound,
            },
            VerifyOutcome::Stuck {
                final_state: UnitState::NotFound,
            },
        ]);
        let checker = FakeChecker::ok();
        let deps = ActivateDeps {
            paths: paths.clone(),
            systemd: &*systemd,
            verifier: &verifier,
            checker: &checker,
        };
        let fd = make_bundle_memfd("new-id-2");
        let resp = activate_bundle(ActivateBundleRequest::default(), fd, None, &deps)
            .await
            .unwrap();
        assert_eq!(resp.outcome, ActivateOutcome::RollbackUnstartable);
    }

    #[tokio::test]
    async fn missing_core_path_returns_ipc_error() {
        let tmp = tempdir().unwrap();
        let paths = Paths::with_root(tmp.path());
        std::fs::create_dir_all(paths.etc_dir()).unwrap();
        std::fs::write(paths.boxpilot_toml(), "schema_version = 1\n").unwrap();
        std::fs::create_dir_all(paths.run_dir()).unwrap();
        let systemd = Arc::new(FixedSystemd {
            answer: UnitState::NotFound,
        });
        let verifier = ScriptedVerifier::new(vec![]);
        let checker = FakeChecker::ok();
        let deps = ActivateDeps {
            paths: paths.clone(),
            systemd: &*systemd,
            verifier: &verifier,
            checker: &checker,
        };
        let fd = make_bundle_memfd("x");
        let err = activate_bundle(ActivateBundleRequest::default(), fd, None, &deps)
            .await
            .unwrap_err();
        assert!(matches!(err, HelperError::Ipc { .. }));
    }

    #[tokio::test]
    async fn active_corrupt_blocks_activation() {
        let tmp = tempdir().unwrap();
        let paths = Paths::with_root(tmp.path());
        write_default_toml(&paths, false);
        std::fs::create_dir_all(paths.run_dir()).unwrap();
        std::fs::create_dir_all(paths.active_symlink().parent().unwrap()).unwrap();
        std::os::unix::fs::symlink(tmp.path().join("ghost"), paths.active_symlink()).unwrap();
        let systemd = Arc::new(FixedSystemd {
            answer: UnitState::NotFound,
        });
        let verifier = ScriptedVerifier::new(vec![]);
        let checker = FakeChecker::ok();
        let deps = ActivateDeps {
            paths: paths.clone(),
            systemd: &*systemd,
            verifier: &verifier,
            checker: &checker,
        };
        let fd = make_bundle_memfd("x");
        let err = activate_bundle(ActivateBundleRequest::default(), fd, None, &deps)
            .await
            .unwrap_err();
        assert!(matches!(err, HelperError::ActiveCorrupt));
    }

    #[test]
    fn window_clamped_to_max() {
        assert_eq!(window_from_request(Some(100)), MAX_WINDOW);
        assert_eq!(window_from_request(Some(0)), DEFAULT_WINDOW);
        assert_eq!(window_from_request(None), DEFAULT_WINDOW);
        assert_eq!(window_from_request(Some(7)), Duration::from_secs(7));
    }
}
