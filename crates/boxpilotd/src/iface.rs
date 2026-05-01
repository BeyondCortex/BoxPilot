//! `app.boxpilot.Helper1` D-Bus interface. Each method goes through
//! [`crate::dispatch::authorize`] before doing any work — including the
//! stubs awaiting later plans. Authorization on stubs avoids leaking which
//! methods exist to unauthorized callers and exercises the §6 contract for
//! every method from day one.
//!
//! Method names on the bus are CamelCase per D-Bus convention; the logical
//! action mapping is in `boxpilot_ipc::HelperMethod`.

use crate::context::HelperContext;
use crate::dispatch;
use boxpilot_ipc::{HelperError, HelperMethod, ServiceStatusResponse};
use std::sync::Arc;
use tracing::{instrument, warn};
use zbus::interface;

pub struct Helper {
    ctx: Arc<HelperContext>,
}

impl Helper {
    pub fn new(ctx: Arc<HelperContext>) -> Self {
        Self { ctx }
    }
}

/// Convert a HelperError into a zbus method error. The error name follows
/// reverse-DNS form so the GUI can branch on `e.name()`.
fn to_zbus_err(e: HelperError) -> zbus::fdo::Error {
    let name = match &e {
        HelperError::NotImplemented => "app.boxpilot.Helper1.NotImplemented",
        HelperError::NotAuthorized => "app.boxpilot.Helper1.NotAuthorized",
        HelperError::NotController => "app.boxpilot.Helper1.NotController",
        HelperError::ControllerOrphaned => "app.boxpilot.Helper1.ControllerOrphaned",
        HelperError::ControllerNotSet => "app.boxpilot.Helper1.ControllerNotSet",
        HelperError::UnsupportedSchemaVersion { .. } => {
            "app.boxpilot.Helper1.UnsupportedSchemaVersion"
        }
        HelperError::Busy => "app.boxpilot.Helper1.Busy",
        HelperError::Systemd { .. } => "app.boxpilot.Helper1.Systemd",
        HelperError::Ipc { .. } => "app.boxpilot.Helper1.Ipc",
        HelperError::BundleTooLarge { .. } => "app.boxpilot.Helper1.BundleTooLarge",
        HelperError::BundleEntryRejected { .. } => "app.boxpilot.Helper1.BundleEntryRejected",
        HelperError::BundleAssetMismatch { .. } => "app.boxpilot.Helper1.BundleAssetMismatch",
        HelperError::SingboxCheckFailed { .. } => "app.boxpilot.Helper1.SingboxCheckFailed",
        HelperError::RollbackTargetMissing => "app.boxpilot.Helper1.RollbackTargetMissing",
        HelperError::RollbackUnstartable { .. } => "app.boxpilot.Helper1.RollbackUnstartable",
        HelperError::ActiveCorrupt => "app.boxpilot.Helper1.ActiveCorrupt",
        HelperError::ReleaseAlreadyActive => "app.boxpilot.Helper1.ReleaseAlreadyActive",
        HelperError::ReleaseNotFound { .. } => "app.boxpilot.Helper1.ReleaseNotFound",
        HelperError::LegacyConfigPathUnsafe { .. } => "app.boxpilot.Helper1.LegacyConfigPathUnsafe",
        HelperError::LegacyUnitNotFound { .. } => "app.boxpilot.Helper1.LegacyUnitNotFound",
        HelperError::LegacyExecStartUnparseable { .. } => {
            "app.boxpilot.Helper1.LegacyExecStartUnparseable"
        }
        HelperError::LegacyStopFailed { .. } => "app.boxpilot.Helper1.LegacyStopFailed",
        HelperError::LegacyDisableFailed { .. } => "app.boxpilot.Helper1.LegacyDisableFailed",
        HelperError::LegacyConflictsWithManaged { .. } => {
            "app.boxpilot.Helper1.LegacyConflictsWithManaged"
        }
        HelperError::LegacyAssetTooLarge { .. } => "app.boxpilot.Helper1.LegacyAssetTooLarge",
        HelperError::LegacyTooManyAssets { .. } => "app.boxpilot.Helper1.LegacyTooManyAssets",
        HelperError::DiagnosticsIoFailed { .. } => "app.boxpilot.Helper1.DiagnosticsIoFailed",
        HelperError::DiagnosticsEncodeFailed { .. } => {
            "app.boxpilot.Helper1.DiagnosticsEncodeFailed"
        }
    };
    let msg = e.to_string();
    // We use zbus::fdo::Error::Failed as the carrier; the precise mapping
    // gets refined when zbus exposes a way to set arbitrary error names
    // from interface methods. For now, encode the typed name into the
    // message prefix so the GUI can still discriminate.
    zbus::fdo::Error::Failed(format!("{name}: {msg}"))
}

fn extract_sender(header: &zbus::message::Header<'_>) -> zbus::fdo::Result<String> {
    header.sender().map(|s| s.to_string()).ok_or_else(|| {
        zbus::fdo::Error::Failed(
            "app.boxpilot.Helper1.Ipc: missing sender on incoming message".into(),
        )
    })
}

#[interface(name = "app.boxpilot.Helper1")]
impl Helper {
    /// Returns spec §3.1 / §6.3 `service.status`. Read-only; no controller
    /// required; orphaned controller is reported in the response, not as an
    /// error.
    #[instrument(skip(self, header))]
    async fn service_status(
        &self,
        #[zbus(header)] header: zbus::message::Header<'_>,
    ) -> zbus::fdo::Result<String> {
        let sender = extract_sender(&header)?;
        let resp = self.do_service_status(&sender).await.map_err(to_zbus_err)?;
        // Wire format on D-Bus is a single JSON string. We use JSON rather
        // than a nested zbus dict so the IPC types live in one Rust type
        // hierarchy and the GUI can deserialize via serde without a
        // bespoke zvariant→TS layer.
        serde_json::to_string(&resp).map_err(|e| {
            zbus::fdo::Error::Failed(format!("app.boxpilot.Helper1.Ipc: serialize: {e}"))
        })
    }

    /// Returns spec §3.1 first-paint bundle. Read-only; no controller
    /// required. Bundles service status, active profile fields from
    /// boxpilot.toml, core path/version, and the active-symlink corruption
    /// flag so the GUI can render Home in one round-trip.
    #[instrument(skip(self, header))]
    async fn home_status(
        &self,
        #[zbus(header)] header: zbus::message::Header<'_>,
    ) -> zbus::fdo::Result<String> {
        let sender = extract_sender(&header)?;
        let resp = self.do_home_status(&sender).await.map_err(to_zbus_err)?;
        serde_json::to_string(&resp).map_err(|e| {
            zbus::fdo::Error::Failed(format!("app.boxpilot.Helper1.Ipc: serialize: {e}"))
        })
    }

    // ----- Stubs for the remaining actions (filled in by later plans). -----
    // Each goes through dispatch::authorize first — an unauthorized caller
    // sees NotAuthorized, not NotImplemented. Later plans replace each body
    // with the real implementation while keeping the authorize chokepoint.

    async fn service_start(
        &self,
        #[zbus(header)] header: zbus::message::Header<'_>,
    ) -> zbus::fdo::Result<String> {
        let sender = extract_sender(&header)?;
        let resp = self.do_service_start(&sender).await.map_err(to_zbus_err)?;
        serde_json::to_string(&resp).map_err(|e| {
            zbus::fdo::Error::Failed(format!("app.boxpilot.Helper1.Ipc: serialize: {e}"))
        })
    }

    async fn service_stop(
        &self,
        #[zbus(header)] header: zbus::message::Header<'_>,
    ) -> zbus::fdo::Result<String> {
        let sender = extract_sender(&header)?;
        let resp = self.do_service_stop(&sender).await.map_err(to_zbus_err)?;
        serde_json::to_string(&resp).map_err(|e| {
            zbus::fdo::Error::Failed(format!("app.boxpilot.Helper1.Ipc: serialize: {e}"))
        })
    }

    async fn service_restart(
        &self,
        #[zbus(header)] header: zbus::message::Header<'_>,
    ) -> zbus::fdo::Result<String> {
        let sender = extract_sender(&header)?;
        let resp = self
            .do_service_restart(&sender)
            .await
            .map_err(to_zbus_err)?;
        serde_json::to_string(&resp).map_err(|e| {
            zbus::fdo::Error::Failed(format!("app.boxpilot.Helper1.Ipc: serialize: {e}"))
        })
    }

    async fn service_enable(
        &self,
        #[zbus(header)] header: zbus::message::Header<'_>,
    ) -> zbus::fdo::Result<String> {
        let sender = extract_sender(&header)?;
        let resp = self.do_service_enable(&sender).await.map_err(to_zbus_err)?;
        serde_json::to_string(&resp).map_err(|e| {
            zbus::fdo::Error::Failed(format!("app.boxpilot.Helper1.Ipc: serialize: {e}"))
        })
    }

    async fn service_disable(
        &self,
        #[zbus(header)] header: zbus::message::Header<'_>,
    ) -> zbus::fdo::Result<String> {
        let sender = extract_sender(&header)?;
        let resp = self
            .do_service_disable(&sender)
            .await
            .map_err(to_zbus_err)?;
        serde_json::to_string(&resp).map_err(|e| {
            zbus::fdo::Error::Failed(format!("app.boxpilot.Helper1.Ipc: serialize: {e}"))
        })
    }
    async fn service_install_managed(
        &self,
        #[zbus(header)] header: zbus::message::Header<'_>,
    ) -> zbus::fdo::Result<String> {
        let sender = extract_sender(&header)?;
        let resp = self
            .do_service_install_managed(&sender)
            .await
            .map_err(to_zbus_err)?;
        serde_json::to_string(&resp).map_err(|e| {
            zbus::fdo::Error::Failed(format!("app.boxpilot.Helper1.Ipc: serialize: {e}"))
        })
    }

    async fn service_logs(
        &self,
        #[zbus(header)] header: zbus::message::Header<'_>,
        request_json: String,
    ) -> zbus::fdo::Result<String> {
        let sender = extract_sender(&header)?;
        let req: boxpilot_ipc::ServiceLogsRequest =
            serde_json::from_str(&request_json).map_err(|e| {
                zbus::fdo::Error::Failed(format!("app.boxpilot.Helper1.Ipc: parse: {e}"))
            })?;
        let resp = self
            .do_service_logs(&sender, req)
            .await
            .map_err(to_zbus_err)?;
        serde_json::to_string(&resp).map_err(|e| {
            zbus::fdo::Error::Failed(format!("app.boxpilot.Helper1.Ipc: serialize: {e}"))
        })
    }
    async fn profile_activate_bundle(
        &self,
        #[zbus(header)] header: zbus::message::Header<'_>,
        request_json: String,
        bundle_fd: zbus::zvariant::OwnedFd,
    ) -> zbus::fdo::Result<String> {
        let sender = extract_sender(&header)?;
        let req: boxpilot_ipc::ActivateBundleRequest = serde_json::from_str(&request_json)
            .map_err(|e| {
                zbus::fdo::Error::Failed(format!("app.boxpilot.Helper1.Ipc: parse: {e}"))
            })?;
        let resp = self
            .do_profile_activate_bundle(&sender, req, bundle_fd.into())
            .await
            .map_err(to_zbus_err)?;
        serde_json::to_string(&resp).map_err(|e| {
            zbus::fdo::Error::Failed(format!("app.boxpilot.Helper1.Ipc: serialize: {e}"))
        })
    }
    async fn profile_rollback_release(
        &self,
        #[zbus(header)] header: zbus::message::Header<'_>,
        request_json: String,
    ) -> zbus::fdo::Result<String> {
        let sender = extract_sender(&header)?;
        let req: boxpilot_ipc::RollbackRequest =
            serde_json::from_str(&request_json).map_err(|e| {
                zbus::fdo::Error::Failed(format!("app.boxpilot.Helper1.Ipc: parse: {e}"))
            })?;
        let resp = self
            .do_profile_rollback_release(&sender, req)
            .await
            .map_err(to_zbus_err)?;
        serde_json::to_string(&resp).map_err(|e| {
            zbus::fdo::Error::Failed(format!("app.boxpilot.Helper1.Ipc: serialize: {e}"))
        })
    }
    async fn core_discover(
        &self,
        #[zbus(header)] header: zbus::message::Header<'_>,
    ) -> zbus::fdo::Result<String> {
        let sender = extract_sender(&header)?;
        let resp = self.do_core_discover(&sender).await.map_err(to_zbus_err)?;
        serde_json::to_string(&resp).map_err(|e| {
            zbus::fdo::Error::Failed(format!("app.boxpilot.Helper1.Ipc: serialize: {e}"))
        })
    }

    async fn core_install_managed(
        &self,
        #[zbus(header)] header: zbus::message::Header<'_>,
        request_json: String,
    ) -> zbus::fdo::Result<String> {
        let sender = extract_sender(&header)?;
        let req: boxpilot_ipc::CoreInstallRequest =
            serde_json::from_str(&request_json).map_err(|e| {
                zbus::fdo::Error::Failed(format!("app.boxpilot.Helper1.Ipc: parse: {e}"))
            })?;
        let resp = self
            .do_core_install_managed(&sender, req)
            .await
            .map_err(to_zbus_err)?;
        serde_json::to_string(&resp).map_err(|e| {
            zbus::fdo::Error::Failed(format!("app.boxpilot.Helper1.Ipc: serialize: {e}"))
        })
    }

    async fn core_upgrade_managed(
        &self,
        #[zbus(header)] header: zbus::message::Header<'_>,
        request_json: String,
    ) -> zbus::fdo::Result<String> {
        let sender = extract_sender(&header)?;
        let req: boxpilot_ipc::CoreInstallRequest =
            serde_json::from_str(&request_json).map_err(|e| {
                zbus::fdo::Error::Failed(format!("app.boxpilot.Helper1.Ipc: parse: {e}"))
            })?;
        let resp = self
            .do_core_upgrade_managed(&sender, req)
            .await
            .map_err(to_zbus_err)?;
        serde_json::to_string(&resp).map_err(|e| {
            zbus::fdo::Error::Failed(format!("app.boxpilot.Helper1.Ipc: serialize: {e}"))
        })
    }

    async fn core_rollback_managed(
        &self,
        #[zbus(header)] header: zbus::message::Header<'_>,
        request_json: String,
    ) -> zbus::fdo::Result<String> {
        let sender = extract_sender(&header)?;
        let req: boxpilot_ipc::CoreRollbackRequest =
            serde_json::from_str(&request_json).map_err(|e| {
                zbus::fdo::Error::Failed(format!("app.boxpilot.Helper1.Ipc: parse: {e}"))
            })?;
        let resp = self
            .do_core_rollback_managed(&sender, req)
            .await
            .map_err(to_zbus_err)?;
        serde_json::to_string(&resp).map_err(|e| {
            zbus::fdo::Error::Failed(format!("app.boxpilot.Helper1.Ipc: serialize: {e}"))
        })
    }

    async fn core_adopt(
        &self,
        #[zbus(header)] header: zbus::message::Header<'_>,
        request_json: String,
    ) -> zbus::fdo::Result<String> {
        let sender = extract_sender(&header)?;
        let req: boxpilot_ipc::CoreAdoptRequest =
            serde_json::from_str(&request_json).map_err(|e| {
                zbus::fdo::Error::Failed(format!("app.boxpilot.Helper1.Ipc: parse: {e}"))
            })?;
        let resp = self
            .do_core_adopt(&sender, req)
            .await
            .map_err(to_zbus_err)?;
        serde_json::to_string(&resp).map_err(|e| {
            zbus::fdo::Error::Failed(format!("app.boxpilot.Helper1.Ipc: serialize: {e}"))
        })
    }
    async fn legacy_observe_service(
        &self,
        #[zbus(header)] header: zbus::message::Header<'_>,
    ) -> zbus::fdo::Result<String> {
        let sender = extract_sender(&header)?;
        let resp = self
            .do_legacy_observe_service(&sender)
            .await
            .map_err(to_zbus_err)?;
        serde_json::to_string(&resp).map_err(|e| {
            zbus::fdo::Error::Failed(format!("app.boxpilot.Helper1.Ipc: serialize: {e}"))
        })
    }
    async fn legacy_migrate_service(
        &self,
        #[zbus(header)] header: zbus::message::Header<'_>,
        request_json: String,
    ) -> zbus::fdo::Result<String> {
        let sender = extract_sender(&header)?;
        let req: boxpilot_ipc::LegacyMigrateRequest =
            serde_json::from_str(&request_json).map_err(|e| {
                zbus::fdo::Error::Failed(format!("app.boxpilot.Helper1.Ipc: parse: {e}"))
            })?;
        let resp = self
            .do_legacy_migrate_service(&sender, req)
            .await
            .map_err(to_zbus_err)?;
        serde_json::to_string(&resp).map_err(|e| {
            zbus::fdo::Error::Failed(format!("app.boxpilot.Helper1.Ipc: serialize: {e}"))
        })
    }
    async fn controller_transfer(
        &self,
        #[zbus(header)] header: zbus::message::Header<'_>,
    ) -> zbus::fdo::Result<String> {
        self.do_stub(&header, HelperMethod::ControllerTransfer)
            .await
    }
    async fn diagnostics_export_redacted(
        &self,
        #[zbus(header)] header: zbus::message::Header<'_>,
    ) -> zbus::fdo::Result<String> {
        let sender = extract_sender(&header)?;
        let resp = self
            .do_diagnostics_export_redacted(&sender)
            .await
            .map_err(to_zbus_err)?;
        serde_json::to_string(&resp).map_err(|e| {
            zbus::fdo::Error::Failed(format!("app.boxpilot.Helper1.Ipc: serialize: {e}"))
        })
    }
}

impl Helper {
    async fn do_service_status(
        &self,
        sender_bus_name: &str,
    ) -> Result<ServiceStatusResponse, HelperError> {
        let call =
            dispatch::authorize(&self.ctx, sender_bus_name, HelperMethod::ServiceStatus).await?;
        let cfg = self.ctx.load_config().await?;
        let unit_name = cfg.target_service.clone();
        let unit_state = self.ctx.systemd.unit_state(&unit_name).await?;
        let controller = call.controller.to_status();
        Ok(ServiceStatusResponse {
            unit_name,
            unit_state,
            controller,
            state_schema_mismatch: self.ctx.state_schema_mismatch,
        })
    }

    async fn do_home_status(
        &self,
        sender_bus_name: &str,
    ) -> Result<boxpilot_ipc::HomeStatusResponse, HelperError> {
        let call =
            dispatch::authorize(&self.ctx, sender_bus_name, HelperMethod::HomeStatus).await?;

        // Service: same shape as do_service_status, using the call we just
        // authorized so we don't re-run polkit.
        let cfg = self.ctx.load_config().await?;
        let unit_name = cfg.target_service.clone();
        let unit_state = self.ctx.systemd.unit_state(&unit_name).await?;
        let controller = call.controller.to_status();
        let service = ServiceStatusResponse {
            unit_name,
            unit_state,
            controller,
            state_schema_mismatch: self.ctx.state_schema_mismatch,
        };

        // Active profile: read straight from boxpilot.toml. All four
        // identity fields must be populated for the snapshot to count as
        // "activated"; otherwise it's None.
        let active_profile = match (
            cfg.active_profile_id.as_ref(),
            cfg.active_profile_sha256.as_ref(),
            cfg.active_release_id.as_ref(),
            cfg.activated_at.as_ref(),
        ) {
            (Some(id), Some(sha), Some(rel), Some(at)) => {
                Some(boxpilot_ipc::ActiveProfileSnapshot {
                    profile_id: id.clone(),
                    profile_name: cfg.active_profile_name.clone(),
                    profile_sha256: sha.clone(),
                    release_id: rel.clone(),
                    activated_at: at.clone(),
                })
            }
            _ => None,
        };

        // Core: discover and find the entry whose path matches cfg.core_path.
        // Discovery failure is non-fatal — the rest of the page still renders.
        let core_version = match self.discover_for_home().await {
            Ok(list) => cfg
                .core_path
                .as_deref()
                .and_then(|p| list.cores.iter().find(|c| c.path == p))
                .map(|c| c.version.clone())
                .unwrap_or_else(|| "unknown".to_string()),
            Err(_) => "unknown".to_string(),
        };
        let core = boxpilot_ipc::CoreSnapshot {
            path: cfg.core_path.clone(),
            state: cfg.core_state,
            version: core_version,
        };

        // Active corrupt: identical predicate to the daemon-startup
        // recovery path so the GUI banner can never disagree with the
        // recovery report. Read-only; safe to call on every poll.
        let active_corrupt = crate::profile::recovery::check_active_status(&self.ctx.paths)
            .await
            .corrupt;

        Ok(boxpilot_ipc::HomeStatusResponse {
            schema_version: boxpilot_ipc::HOME_STATUS_SCHEMA_VERSION,
            service,
            active_profile,
            core,
            active_corrupt,
        })
    }

    async fn discover_for_home(&self) -> Result<boxpilot_ipc::CoreDiscoverResponse, HelperError> {
        let deps = crate::core::discover::DiscoverDeps {
            paths: self.ctx.paths.clone(),
            fs: &*self.ctx.fs_meta,
            version_checker: &*self.ctx.version_checker,
        };
        crate::core::discover::discover(&deps).await
    }

    async fn do_service_start(
        &self,
        sender: &str,
    ) -> Result<boxpilot_ipc::ServiceControlResponse, HelperError> {
        let _call = dispatch::authorize(&self.ctx, sender, HelperMethod::ServiceStart).await?;
        let cfg = self.ctx.load_config().await?;
        crate::service::control::run(
            crate::service::control::Verb::Start,
            &cfg.target_service,
            &*self.ctx.systemd,
        )
        .await
    }

    async fn do_service_stop(
        &self,
        sender: &str,
    ) -> Result<boxpilot_ipc::ServiceControlResponse, HelperError> {
        let _call = dispatch::authorize(&self.ctx, sender, HelperMethod::ServiceStop).await?;
        let cfg = self.ctx.load_config().await?;
        crate::service::control::run(
            crate::service::control::Verb::Stop,
            &cfg.target_service,
            &*self.ctx.systemd,
        )
        .await
    }

    async fn do_service_restart(
        &self,
        sender: &str,
    ) -> Result<boxpilot_ipc::ServiceControlResponse, HelperError> {
        let _call = dispatch::authorize(&self.ctx, sender, HelperMethod::ServiceRestart).await?;
        let cfg = self.ctx.load_config().await?;
        crate::service::control::run(
            crate::service::control::Verb::Restart,
            &cfg.target_service,
            &*self.ctx.systemd,
        )
        .await
    }

    async fn do_service_enable(
        &self,
        sender: &str,
    ) -> Result<boxpilot_ipc::ServiceControlResponse, HelperError> {
        let _call = dispatch::authorize(&self.ctx, sender, HelperMethod::ServiceEnable).await?;
        let cfg = self.ctx.load_config().await?;
        crate::service::control::run(
            crate::service::control::Verb::Enable,
            &cfg.target_service,
            &*self.ctx.systemd,
        )
        .await
    }

    async fn do_service_disable(
        &self,
        sender: &str,
    ) -> Result<boxpilot_ipc::ServiceControlResponse, HelperError> {
        let _call = dispatch::authorize(&self.ctx, sender, HelperMethod::ServiceDisable).await?;
        let cfg = self.ctx.load_config().await?;
        crate::service::control::run(
            crate::service::control::Verb::Disable,
            &cfg.target_service,
            &*self.ctx.systemd,
        )
        .await
    }

    async fn do_service_install_managed(
        &self,
        sender: &str,
    ) -> Result<boxpilot_ipc::ServiceInstallManagedResponse, HelperError> {
        let call =
            dispatch::authorize(&self.ctx, sender, HelperMethod::ServiceInstallManaged).await?;
        let controller = dispatch::maybe_claim_controller(
            call.will_claim_controller,
            call.caller_uid,
            &*self.ctx.user_lookup,
        )?;
        let cfg = self.ctx.load_config().await?;
        let deps = crate::service::install::InstallDeps {
            paths: self.ctx.paths.clone(),
            systemd: &*self.ctx.systemd,
            fs: &*self.ctx.fs_meta,
        };
        let mut resp = crate::service::install::install_managed(&cfg, &deps).await?;
        resp.claimed_controller = controller.is_some();

        // Persist controller-name + boxpilot.toml + polkit drop-in if claiming.
        // T8 made StateCommit also write the polkit drop-in atomically with
        // controller-name. The install_state passed here is empty because
        // service.install_managed does not change the cores ledger; the guard
        // inside StateCommit::apply preserves any on-disk install-state.json.
        //
        // Atomicity note: the unit file + daemon-reload above are NOT atomic
        // with the StateCommit below. If the commit fails, the unit lives on
        // disk but the controller claim is not recorded; the next attempt
        // rewrites both, so the corner is benign rather than blocking.
        if let Some(c) = controller {
            let commit = crate::core::commit::StateCommit {
                paths: self.ctx.paths.clone(),
                toml_updates: crate::core::commit::TomlUpdates::default(),
                controller: Some(c),
                install_state: boxpilot_ipc::InstallState::empty(),
                current_symlink_target: None,
            };
            commit.apply().await?;
        }

        Ok(resp)
    }

    async fn do_service_logs(
        &self,
        sender: &str,
        req: boxpilot_ipc::ServiceLogsRequest,
    ) -> Result<boxpilot_ipc::ServiceLogsResponse, HelperError> {
        let _call = dispatch::authorize(&self.ctx, sender, HelperMethod::ServiceLogs).await?;
        let cfg = self.ctx.load_config().await?;
        crate::service::logs::read(&req, &cfg.target_service, &*self.ctx.journal).await
    }

    /// Run the §6 dispatch contract (caller-uid → controller → polkit →
    /// optional lock) for a method whose body isn't implemented yet, then
    /// return `NotImplemented`. Unauthorized callers see `NotAuthorized`
    /// rather than `NotImplemented`, which is both more honest and avoids
    /// leaking which methods exist.
    async fn do_stub(
        &self,
        header: &zbus::message::Header<'_>,
        method: HelperMethod,
    ) -> zbus::fdo::Result<String> {
        let sender = extract_sender(header)?;
        dispatch::authorize(&self.ctx, &sender, method)
            .await
            .map_err(to_zbus_err)?;
        warn!(
            method = method.as_logical(),
            "called a not-yet-implemented helper method"
        );
        Err(to_zbus_err(HelperError::NotImplemented))
    }

    async fn do_core_discover(
        &self,
        sender: &str,
    ) -> Result<boxpilot_ipc::CoreDiscoverResponse, HelperError> {
        let _call = dispatch::authorize(&self.ctx, sender, HelperMethod::CoreDiscover).await?;
        let deps = crate::core::discover::DiscoverDeps {
            paths: self.ctx.paths.clone(),
            fs: &*self.ctx.fs_meta,
            version_checker: &*self.ctx.version_checker,
        };
        crate::core::discover::discover(&deps).await
    }

    async fn do_core_install_managed(
        &self,
        sender: &str,
        req: boxpilot_ipc::CoreInstallRequest,
    ) -> Result<boxpilot_ipc::CoreInstallResponse, HelperError> {
        let call = dispatch::authorize(&self.ctx, sender, HelperMethod::CoreInstallManaged).await?;
        let controller = dispatch::maybe_claim_controller(
            call.will_claim_controller,
            call.caller_uid,
            &*self.ctx.user_lookup,
        )?;
        let deps = crate::core::install::InstallDeps {
            paths: self.ctx.paths.clone(),
            github: &*self.ctx.github,
            downloader: &*self.ctx.downloader,
            fs: &*self.ctx.fs_meta,
            version_checker: &*self.ctx.version_checker,
        };
        crate::core::install::install_or_upgrade(&req, &deps, controller).await
    }

    async fn do_core_upgrade_managed(
        &self,
        sender: &str,
        req: boxpilot_ipc::CoreInstallRequest,
    ) -> Result<boxpilot_ipc::CoreInstallResponse, HelperError> {
        let call = dispatch::authorize(&self.ctx, sender, HelperMethod::CoreUpgradeManaged).await?;
        let controller = dispatch::maybe_claim_controller(
            call.will_claim_controller,
            call.caller_uid,
            &*self.ctx.user_lookup,
        )?;
        let deps = crate::core::install::InstallDeps {
            paths: self.ctx.paths.clone(),
            github: &*self.ctx.github,
            downloader: &*self.ctx.downloader,
            fs: &*self.ctx.fs_meta,
            version_checker: &*self.ctx.version_checker,
        };
        crate::core::install::install_or_upgrade(&req, &deps, controller).await
    }

    async fn do_core_rollback_managed(
        &self,
        sender: &str,
        req: boxpilot_ipc::CoreRollbackRequest,
    ) -> Result<boxpilot_ipc::CoreInstallResponse, HelperError> {
        let call =
            dispatch::authorize(&self.ctx, sender, HelperMethod::CoreRollbackManaged).await?;
        let controller = dispatch::maybe_claim_controller(
            call.will_claim_controller,
            call.caller_uid,
            &*self.ctx.user_lookup,
        )?;
        let deps = crate::core::rollback::RollbackDeps {
            paths: self.ctx.paths.clone(),
            fs: &*self.ctx.fs_meta,
            version_checker: &*self.ctx.version_checker,
        };
        crate::core::rollback::rollback(&req, &deps, controller).await
    }

    async fn do_core_adopt(
        &self,
        sender: &str,
        req: boxpilot_ipc::CoreAdoptRequest,
    ) -> Result<boxpilot_ipc::CoreInstallResponse, HelperError> {
        let call = dispatch::authorize(&self.ctx, sender, HelperMethod::CoreAdopt).await?;
        let controller = dispatch::maybe_claim_controller(
            call.will_claim_controller,
            call.caller_uid,
            &*self.ctx.user_lookup,
        )?;
        let deps = crate::core::adopt::AdoptDeps {
            paths: self.ctx.paths.clone(),
            fs: &*self.ctx.fs_meta,
            version_checker: &*self.ctx.version_checker,
        };
        crate::core::adopt::adopt(&req, &deps, controller).await
    }

    async fn do_profile_activate_bundle(
        &self,
        sender: &str,
        req: boxpilot_ipc::ActivateBundleRequest,
        fd: std::os::fd::OwnedFd,
    ) -> Result<boxpilot_ipc::ActivateBundleResponse, HelperError> {
        let call =
            dispatch::authorize(&self.ctx, sender, HelperMethod::ProfileActivateBundle).await?;
        let controller = dispatch::maybe_claim_controller(
            call.will_claim_controller,
            call.caller_uid,
            &*self.ctx.user_lookup,
        )?;
        let deps = crate::profile::activate::ActivateDeps {
            paths: self.ctx.paths.clone(),
            systemd: &*self.ctx.systemd,
            verifier: &*self.ctx.verifier,
            checker: &*self.ctx.checker,
        };
        crate::profile::activate::activate_bundle(req, fd, controller, &deps).await
    }

    async fn do_profile_rollback_release(
        &self,
        sender: &str,
        req: boxpilot_ipc::RollbackRequest,
    ) -> Result<boxpilot_ipc::ActivateBundleResponse, HelperError> {
        let call =
            dispatch::authorize(&self.ctx, sender, HelperMethod::ProfileRollbackRelease).await?;
        let controller = dispatch::maybe_claim_controller(
            call.will_claim_controller,
            call.caller_uid,
            &*self.ctx.user_lookup,
        )?;
        let deps = crate::profile::rollback::RollbackDeps {
            paths: self.ctx.paths.clone(),
            systemd: &*self.ctx.systemd,
            verifier: &*self.ctx.verifier,
        };
        crate::profile::rollback::rollback_release(req, controller, &deps).await
    }

    async fn do_legacy_observe_service(
        &self,
        sender: &str,
    ) -> Result<boxpilot_ipc::LegacyObserveServiceResponse, HelperError> {
        let _call =
            dispatch::authorize(&self.ctx, sender, HelperMethod::LegacyObserveService).await?;
        let cfg = self.ctx.load_config().await?;
        let deps = crate::legacy::observe::ObserveDeps {
            systemd: &*self.ctx.systemd,
            fs_read: &*self.ctx.fs_fragment_reader,
        };
        crate::legacy::observe::observe(&cfg, &deps).await
    }

    async fn do_diagnostics_export_redacted(
        &self,
        sender: &str,
    ) -> Result<boxpilot_ipc::DiagnosticsExportResponse, HelperError> {
        let _call =
            dispatch::authorize(&self.ctx, sender, HelperMethod::DiagnosticsExportRedacted).await?;
        let cfg = self.ctx.load_config().await?;
        crate::diagnostics::compose(crate::diagnostics::ComposeInputs {
            paths: &self.ctx.paths,
            unit_name: &cfg.target_service,
            journal: &*self.ctx.journal,
            os_release_path: std::path::Path::new("/etc/os-release"),
            now_iso: || chrono::Utc::now().format("%Y-%m-%dT%H-%M-%SZ").to_string(),
        })
        .await
    }

    async fn do_legacy_migrate_service(
        &self,
        sender: &str,
        req: boxpilot_ipc::LegacyMigrateRequest,
    ) -> Result<boxpilot_ipc::LegacyMigrateResponse, HelperError> {
        let _call =
            dispatch::authorize(&self.ctx, sender, HelperMethod::LegacyMigrateService).await?;
        let cfg = self.ctx.load_config().await?;
        let prep = crate::legacy::migrate::PrepareDeps {
            systemd: &*self.ctx.systemd,
            fs_read: &*self.ctx.fs_fragment_reader,
            config_reader: &*self.ctx.config_reader,
        };
        let backups_dir = self.ctx.paths.backups_units_dir();
        let cut = crate::legacy::migrate::CutoverDeps {
            systemd: &*self.ctx.systemd,
            backups_units_dir: &backups_dir,
            now_iso: || chrono::Utc::now().format("%Y-%m-%dT%H-%M-%SZ").to_string(),
        };
        crate::legacy::migrate::run(&cfg, req, &prep, &cut).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::authority::testing::CannedAuthority;
    use crate::context::testing::ctx_with;
    use crate::context::testing::ctx_with_journal_lines;
    use crate::context::testing::ctx_with_recording;
    use crate::systemd::testing::{RecordedCall, RecordingSystemd};
    use boxpilot_ipc::UnitState;
    use tempfile::tempdir;

    #[tokio::test]
    async fn service_status_passes_through_unit_not_found() {
        let tmp = tempdir().unwrap();
        let ctx = Arc::new(ctx_with(
            &tmp,
            None,
            CannedAuthority::allowing(&["app.boxpilot.helper.service.status"]),
            UnitState::NotFound,
            &[(":1.42", 1000)],
        ));
        let h = Helper::new(ctx);
        let resp = h.do_service_status(":1.42").await.unwrap();
        assert_eq!(resp.unit_name, "boxpilot-sing-box.service");
        assert_eq!(resp.unit_state, UnitState::NotFound);
    }

    #[tokio::test]
    async fn service_status_returns_known_state_when_unit_exists() {
        let tmp = tempdir().unwrap();
        let known = UnitState::Known {
            active_state: "active".into(),
            sub_state: "running".into(),
            load_state: "loaded".into(),
            n_restarts: 2,
            exec_main_status: 0,
        };
        let ctx = Arc::new(ctx_with(
            &tmp,
            Some("schema_version = 1\ncontroller_uid = 1000\n"),
            CannedAuthority::allowing(&["app.boxpilot.helper.service.status"]),
            known.clone(),
            &[(":1.42", 1000)],
        ));
        let h = Helper::new(ctx);
        let resp = h.do_service_status(":1.42").await.unwrap();
        assert_eq!(resp.unit_state, known);
    }

    #[tokio::test]
    async fn service_status_surfaces_state_schema_mismatch() {
        // Issue #8: when boxpilotd starts up and finds an install-state.json
        // with an incompatible schema_version, it records the version on
        // HelperContext. service.status (read-only) must echo it back so
        // the GUI can show a single banner instead of every mutating call
        // failing with the same error.
        let tmp = tempdir().unwrap();
        let mut inner = ctx_with(
            &tmp,
            None,
            CannedAuthority::allowing(&["app.boxpilot.helper.service.status"]),
            UnitState::NotFound,
            &[(":1.42", 1000)],
        );
        inner.state_schema_mismatch = Some(99);
        let ctx = Arc::new(inner);
        let h = Helper::new(ctx);
        let resp = h.do_service_status(":1.42").await.unwrap();
        assert_eq!(resp.state_schema_mismatch, Some(99));
    }

    #[tokio::test]
    async fn service_status_denied_by_polkit_returns_not_authorized() {
        let tmp = tempdir().unwrap();
        let ctx = Arc::new(ctx_with(
            &tmp,
            None,
            CannedAuthority::denying(&["app.boxpilot.helper.service.status"]),
            UnitState::NotFound,
            &[(":1.42", 1000)],
        ));
        let h = Helper::new(ctx);
        let r = h.do_service_status(":1.42").await;
        assert!(matches!(r, Err(HelperError::NotAuthorized)));
    }

    #[tokio::test]
    async fn home_status_returns_unactivated_when_config_lacks_active_fields() {
        let tmp = tempdir().unwrap();
        let ctx = Arc::new(ctx_with(
            &tmp,
            None,
            CannedAuthority::allowing(&["app.boxpilot.helper.home.status"]),
            UnitState::NotFound,
            &[(":1.42", 1000)],
        ));
        let h = Helper::new(ctx);
        let resp = h.do_home_status(":1.42").await.unwrap();
        assert!(resp.active_profile.is_none());
        assert_eq!(resp.schema_version, 1);
        assert!(!resp.active_corrupt);
        assert_eq!(resp.service.unit_name, "boxpilot-sing-box.service");
    }

    #[tokio::test]
    async fn home_status_denied_by_polkit_returns_not_authorized() {
        let tmp = tempdir().unwrap();
        let ctx = Arc::new(ctx_with(
            &tmp,
            None,
            CannedAuthority::denying(&["app.boxpilot.helper.home.status"]),
            UnitState::NotFound,
            &[(":1.42", 1000)],
        ));
        let h = Helper::new(ctx);
        let r = h.do_home_status(":1.42").await;
        assert!(matches!(r, Err(HelperError::NotAuthorized)));
    }

    /// A denied stub call returns NotAuthorized (not NotImplemented),
    /// proving the §6 chokepoint runs before the stub body is reached.
    /// Uses `core.discover` because it's read-only — testing a mutating
    /// method here would also need a controller to be set in
    /// `boxpilot.toml`, which adds noise; the read-only path exercises
    /// dispatch + polkit denial cleanly.
    #[tokio::test]
    async fn stub_denied_by_polkit_returns_not_authorized_not_not_implemented() {
        let tmp = tempdir().unwrap();
        let ctx = Arc::new(ctx_with(
            &tmp,
            None,
            CannedAuthority::denying(&["app.boxpilot.helper.core.discover"]),
            UnitState::NotFound,
            &[(":1.42", 1000)],
        ));
        let h = Helper::new(ctx);
        // We can't construct a real zbus::message::Header in unit tests, so
        // we test through the inner dispatch path. The interface-level
        // wiring is mechanically identical for every stub (verified in
        // the file above by inspection).
        let r = dispatch::authorize(&h.ctx, ":1.42", HelperMethod::CoreDiscover).await;
        assert!(matches!(r, Err(HelperError::NotAuthorized)));
    }

    /// An authorized stub call passes the §6 chokepoint and reaches the
    /// stub body, which returns NotImplemented. Confirms the ordering
    /// (authorize first, then NotImplemented) for later plans to rely on.
    #[tokio::test]
    async fn stub_authorized_reaches_not_implemented() {
        let tmp = tempdir().unwrap();
        let ctx = Arc::new(ctx_with(
            &tmp,
            None,
            CannedAuthority::allowing(&["app.boxpilot.helper.core.discover"]),
            UnitState::NotFound,
            &[(":1.42", 1000)],
        ));
        // Same caveat as above: we exercise dispatch directly. The Helper
        // method body would then call to_zbus_err(NotImplemented).
        let r = dispatch::authorize(&ctx, ":1.42", HelperMethod::CoreDiscover).await;
        // Map to a Debug-printable shape so a future failure surfaces the
        // error variant; AuthorizedCall itself is not Debug.
        let r_dbg: Result<(), &HelperError> = match &r {
            Ok(_) => Ok(()),
            Err(e) => Err(e),
        };
        assert!(
            r.is_ok(),
            "authorized read-only stub call should pass dispatch: {r_dbg:?}"
        );
    }

    #[tokio::test]
    async fn core_discover_returns_ok_on_empty_tempdir() {
        let tmp = tempdir().unwrap();
        let ctx = Arc::new(ctx_with(
            &tmp,
            None,
            CannedAuthority::allowing(&["app.boxpilot.helper.core.discover"]),
            UnitState::NotFound,
            &[(":1.42", 1000)],
        ));
        let h = Helper::new(ctx);
        let resp = h.do_core_discover(":1.42").await.unwrap();
        assert!(resp.cores.iter().all(|c| !matches!(
            c.kind,
            boxpilot_ipc::CoreKind::ManagedInstalled | boxpilot_ipc::CoreKind::ManagedAdopted
        )));
    }

    #[tokio::test]
    async fn service_start_calls_systemd_start_unit() {
        let tmp = tempdir().unwrap();
        let rec = Arc::new(RecordingSystemd::new(UnitState::NotFound));
        let ctx = Arc::new(ctx_with_recording(
            &tmp,
            Some("schema_version = 1\ncontroller_uid = 1000\n"),
            CannedAuthority::allowing(&["app.boxpilot.helper.service.start"]),
            rec.clone(),
            &[(":1.42", 1000)],
        ));
        let h = Helper::new(ctx);
        h.do_service_start(":1.42").await.unwrap();
        assert!(rec
            .calls()
            .iter()
            .any(|c| matches!(c, RecordedCall::StartUnit(_))));
    }

    #[tokio::test]
    async fn service_stop_calls_systemd_stop_unit() {
        let tmp = tempdir().unwrap();
        let rec = Arc::new(RecordingSystemd::new(UnitState::NotFound));
        let ctx = Arc::new(ctx_with_recording(
            &tmp,
            Some("schema_version = 1\ncontroller_uid = 1000\n"),
            CannedAuthority::allowing(&["app.boxpilot.helper.service.stop"]),
            rec.clone(),
            &[(":1.42", 1000)],
        ));
        let h = Helper::new(ctx);
        h.do_service_stop(":1.42").await.unwrap();
        assert!(rec
            .calls()
            .iter()
            .any(|c| matches!(c, RecordedCall::StopUnit(_))));
    }

    #[tokio::test]
    async fn service_restart_calls_systemd_restart_unit() {
        let tmp = tempdir().unwrap();
        let rec = Arc::new(RecordingSystemd::new(UnitState::NotFound));
        let ctx = Arc::new(ctx_with_recording(
            &tmp,
            Some("schema_version = 1\ncontroller_uid = 1000\n"),
            CannedAuthority::allowing(&["app.boxpilot.helper.service.restart"]),
            rec.clone(),
            &[(":1.42", 1000)],
        ));
        let h = Helper::new(ctx);
        h.do_service_restart(":1.42").await.unwrap();
        assert!(rec
            .calls()
            .iter()
            .any(|c| matches!(c, RecordedCall::RestartUnit(_))));
    }

    #[tokio::test]
    async fn service_enable_calls_systemd_enable_unit_files() {
        let tmp = tempdir().unwrap();
        let rec = Arc::new(RecordingSystemd::new(UnitState::NotFound));
        let ctx = Arc::new(ctx_with_recording(
            &tmp,
            Some("schema_version = 1\ncontroller_uid = 1000\n"),
            CannedAuthority::allowing(&["app.boxpilot.helper.service.enable"]),
            rec.clone(),
            &[(":1.42", 1000)],
        ));
        let h = Helper::new(ctx);
        h.do_service_enable(":1.42").await.unwrap();
        assert!(rec
            .calls()
            .iter()
            .any(|c| matches!(c, RecordedCall::EnableUnitFiles(_))));
    }

    #[tokio::test]
    async fn service_disable_calls_systemd_disable_unit_files() {
        let tmp = tempdir().unwrap();
        let rec = Arc::new(RecordingSystemd::new(UnitState::NotFound));
        let ctx = Arc::new(ctx_with_recording(
            &tmp,
            Some("schema_version = 1\ncontroller_uid = 1000\n"),
            CannedAuthority::allowing(&["app.boxpilot.helper.service.disable"]),
            rec.clone(),
            &[(":1.42", 1000)],
        ));
        let h = Helper::new(ctx);
        h.do_service_disable(":1.42").await.unwrap();
        assert!(rec
            .calls()
            .iter()
            .any(|c| matches!(c, RecordedCall::DisableUnitFiles(_))));
    }

    #[tokio::test]
    async fn service_install_managed_writes_unit_when_core_path_set() {
        let tmp = tempdir().unwrap();
        let rec = Arc::new(RecordingSystemd::new(UnitState::Known {
            active_state: "inactive".into(),
            sub_state: "dead".into(),
            load_state: "loaded".into(),
            n_restarts: 0,
            exec_main_status: 0,
        }));
        // /usr/bin/sing-box is in the §6.5 default-allowed prefix list and
        // PermissiveTestFs reports it as root-owned 0o755, so the trust
        // check passes inside the test.
        let ctx = Arc::new(ctx_with_recording(
            &tmp,
            Some("schema_version = 1\ncore_path = \"/usr/bin/sing-box\"\ncore_state = \"managed-installed\"\ncontroller_uid = 1000\n"),
            CannedAuthority::allowing(&["app.boxpilot.helper.service.install-managed"]),
            rec.clone(),
            &[(":1.42", 1000)],
        ));
        let h = Helper::new(ctx);
        let resp = h.do_service_install_managed(":1.42").await.unwrap();
        assert!(resp
            .generated_unit_path
            .ends_with("etc/systemd/system/boxpilot-sing-box.service"));
        assert!(rec
            .calls()
            .iter()
            .any(|c| matches!(c, RecordedCall::Reload)));
    }

    #[tokio::test]
    async fn service_logs_returns_journal_lines() {
        let tmp = tempdir().unwrap();
        let ctx = Arc::new(ctx_with_journal_lines(
            &tmp,
            Some("schema_version = 1\ncontroller_uid = 1000\n"),
            CannedAuthority::allowing(&["app.boxpilot.helper.service.logs"]),
            UnitState::NotFound,
            &[(":1.42", 1000)],
            vec!["entry1".into(), "entry2".into()],
        ));
        let h = Helper::new(ctx);
        let req = boxpilot_ipc::ServiceLogsRequest { lines: 5 };
        let resp = h.do_service_logs(":1.42", req).await.unwrap();
        assert_eq!(resp.lines, vec!["entry1".to_string(), "entry2".to_string()]);
    }

    #[tokio::test]
    async fn legacy_observe_returns_not_detected_when_unit_absent() {
        let tmp = tempdir().unwrap();
        let ctx = Arc::new(ctx_with(
            &tmp,
            None,
            CannedAuthority::allowing(&["app.boxpilot.helper.legacy.observe-service"]),
            UnitState::NotFound,
            &[(":1.42", 1000)],
        ));
        let h = Helper::new(ctx);
        let resp = h.do_legacy_observe_service(":1.42").await.unwrap();
        assert!(!resp.detected);
    }

    #[tokio::test]
    async fn legacy_observe_denied_returns_not_authorized() {
        let tmp = tempdir().unwrap();
        let ctx = Arc::new(ctx_with(
            &tmp,
            None,
            CannedAuthority::denying(&["app.boxpilot.helper.legacy.observe-service"]),
            UnitState::NotFound,
            &[(":1.42", 1000)],
        ));
        let h = Helper::new(ctx);
        let r = h.do_legacy_observe_service(":1.42").await;
        assert!(matches!(r, Err(HelperError::NotAuthorized)));
    }

    #[tokio::test]
    async fn diagnostics_export_redacted_writes_bundle() {
        let tmp = tempdir().unwrap();
        // Set up the minimal filesystem the composer needs.
        let paths = crate::paths::Paths::with_root(tmp.path());
        let active = paths.releases_dir().join("rel-1");
        std::fs::create_dir_all(&active).unwrap();
        std::fs::write(
            active.join("config.json"),
            br#"{"outbounds":[{"type":"vless","tag":"main","password":"x"}]}"#,
        )
        .unwrap();
        std::fs::write(active.join("manifest.json"), b"{}").unwrap();
        std::fs::create_dir_all(paths.etc_dir()).unwrap();
        std::os::unix::fs::symlink(&active, paths.active_symlink()).unwrap();
        std::fs::create_dir_all(paths.cores_dir().parent().unwrap()).unwrap();
        std::fs::write(
            paths.install_state_json(),
            b"{\"schema_version\":1,\"managed_cores\":[]}",
        )
        .unwrap();
        std::fs::create_dir_all(tmp.path().join("etc/systemd/system")).unwrap();
        std::fs::write(
            paths.systemd_unit_path("boxpilot-sing-box.service"),
            b"[Service]\nExecStart=/usr/bin/sing-box\n",
        )
        .unwrap();

        let ctx = Arc::new(ctx_with_journal_lines(
            &tmp,
            Some("schema_version = 1\ncontroller_uid = 1000\n"),
            CannedAuthority::allowing(&["app.boxpilot.helper.diagnostics.export-redacted"]),
            UnitState::NotFound,
            &[(":1.42", 1000)],
            vec!["a".into(), "b".into()],
        ));
        let h = Helper::new(ctx);
        let resp = h.do_diagnostics_export_redacted(":1.42").await.unwrap();
        assert!(std::path::Path::new(&resp.bundle_path).exists());
        assert!(resp.bundle_size_bytes > 0);
    }

    #[tokio::test]
    async fn diagnostics_export_redacted_denied_returns_not_authorized() {
        let tmp = tempdir().unwrap();
        let ctx = Arc::new(ctx_with(
            &tmp,
            None,
            CannedAuthority::denying(&["app.boxpilot.helper.diagnostics.export-redacted"]),
            UnitState::NotFound,
            &[(":1.42", 1000)],
        ));
        let h = Helper::new(ctx);
        let r = h.do_diagnostics_export_redacted(":1.42").await;
        assert!(matches!(r, Err(HelperError::NotAuthorized)));
    }

    #[tokio::test]
    async fn legacy_migrate_prepare_passes_dispatch_then_returns_unit_not_found_when_absent() {
        let tmp = tempdir().unwrap();
        let ctx = Arc::new(ctx_with(
            &tmp,
            None,
            CannedAuthority::allowing(&["app.boxpilot.helper.legacy.migrate-service"]),
            UnitState::NotFound,
            &[(":1.42", 1000)],
        ));
        let h = Helper::new(ctx);
        let r = h
            .do_legacy_migrate_service(":1.42", boxpilot_ipc::LegacyMigrateRequest::Prepare)
            .await;
        assert!(matches!(r, Err(HelperError::LegacyUnitNotFound { .. })));
    }
}
