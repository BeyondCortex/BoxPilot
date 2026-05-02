//! Linux `IpcServer` impl. The actual D-Bus interface methods live in
//! `boxpilotd::iface` (a thin shell over [`HelperDispatch`]); this struct
//! exists so [`IpcServer::run`] becomes the single "block until shutdown"
//! await point that `main.rs` waits on. Constructing the server keeps the
//! D-Bus connection + an opaque shutdown notifier; PR 12 will mirror the
//! same trait shape with a Windows named-pipe impl.

use crate::traits::ipc::{HelperDispatch, IpcServer};
use async_trait::async_trait;
use boxpilot_ipc::HelperError;
use std::sync::Arc;
use tokio::sync::Notify;
use zbus::Connection;

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
