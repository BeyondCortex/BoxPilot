//! Cross-platform IPC fake. `pair(caller)` returns a connected
//! `(FakeIpcClient, FakeIpcServer)` pair backed by a tokio mpsc channel
//! plus a per-call oneshot reply. Lets helper-side integration smoke
//! tests exercise the full `IpcServer -> HelperDispatch -> handlers::*`
//! pipeline without needing a real D-Bus or named-pipe broker.
//!
//! Compiles on every target, per AC4 / spec §10.

use crate::traits::authority::CallerPrincipal;
use crate::traits::bundle_aux::AuxStream;
use crate::traits::ipc::{ConnectionInfo, HelperDispatch, IpcClient, IpcServer};
use async_trait::async_trait;
use boxpilot_ipc::{HelperError, HelperMethod, HelperResult};
use std::sync::Arc;
use tokio::sync::{mpsc, oneshot, Mutex};

struct FakeCall {
    method: HelperMethod,
    body: Vec<u8>,
    aux: AuxStream,
    reply: oneshot::Sender<HelperResult<Vec<u8>>>,
}

pub struct FakeIpcClient {
    tx: mpsc::Sender<FakeCall>,
}

#[async_trait]
impl IpcClient for FakeIpcClient {
    async fn call(
        &self,
        method: HelperMethod,
        body: Vec<u8>,
        aux: AuxStream,
    ) -> HelperResult<Vec<u8>> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.tx
            .send(FakeCall {
                method,
                body,
                aux,
                reply: reply_tx,
            })
            .await
            .map_err(|_| HelperError::Ipc {
                message: "fake ipc: server dropped".into(),
            })?;
        reply_rx.await.map_err(|_| HelperError::Ipc {
            message: "fake ipc: reply dropped".into(),
        })?
    }
}

pub struct FakeIpcServer {
    rx: Mutex<mpsc::Receiver<FakeCall>>,
    caller: CallerPrincipal,
}

#[async_trait]
impl IpcServer for FakeIpcServer {
    async fn run(&self, dispatch: Arc<dyn HelperDispatch>) -> Result<(), HelperError> {
        let mut rx = self.rx.lock().await;
        while let Some(call) = rx.recv().await {
            let conn = ConnectionInfo {
                caller: self.caller.clone(),
            };
            let result = dispatch
                .handle(conn, call.method, call.body, call.aux)
                .await;
            // If the client dropped its receiver, just continue — the
            // next call still routes correctly.
            let _ = call.reply.send(result);
        }
        Ok(())
    }
}

/// Construct a `(client, server)` pair where every `client.call(...)`
/// arrives at `server.run(dispatch)` carrying the supplied
/// `caller` as the `ConnectionInfo`.
pub fn pair(caller: CallerPrincipal) -> (FakeIpcClient, FakeIpcServer) {
    let (tx, rx) = mpsc::channel(16);
    (
        FakeIpcClient { tx },
        FakeIpcServer {
            rx: Mutex::new(rx),
            caller,
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use boxpilot_ipc::HelperResult;

    struct EchoDispatch;

    #[async_trait]
    impl HelperDispatch for EchoDispatch {
        async fn handle(
            &self,
            _conn: ConnectionInfo,
            _method: HelperMethod,
            body: Vec<u8>,
            _aux: AuxStream,
        ) -> HelperResult<Vec<u8>> {
            Ok(body)
        }
    }

    #[tokio::test]
    async fn pair_round_trips_through_dispatch() {
        let (client, server) = pair(CallerPrincipal::LinuxUid(1000));
        let dispatch: Arc<dyn HelperDispatch> = Arc::new(EchoDispatch);

        let server_task = tokio::spawn(async move {
            let _ = server.run(dispatch).await;
        });

        let body = b"hello".to_vec();
        let resp = client
            .call(HelperMethod::ServiceStatus, body.clone(), AuxStream::none())
            .await
            .unwrap();
        assert_eq!(resp, body);

        // Drop the client to close the channel and let the server task
        // exit cleanly.
        drop(client);
        server_task.await.unwrap();
    }

    #[tokio::test]
    async fn dropped_server_returns_ipc_error() {
        let (client, server) = pair(CallerPrincipal::LinuxUid(1000));
        drop(server);
        let r = client
            .call(HelperMethod::ServiceStatus, vec![], AuxStream::none())
            .await;
        assert!(matches!(r, Err(HelperError::Ipc { .. })));
    }
}
