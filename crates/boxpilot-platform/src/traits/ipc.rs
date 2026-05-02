//! Cross-platform IPC server / dispatch / client surface. Per spec §5.4 +
//! COQ8/9/10/15.

use crate::traits::authority::CallerPrincipal;
use crate::traits::bundle_aux::AuxStream;
use async_trait::async_trait;
use boxpilot_ipc::{HelperError, HelperMethod, HelperResult};

#[derive(Debug, Clone)]
pub struct ConnectionInfo {
    pub caller: CallerPrincipal,
}

#[async_trait]
pub trait IpcServer: Send + Sync {
    async fn run(&self, dispatch: std::sync::Arc<dyn HelperDispatch>) -> Result<(), HelperError>;
}

#[async_trait]
pub trait HelperDispatch: Send + Sync {
    async fn handle(
        &self,
        conn: ConnectionInfo,
        method: HelperMethod,
        body: Vec<u8>,
        aux: AuxStream,
    ) -> HelperResult<Vec<u8>>;
}

#[async_trait]
pub trait IpcClient: Send + Sync {
    async fn call(
        &self,
        method: HelperMethod,
        body: Vec<u8>,
        aux: AuxStream,
    ) -> HelperResult<Vec<u8>>;
}
