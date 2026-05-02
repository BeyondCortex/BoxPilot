//! `DispatchHandler` — the cross-platform `HelperDispatch` impl that routes
//! `(method, body, aux)` tuples to the appropriate per-verb handler in
//! [`crate::handlers`]. The handler-side counterpart to `IpcServer`: it
//! does not know what transport delivered the call (D-Bus, named pipe,
//! mpsc fake), only the typed `HelperMethod` and the resolved
//! `CallerPrincipal`.
//!
//! Aux-shape contract (per `HelperMethod::aux_shape`) is checked at the
//! top of [`DispatchHandler::handle`] before any work runs. A mismatch
//! returns `HelperError::Ipc` rather than reaching the handler.

use crate::context::HelperContext;
use crate::handlers;
use async_trait::async_trait;
use boxpilot_ipc::method::wire::AuxShape;
use boxpilot_ipc::{HelperError, HelperMethod, HelperResult};
use boxpilot_platform::traits::bundle_aux::AuxStream;
use boxpilot_platform::traits::ipc::{ConnectionInfo, HelperDispatch};
use std::sync::Arc;

pub struct DispatchHandler {
    ctx: Arc<HelperContext>,
}

impl DispatchHandler {
    pub fn new(ctx: Arc<HelperContext>) -> Self {
        Self { ctx }
    }
}

#[async_trait]
impl HelperDispatch for DispatchHandler {
    async fn handle(
        &self,
        conn: ConnectionInfo,
        method: HelperMethod,
        body: Vec<u8>,
        aux: AuxStream,
    ) -> HelperResult<Vec<u8>> {
        // Aux-shape contract: enforce before invoking the per-verb handler
        // so a wire-level mismatch surfaces as a single, uniform error
        // instead of leaking into the handler's local checks.
        match (method.aux_shape(), aux.is_none()) {
            (AuxShape::None, false) => {
                return Err(HelperError::Ipc {
                    message: format!("{:?} expects no aux stream", method),
                });
            }
            (AuxShape::Required, true) => {
                return Err(HelperError::Ipc {
                    message: format!("{:?} requires an aux stream", method),
                });
            }
            _ => {}
        }

        let ctx = self.ctx.clone();
        let principal = conn.caller;
        match method {
            HelperMethod::ServiceStatus => {
                handlers::service_status::handle(ctx, principal, body, aux).await
            }
            #[cfg(target_os = "linux")]
            HelperMethod::ServiceStart => {
                handlers::service_start::handle(ctx, principal, body, aux).await
            }
            #[cfg(target_os = "linux")]
            HelperMethod::ServiceStop => {
                handlers::service_stop::handle(ctx, principal, body, aux).await
            }
            #[cfg(target_os = "linux")]
            HelperMethod::ServiceRestart => {
                handlers::service_restart::handle(ctx, principal, body, aux).await
            }
            #[cfg(target_os = "linux")]
            HelperMethod::ServiceEnable => {
                handlers::service_enable::handle(ctx, principal, body, aux).await
            }
            #[cfg(target_os = "linux")]
            HelperMethod::ServiceDisable => {
                handlers::service_disable::handle(ctx, principal, body, aux).await
            }
            #[cfg(target_os = "linux")]
            HelperMethod::ServiceInstallManaged => {
                handlers::service_install_managed::handle(ctx, principal, body, aux).await
            }
            #[cfg(target_os = "linux")]
            HelperMethod::ServiceLogs => {
                handlers::service_logs::handle(ctx, principal, body, aux).await
            }
            #[cfg(target_os = "linux")]
            HelperMethod::ProfileActivateBundle => {
                handlers::profile_activate_bundle::handle(ctx, principal, body, aux).await
            }
            #[cfg(target_os = "linux")]
            HelperMethod::ProfileRollbackRelease => {
                handlers::profile_rollback_release::handle(ctx, principal, body, aux).await
            }
            #[cfg(target_os = "linux")]
            HelperMethod::CoreDiscover => {
                handlers::core_discover::handle(ctx, principal, body, aux).await
            }
            #[cfg(target_os = "linux")]
            HelperMethod::CoreInstallManaged => {
                handlers::core_install_managed::handle(ctx, principal, body, aux).await
            }
            #[cfg(target_os = "linux")]
            HelperMethod::CoreUpgradeManaged => {
                handlers::core_upgrade_managed::handle(ctx, principal, body, aux).await
            }
            #[cfg(target_os = "linux")]
            HelperMethod::CoreRollbackManaged => {
                handlers::core_rollback_managed::handle(ctx, principal, body, aux).await
            }
            #[cfg(target_os = "linux")]
            HelperMethod::CoreAdopt => {
                handlers::core_adopt::handle(ctx, principal, body, aux).await
            }
            #[cfg(target_os = "linux")]
            HelperMethod::LegacyObserveService => {
                handlers::legacy_observe_service::handle(ctx, principal, body, aux).await
            }
            #[cfg(target_os = "linux")]
            HelperMethod::LegacyMigrateService => {
                handlers::legacy_migrate_service::handle(ctx, principal, body, aux).await
            }
            HelperMethod::ControllerTransfer => {
                handlers::controller_transfer::handle(ctx, principal, body, aux).await
            }
            HelperMethod::DiagnosticsExportRedacted => {
                handlers::diagnostics_export_redacted::handle(ctx, principal, body, aux).await
            }
            HelperMethod::HomeStatus => {
                handlers::home_status::handle(ctx, principal, body, aux).await
            }
            // Windows batch ③/④ will fill in real implementations for the
            // Linux-only verbs above. Until then, return NotImplemented so
            // a Windows caller sees a clear error rather than a compile
            // failure.
            #[cfg(not(target_os = "linux"))]
            _ => Err(HelperError::NotImplemented),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::authority::testing::CannedAuthority;
    use crate::authority::CallerPrincipal;
    use crate::context::testing::ctx_with;
    use boxpilot_ipc::UnitState;
    use tempfile::tempdir;

    #[tokio::test]
    async fn dispatch_routes_service_status_through_handler() {
        let tmp = tempdir().unwrap();
        let ctx = Arc::new(ctx_with(
            &tmp,
            None,
            CannedAuthority::allowing(&["app.boxpilot.helper.service.status"]),
            UnitState::NotFound,
            &[(":1.42", 1000)],
        ));
        let dispatch = DispatchHandler::new(ctx);
        let conn = ConnectionInfo {
            caller: CallerPrincipal::LinuxUid(1000),
        };
        let body = dispatch
            .handle(conn, HelperMethod::ServiceStatus, vec![], AuxStream::none())
            .await
            .unwrap();
        let resp: boxpilot_ipc::ServiceStatusResponse = serde_json::from_slice(&body).unwrap();
        assert_eq!(resp.unit_name, "boxpilot-sing-box.service");
    }

    #[tokio::test]
    async fn dispatch_rejects_unexpected_aux_for_nullary_verb() {
        let tmp = tempdir().unwrap();
        let ctx = Arc::new(ctx_with(
            &tmp,
            None,
            CannedAuthority::allowing(&["app.boxpilot.helper.service.status"]),
            UnitState::NotFound,
            &[(":1.42", 1000)],
        ));
        let dispatch = DispatchHandler::new(ctx);
        let conn = ConnectionInfo {
            caller: CallerPrincipal::LinuxUid(1000),
        };
        // Hand it an AsyncRead-backed aux for a verb whose AuxShape is None.
        let aux = AuxStream::from_async_read(tokio::io::empty());
        let r = dispatch
            .handle(conn, HelperMethod::ServiceStatus, vec![], aux)
            .await;
        assert!(matches!(r, Err(HelperError::Ipc { .. })));
    }

    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn dispatch_rejects_missing_aux_for_activate_bundle() {
        let tmp = tempdir().unwrap();
        let ctx = Arc::new(ctx_with(
            &tmp,
            None,
            CannedAuthority::allowing(&["app.boxpilot.helper.profile.activate-bundle"]),
            UnitState::NotFound,
            &[(":1.42", 1000)],
        ));
        let dispatch = DispatchHandler::new(ctx);
        let conn = ConnectionInfo {
            caller: CallerPrincipal::LinuxUid(1000),
        };
        let r = dispatch
            .handle(
                conn,
                HelperMethod::ProfileActivateBundle,
                vec![],
                AuxStream::none(),
            )
            .await;
        assert!(matches!(r, Err(HelperError::Ipc { .. })));
    }
}
