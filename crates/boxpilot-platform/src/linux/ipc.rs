//! Linux `IpcServer` + `IpcClient` impls. The server side is a thin
//! "block until shutdown" wrapper — the actual D-Bus interface methods
//! live in `boxpilotd::iface` (a thin shell over [`HelperDispatch`]).
//! The client side wraps `zbus::Proxy` so `boxpilot-tauri` no longer
//! needs to depend on `zbus` directly.

use crate::traits::bundle_aux::{AuxStream, AuxStreamRepr};
use crate::traits::ipc::{HelperDispatch, IpcClient, IpcServer};
use async_trait::async_trait;
use boxpilot_ipc::{HelperError, HelperMethod, HelperResult};
use std::sync::Arc;
use tokio::sync::Notify;
use zbus::zvariant::OwnedFd as ZbusOwnedFd;
use zbus::Connection;

const BUS_NAME: &str = "app.boxpilot.Helper";
const OBJECT_PATH: &str = "/app/boxpilot/Helper";
const INTERFACE: &str = "app.boxpilot.Helper1";

pub struct ZbusIpcServer {
    /// Held to keep the bus connection alive for the duration of `run`.
    /// The actual interface is registered on `conn` from `main.rs` before
    /// `run` is awaited.
    pub conn: Connection,
    /// Signaled by the caller (typically `main.rs` on SIGTERM/SIGINT) to
    /// unblock [`IpcServer::run`]. Wrapped in `Arc` so the caller can hold
    /// the trigger end while the server holds the wait end.
    pub stop: Arc<Notify>,
}

#[async_trait]
impl IpcServer for ZbusIpcServer {
    /// `dispatch` is unused here — the legacy `iface.rs` shell already
    /// holds an `Arc<dyn HelperDispatch>` and routes through it. This
    /// signature exists so PR 11b can converge the Linux + Windows IPC
    /// impls onto the same trait, at which point this method will accept
    /// the dispatch and register the interface itself instead of relying
    /// on `main.rs` to do the registration.
    async fn run(&self, _dispatch: Arc<dyn HelperDispatch>) -> Result<(), HelperError> {
        // Hold a reference to the connection so it isn't dropped while
        // we wait. zbus serves requests on background tasks owned by
        // `conn`, so we just need to keep it alive until the shutdown
        // signal arrives.
        let _conn = self.conn.clone();
        self.stop.notified().await;
        Ok(())
    }
}

/// Linux `IpcClient` over the system D-Bus. Wraps `zbus::Proxy` so the
/// GUI can call `app.boxpilot.Helper1.<Method>` without taking a direct
/// `zbus` dependency.
pub struct ZbusIpcClient {
    pub conn: Connection,
}

impl ZbusIpcClient {
    pub async fn connect_system() -> Result<Self, HelperError> {
        let conn = Connection::system().await.map_err(|e| HelperError::Ipc {
            message: format!("connect to system bus: {e}"),
        })?;
        Ok(Self { conn })
    }
}

#[async_trait]
impl IpcClient for ZbusIpcClient {
    async fn call(
        &self,
        method: HelperMethod,
        body: Vec<u8>,
        aux: AuxStream,
    ) -> HelperResult<Vec<u8>> {
        let proxy = zbus::Proxy::new(&self.conn, BUS_NAME, OBJECT_PATH, INTERFACE)
            .await
            .map_err(|e| HelperError::Ipc {
                message: format!("build proxy: {e}"),
            })?;

        let method_name = camel_case_for(method);
        let body_str = std::str::from_utf8(&body).map_err(|e| HelperError::Ipc {
            message: format!("body not utf-8: {e}"),
        })?;

        let resp_str: String = match aux.into_repr() {
            AuxStreamRepr::None => {
                if body.is_empty() {
                    proxy.call(method_name.as_str(), &()).await
                } else {
                    proxy.call(method_name.as_str(), &body_str).await
                }
                .map_err(|e| HelperError::Ipc {
                    message: format!("call {method_name}: {e}"),
                })?
            }
            AuxStreamRepr::AsyncRead(_) => {
                return Err(HelperError::Ipc {
                    message: "linux IpcClient: AsyncRead aux not supported (use from_owned_fd)"
                        .into(),
                });
            }
            AuxStreamRepr::LinuxFd(fd) => {
                let z_fd = ZbusOwnedFd::from(fd);
                proxy
                    .call::<_, _, String>(method_name.as_str(), &(body_str, z_fd))
                    .await
                    .map_err(|e| HelperError::Ipc {
                        message: format!("call {method_name} with fd: {e}"),
                    })?
            }
        };

        Ok(resp_str.into_bytes())
    }
}

/// `HelperMethod` → D-Bus method name. zbus converts `fn service_status`
/// in the helper's `#[interface]` impl to `ServiceStatus` on the wire by
/// default, which matches `format!("{HelperMethod::ServiceStatus:?}")`.
fn camel_case_for(m: HelperMethod) -> String {
    format!("{m:?}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn camel_case_matches_dbus_method_names() {
        assert_eq!(camel_case_for(HelperMethod::ServiceStatus), "ServiceStatus");
        assert_eq!(
            camel_case_for(HelperMethod::ProfileActivateBundle),
            "ProfileActivateBundle"
        );
        assert_eq!(camel_case_for(HelperMethod::HomeStatus), "HomeStatus");
        assert_eq!(
            camel_case_for(HelperMethod::DiagnosticsExportRedacted),
            "DiagnosticsExportRedacted"
        );
    }
}
