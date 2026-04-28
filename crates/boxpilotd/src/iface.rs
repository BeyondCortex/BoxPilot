//! `app.boxpilot.Helper1` D-Bus interface. Each method goes through
//! [`crate::dispatch::authorize`] before doing any work.
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
    };
    let msg = e.to_string();
    // We use zbus::fdo::Error::Failed as the carrier; the precise mapping
    // gets refined when zbus exposes a way to set arbitrary error names
    // from interface methods. For now, encode the typed name into the
    // message prefix so the GUI can still discriminate.
    zbus::fdo::Error::Failed(format!("{name}: {msg}"))
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
        let sender = header.sender().ok_or_else(|| {
            zbus::fdo::Error::Failed(
                "app.boxpilot.Helper1.Ipc: missing sender on incoming message".into(),
            )
        })?;
        let resp = self.do_service_status(&sender.to_string()).await.map_err(to_zbus_err)?;
        // Wire format on D-Bus is a single JSON string. We use JSON rather
        // than a nested zbus dict so the IPC types live in one Rust type
        // hierarchy and the GUI can deserialize via serde without a
        // bespoke zvariant→TS layer.
        Ok(serde_json::to_string(&resp).map_err(|e| {
            zbus::fdo::Error::Failed(format!("app.boxpilot.Helper1.Ipc: serialize: {e}"))
        })?)
    }

    // ----- Stubs for the other 18 actions (filled in by plans #2-#9). -----
    async fn service_start(&self) -> zbus::fdo::Result<String> { stub() }
    async fn service_stop(&self) -> zbus::fdo::Result<String> { stub() }
    async fn service_restart(&self) -> zbus::fdo::Result<String> { stub() }
    async fn service_enable(&self) -> zbus::fdo::Result<String> { stub() }
    async fn service_disable(&self) -> zbus::fdo::Result<String> { stub() }
    async fn service_install_managed(&self) -> zbus::fdo::Result<String> { stub() }
    async fn service_logs(&self) -> zbus::fdo::Result<String> { stub() }
    async fn profile_activate_bundle(&self) -> zbus::fdo::Result<String> { stub() }
    async fn profile_rollback_release(&self) -> zbus::fdo::Result<String> { stub() }
    async fn core_discover(&self) -> zbus::fdo::Result<String> { stub() }
    async fn core_install_managed(&self) -> zbus::fdo::Result<String> { stub() }
    async fn core_upgrade_managed(&self) -> zbus::fdo::Result<String> { stub() }
    async fn core_rollback_managed(&self) -> zbus::fdo::Result<String> { stub() }
    async fn core_adopt(&self) -> zbus::fdo::Result<String> { stub() }
    async fn legacy_observe_service(&self) -> zbus::fdo::Result<String> { stub() }
    async fn legacy_migrate_service(&self) -> zbus::fdo::Result<String> { stub() }
    async fn controller_transfer(&self) -> zbus::fdo::Result<String> { stub() }
    async fn diagnostics_export_redacted(&self) -> zbus::fdo::Result<String> { stub() }
}

fn stub() -> zbus::fdo::Result<String> {
    warn!("called a not-yet-implemented helper method");
    Err(to_zbus_err(HelperError::NotImplemented))
}

impl Helper {
    async fn do_service_status(
        &self,
        sender_bus_name: &str,
    ) -> Result<ServiceStatusResponse, HelperError> {
        let _call = dispatch::authorize(&self.ctx, sender_bus_name, HelperMethod::ServiceStatus).await?;
        let cfg = self.ctx.load_config().await?;
        let unit_name = cfg.target_service.clone();
        let unit_state = self.ctx.systemd.unit_state(&unit_name).await?;
        let controller = self.ctx.controller_state().await?.to_status();
        Ok(ServiceStatusResponse { unit_name, unit_state, controller })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::authority::testing::CannedAuthority;
    use crate::context::testing::ctx_with;
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
}
