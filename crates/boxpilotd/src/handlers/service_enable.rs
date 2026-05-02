//! `service.enable` handler — mutating; controller required.

use crate::context::HelperContext;
use crate::dispatch;
use boxpilot_ipc::{HelperError, HelperMethod, HelperResult};
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
            message: "service.enable takes no aux stream".into(),
        });
    }
    let _call = dispatch::authorize(&ctx, &principal, HelperMethod::ServiceEnable).await?;
    let cfg = ctx.load_config().await?;
    let resp = crate::service::control::run(
        crate::service::control::Verb::Enable,
        &cfg.target_service,
        &*ctx.systemd,
    )
    .await?;
    serde_json::to_vec(&resp).map_err(|e| HelperError::Ipc {
        message: format!("encode: {e}"),
    })
}
