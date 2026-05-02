//! `service.logs` handler — read-only.

use crate::context::HelperContext;
use crate::dispatch;
use boxpilot_ipc::{HelperError, HelperMethod, HelperResult};
use boxpilot_platform::traits::authority::CallerPrincipal;
use boxpilot_platform::traits::bundle_aux::AuxStream;
use std::sync::Arc;

pub async fn handle(
    ctx: Arc<HelperContext>,
    principal: CallerPrincipal,
    body: Vec<u8>,
    aux: AuxStream,
) -> HelperResult<Vec<u8>> {
    if !aux.is_none() {
        return Err(HelperError::Ipc {
            message: "service.logs takes no aux stream".into(),
        });
    }
    let req: boxpilot_ipc::ServiceLogsRequest =
        serde_json::from_slice(&body).map_err(|e| HelperError::Ipc {
            message: format!("decode: {e}"),
        })?;
    let _call = dispatch::authorize(&ctx, &principal, HelperMethod::ServiceLogs).await?;
    let cfg = ctx.load_config().await?;
    let resp = crate::service::logs::read(&req, &cfg.target_service, &*ctx.journal).await?;
    serde_json::to_vec(&resp).map_err(|e| HelperError::Ipc {
        message: format!("encode: {e}"),
    })
}
