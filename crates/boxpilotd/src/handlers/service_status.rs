//! `service.status` handler — read-only.

use crate::context::HelperContext;
use crate::dispatch;
use boxpilot_ipc::{HelperError, HelperMethod, HelperResult, ServiceStatusResponse};
use boxpilot_platform::traits::authority::CallerPrincipal;
use boxpilot_platform::traits::bundle_aux::AuxStream;
use std::sync::Arc;

pub async fn handle(
    ctx: Arc<HelperContext>,
    principal: CallerPrincipal,
    _body: Vec<u8>,
    aux: AuxStream,
) -> HelperResult<Vec<u8>> {
    if !aux.is_none() {
        return Err(HelperError::Ipc {
            message: "service.status takes no aux stream".into(),
        });
    }
    let call = dispatch::authorize(&ctx, &principal, HelperMethod::ServiceStatus).await?;
    let cfg = ctx.load_config().await?;
    let unit_name = cfg.target_service.clone();
    let unit_state = ctx.systemd.unit_state(&unit_name).await?;
    let controller = call.controller.to_status();
    let resp = ServiceStatusResponse {
        unit_name,
        unit_state,
        controller,
        state_schema_mismatch: ctx.state_schema_mismatch,
    };
    serde_json::to_vec(&resp).map_err(|e| HelperError::Ipc {
        message: format!("encode: {e}"),
    })
}
