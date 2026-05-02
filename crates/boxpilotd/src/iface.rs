#![cfg(target_os = "linux")]

//! `app.boxpilot.Helper1` D-Bus interface — thin shell over
//! [`HelperDispatch`] (PR 11a). Each interface method:
//!   1. extracts the D-Bus sender bus name from the message header
//!   2. resolves it to a `CallerPrincipal::LinuxUid` via the platform
//!      `CallerResolver`
//!   3. sets `ctx.authority_subject` so the `DBusAuthority` impl can
//!      hand the original sender string to polkit (Linux subject form)
//!   4. encodes any typed request body to JSON bytes
//!   5. calls `self.dispatch.handle(conn, method, body, aux)` and
//!      returns the response, decoded back to the typed shape
//!
//! Method names on the bus are CamelCase per D-Bus convention; the
//! logical action mapping is in `boxpilot_ipc::HelperMethod`.

use crate::authority::CallerPrincipal;
use crate::context::HelperContext;
use crate::dispatch_handler::DispatchHandler;
use boxpilot_ipc::{HelperError, HelperMethod, HelperResult, ServiceStatusResponse};
use boxpilot_platform::traits::bundle_aux::AuxStream;
use boxpilot_platform::traits::ipc::{ConnectionInfo, HelperDispatch};
use std::sync::Arc;
use tracing::instrument;
use zbus::interface;

/// Well-known D-Bus name the helper claims at startup. Lives here (with
/// the interface impl) rather than in `main.rs` so it's reachable from
/// `#[cfg(test)] mod tests` for the wire-name guard test below.
///
/// **Frozen by the .deb** — see `dbus_wire_names_are_frozen` test.
pub const BUS_NAME: &str = "app.boxpilot.Helper";

/// Object path the helper exports the interface at. Same freeze constraint
/// as `BUS_NAME` — Tauri's `HelperProxy` hardcodes it as `default_path`.
pub const OBJECT_PATH: &str = "/app/boxpilot/Helper";

pub struct Helper {
    ctx: Arc<HelperContext>,
    dispatch: Arc<dyn HelperDispatch>,
}

impl Helper {
    /// Convenience constructor that builds the default `DispatchHandler`
    /// from `ctx`. Production code in `main.rs` and every helper-side
    /// unit test funnels through this.
    pub fn new(ctx: Arc<HelperContext>) -> Self {
        let dispatch: Arc<dyn HelperDispatch> = Arc::new(DispatchHandler::new(ctx.clone()));
        Self { ctx, dispatch }
    }

    /// Plumb a custom `dispatch` (e.g. an mpsc fake from
    /// `boxpilot_platform::fakes::ipc`). Used by integration smoke tests
    /// that want to intercept verbs without re-wiring the rest of the
    /// `HelperContext`.
    #[allow(dead_code)]
    pub fn with_dispatch(ctx: Arc<HelperContext>, dispatch: Arc<dyn HelperDispatch>) -> Self {
        Self { ctx, dispatch }
    }
}

/// Resolve the kernel uid for a D-Bus sender via the platform `CallerResolver`
/// and wrap it as a `CallerPrincipal::LinuxUid`. The Windows IpcServer
/// (PR 12) builds a `WindowsSid` from the named-pipe client token instead.
async fn resolve_caller_principal(
    ctx: &HelperContext,
    sender: &str,
) -> HelperResult<CallerPrincipal> {
    Ok(CallerPrincipal::LinuxUid(
        ctx.callers.resolve(sender).await?,
    ))
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
        let sender = extract_sender(&header)?;
        // controller.transfer is currently a stub; surfaces NotImplemented
        // (or NotAuthorized for denied callers) via the dispatch chokepoint.
        match self.do_controller_transfer(&sender).await {
            Ok(()) => Ok(String::new()),
            Err(e) => Err(to_zbus_err(e)),
        }
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

// ---------------------------------------------------------------------------
// Thin shells around `dispatch.handle(...)`. Tests in this module call
// these directly; production traffic arrives via the zbus interface above
// and lands in the same shells. Each shell:
//   - resolves the sender to a `CallerPrincipal::LinuxUid`
//   - sets `ctx.authority_subject` so polkit gets the original sender
//   - encodes the request to JSON bytes (or empty for nullary verbs)
//   - calls `dispatch.handle(...)` (no aux for all but activate-bundle)
//   - decodes the response JSON bytes back to the typed shape
//
// Bypassing `dispatch.handle` would re-grow the duplication PR 11a was
// designed to remove, so the shells stay minimal.

impl Helper {
    async fn dispatch_call(
        &self,
        sender: &str,
        method: HelperMethod,
        body: Vec<u8>,
        aux: AuxStream,
    ) -> HelperResult<Vec<u8>> {
        // Set the sender shuttle BEFORE any await: `ZbusSubject::current_sender()`
        // (read inside `DBusAuthority::check`) must observe this call's sender,
        // not a concurrent call's. Awaiting first would let another `dispatch_call`
        // overwrite the shared slot before our authorize step runs.
        self.ctx.authority_subject.set(sender);
        let principal = resolve_caller_principal(&self.ctx, sender).await?;
        let conn = ConnectionInfo { caller: principal };
        self.dispatch.handle(conn, method, body, aux).await
    }

    fn decode<T: serde::de::DeserializeOwned>(bytes: &[u8]) -> HelperResult<T> {
        serde_json::from_slice(bytes).map_err(|e| HelperError::Ipc {
            message: format!("decode: {e}"),
        })
    }

    fn encode<T: serde::Serialize>(value: &T) -> HelperResult<Vec<u8>> {
        serde_json::to_vec(value).map_err(|e| HelperError::Ipc {
            message: format!("encode: {e}"),
        })
    }

    async fn do_service_status(
        &self,
        sender: &str,
    ) -> Result<ServiceStatusResponse, HelperError> {
        let bytes = self
            .dispatch_call(sender, HelperMethod::ServiceStatus, vec![], AuxStream::none())
            .await?;
        Self::decode(&bytes)
    }

    async fn do_home_status(
        &self,
        sender: &str,
    ) -> Result<boxpilot_ipc::HomeStatusResponse, HelperError> {
        let bytes = self
            .dispatch_call(sender, HelperMethod::HomeStatus, vec![], AuxStream::none())
            .await?;
        Self::decode(&bytes)
    }

    async fn do_service_start(
        &self,
        sender: &str,
    ) -> Result<boxpilot_ipc::ServiceControlResponse, HelperError> {
        let bytes = self
            .dispatch_call(sender, HelperMethod::ServiceStart, vec![], AuxStream::none())
            .await?;
        Self::decode(&bytes)
    }

    async fn do_service_stop(
        &self,
        sender: &str,
    ) -> Result<boxpilot_ipc::ServiceControlResponse, HelperError> {
        let bytes = self
            .dispatch_call(sender, HelperMethod::ServiceStop, vec![], AuxStream::none())
            .await?;
        Self::decode(&bytes)
    }

    async fn do_service_restart(
        &self,
        sender: &str,
    ) -> Result<boxpilot_ipc::ServiceControlResponse, HelperError> {
        let bytes = self
            .dispatch_call(
                sender,
                HelperMethod::ServiceRestart,
                vec![],
                AuxStream::none(),
            )
            .await?;
        Self::decode(&bytes)
    }

    async fn do_service_enable(
        &self,
        sender: &str,
    ) -> Result<boxpilot_ipc::ServiceControlResponse, HelperError> {
        let bytes = self
            .dispatch_call(
                sender,
                HelperMethod::ServiceEnable,
                vec![],
                AuxStream::none(),
            )
            .await?;
        Self::decode(&bytes)
    }

    async fn do_service_disable(
        &self,
        sender: &str,
    ) -> Result<boxpilot_ipc::ServiceControlResponse, HelperError> {
        let bytes = self
            .dispatch_call(
                sender,
                HelperMethod::ServiceDisable,
                vec![],
                AuxStream::none(),
            )
            .await?;
        Self::decode(&bytes)
    }

    async fn do_service_install_managed(
        &self,
        sender: &str,
    ) -> Result<boxpilot_ipc::ServiceInstallManagedResponse, HelperError> {
        let bytes = self
            .dispatch_call(
                sender,
                HelperMethod::ServiceInstallManaged,
                vec![],
                AuxStream::none(),
            )
            .await?;
        Self::decode(&bytes)
    }

    async fn do_service_logs(
        &self,
        sender: &str,
        req: boxpilot_ipc::ServiceLogsRequest,
    ) -> Result<boxpilot_ipc::ServiceLogsResponse, HelperError> {
        let body = Self::encode(&req)?;
        let bytes = self
            .dispatch_call(sender, HelperMethod::ServiceLogs, body, AuxStream::none())
            .await?;
        Self::decode(&bytes)
    }

    async fn do_core_discover(
        &self,
        sender: &str,
    ) -> Result<boxpilot_ipc::CoreDiscoverResponse, HelperError> {
        let bytes = self
            .dispatch_call(sender, HelperMethod::CoreDiscover, vec![], AuxStream::none())
            .await?;
        Self::decode(&bytes)
    }

    async fn do_core_install_managed(
        &self,
        sender: &str,
        req: boxpilot_ipc::CoreInstallRequest,
    ) -> Result<boxpilot_ipc::CoreInstallResponse, HelperError> {
        let body = Self::encode(&req)?;
        let bytes = self
            .dispatch_call(
                sender,
                HelperMethod::CoreInstallManaged,
                body,
                AuxStream::none(),
            )
            .await?;
        Self::decode(&bytes)
    }

    async fn do_core_upgrade_managed(
        &self,
        sender: &str,
        req: boxpilot_ipc::CoreInstallRequest,
    ) -> Result<boxpilot_ipc::CoreInstallResponse, HelperError> {
        let body = Self::encode(&req)?;
        let bytes = self
            .dispatch_call(
                sender,
                HelperMethod::CoreUpgradeManaged,
                body,
                AuxStream::none(),
            )
            .await?;
        Self::decode(&bytes)
    }

    async fn do_core_rollback_managed(
        &self,
        sender: &str,
        req: boxpilot_ipc::CoreRollbackRequest,
    ) -> Result<boxpilot_ipc::CoreInstallResponse, HelperError> {
        let body = Self::encode(&req)?;
        let bytes = self
            .dispatch_call(
                sender,
                HelperMethod::CoreRollbackManaged,
                body,
                AuxStream::none(),
            )
            .await?;
        Self::decode(&bytes)
    }

    async fn do_core_adopt(
        &self,
        sender: &str,
        req: boxpilot_ipc::CoreAdoptRequest,
    ) -> Result<boxpilot_ipc::CoreInstallResponse, HelperError> {
        let body = Self::encode(&req)?;
        let bytes = self
            .dispatch_call(sender, HelperMethod::CoreAdopt, body, AuxStream::none())
            .await?;
        Self::decode(&bytes)
    }

    async fn do_profile_activate_bundle(
        &self,
        sender: &str,
        req: boxpilot_ipc::ActivateBundleRequest,
        fd: std::os::fd::OwnedFd,
    ) -> Result<boxpilot_ipc::ActivateBundleResponse, HelperError> {
        let body = Self::encode(&req)?;
        let aux = AuxStream::from_owned_fd(fd);
        let bytes = self
            .dispatch_call(sender, HelperMethod::ProfileActivateBundle, body, aux)
            .await?;
        Self::decode(&bytes)
    }

    async fn do_profile_rollback_release(
        &self,
        sender: &str,
        req: boxpilot_ipc::RollbackRequest,
    ) -> Result<boxpilot_ipc::ActivateBundleResponse, HelperError> {
        let body = Self::encode(&req)?;
        let bytes = self
            .dispatch_call(
                sender,
                HelperMethod::ProfileRollbackRelease,
                body,
                AuxStream::none(),
            )
            .await?;
        Self::decode(&bytes)
    }

    async fn do_legacy_observe_service(
        &self,
        sender: &str,
    ) -> Result<boxpilot_ipc::LegacyObserveServiceResponse, HelperError> {
        let bytes = self
            .dispatch_call(
                sender,
                HelperMethod::LegacyObserveService,
                vec![],
                AuxStream::none(),
            )
            .await?;
        Self::decode(&bytes)
    }

    async fn do_legacy_migrate_service(
        &self,
        sender: &str,
        req: boxpilot_ipc::LegacyMigrateRequest,
    ) -> Result<boxpilot_ipc::LegacyMigrateResponse, HelperError> {
        let body = Self::encode(&req)?;
        let bytes = self
            .dispatch_call(
                sender,
                HelperMethod::LegacyMigrateService,
                body,
                AuxStream::none(),
            )
            .await?;
        Self::decode(&bytes)
    }

    async fn do_diagnostics_export_redacted(
        &self,
        sender: &str,
    ) -> Result<boxpilot_ipc::DiagnosticsExportResponse, HelperError> {
        let bytes = self
            .dispatch_call(
                sender,
                HelperMethod::DiagnosticsExportRedacted,
                vec![],
                AuxStream::none(),
            )
            .await?;
        Self::decode(&bytes)
    }

    /// `controller.transfer` is a stub today — it runs the dispatch
    /// chokepoint (so denied callers see NotAuthorized) and surfaces
    /// NotImplemented for the real transition. Returns `()` because the
    /// stub has no payload.
    async fn do_controller_transfer(&self, sender: &str) -> Result<(), HelperError> {
        self.dispatch_call(
            sender,
            HelperMethod::ControllerTransfer,
            vec![],
            AuxStream::none(),
        )
        .await
        .map(|_| ())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::authority::testing::CannedAuthority;
    use crate::context::testing::ctx_with;
    use crate::context::testing::ctx_with_journal_lines;
    use crate::context::testing::ctx_with_recording;
    use crate::dispatch;
    use crate::systemd::testing::{RecordedCall, RecordingSystemd};
    use boxpilot_ipc::UnitState;
    use tempfile::tempdir;

    /// Guard test (per COQ17 / Round 4 finding 4.8): D-Bus wire names are
    /// part of the .deb-shipped polkit + dbus service files. Changing them
    /// without a corresponding deb postinst migration breaks already-installed
    /// users.
    #[test]
    fn dbus_wire_names_are_frozen() {
        assert_eq!(
            super::BUS_NAME,
            "app.boxpilot.Helper",
            "Bus name change requires deb postinst migration of \
             /usr/share/dbus-1/system-services/app.boxpilot.Helper.service \
             and the polkit policy file"
        );
        assert_eq!(
            super::OBJECT_PATH,
            "/app/boxpilot/Helper",
            "Object path change requires updating Tauri's HelperProxy default_path \
             (boxpilot-tauri/src/helper_client.rs)"
        );
    }

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
            platform_extra: boxpilot_ipc::PlatformUnitExtra::Linux,
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
        let principal = CallerPrincipal::LinuxUid(1000);
        let r = dispatch::authorize(&h.ctx, &principal, HelperMethod::CoreDiscover).await;
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
        let principal = CallerPrincipal::LinuxUid(1000);
        let r = dispatch::authorize(&ctx, &principal, HelperMethod::CoreDiscover).await;
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

    /// Regression for the missing `maybe_claim_controller` + commit in the
    /// service.{start,stop,restart,enable,disable} handlers (PR #4 review
    /// finding r3158446532): when `ControllerState::Unset` and polkit
    /// allows the mutating call, the handler must persist the claim so the
    /// next mutating call sees `ControllerState::Set` and refuses non-
    /// controllers under §6.6. Uses the running process's own uid so the
    /// real `PasswdLookup` resolves a username on the CI host.
    #[tokio::test]
    async fn service_start_claims_controller_when_unset() {
        let me_uid = nix::unistd::Uid::current().as_raw();
        let tmp = tempdir().unwrap();
        let rec = Arc::new(RecordingSystemd::new(UnitState::NotFound));
        // No controller_uid in the toml → ControllerState::Unset.
        let ctx = Arc::new(ctx_with_recording(
            &tmp,
            Some("schema_version = 1\n"),
            CannedAuthority::allowing(&["app.boxpilot.helper.service.start"]),
            rec.clone(),
            &[(":1.42", me_uid)],
        ));
        let paths = ctx.paths.clone();
        let h = Helper::new(ctx);
        h.do_service_start(":1.42").await.unwrap();
        let toml_after = std::fs::read_to_string(paths.boxpilot_toml()).unwrap();
        assert!(
            toml_after.contains(&format!("controller_uid = {me_uid}")),
            "expected controller_uid = {me_uid} in toml, got:\n{toml_after}"
        );
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
            platform_extra: boxpilot_ipc::PlatformUnitExtra::Linux,
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
        let paths = boxpilot_platform::Paths::with_root(tmp.path());
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

    /// Regression for the round-4 review finding on `legacy_migrate_service`:
    /// `dispatch::maybe_claim_controller` must NOT run on a `Prepare` request,
    /// even when `will_claim_controller` is true. Prepare is read-only per the
    /// `legacy::migrate` module doc, so a missing-from-passwd uid (which would
    /// normally trip `HelperError::ControllerOrphaned`) must not be able to
    /// abort it.
    ///
    /// The 4_000_000_000 uid is virtually guaranteed to be absent from the
    /// CI host's `/etc/passwd`, so `PasswdLookup::lookup_username(4B)` returns
    /// `None`. Combined with `controller_uid` absent from the toml (→
    /// `ControllerState::Unset` → `will_claim_controller = true`), the pre-fix
    /// code path would have returned `ControllerOrphaned` here. After aacbfba
    /// the lookup is gated behind `matches!(resp, Cutover(_))`, so Prepare
    /// surfaces the underlying `LegacyUnitNotFound` instead.
    #[tokio::test]
    async fn legacy_migrate_prepare_does_not_trigger_controller_orphaned_for_unknown_uid() {
        let tmp = tempdir().unwrap();
        let ctx = Arc::new(ctx_with(
            &tmp,
            None, // ControllerState::Unset → will_claim_controller=true
            CannedAuthority::allowing(&["app.boxpilot.helper.legacy.migrate-service"]),
            UnitState::NotFound,
            &[(":1.42", 4_000_000_000)],
        ));
        let h = Helper::new(ctx);
        let r = h
            .do_legacy_migrate_service(":1.42", boxpilot_ipc::LegacyMigrateRequest::Prepare)
            .await;
        match r {
            Err(HelperError::LegacyUnitNotFound { .. }) => {}
            Err(HelperError::ControllerOrphaned) => panic!(
                "Prepare must not call maybe_claim_controller (regression — \
                 controller-claim escaped the Cutover gate)"
            ),
            other => panic!("expected LegacyUnitNotFound, got {other:?}"),
        }
    }
}
