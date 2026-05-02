//! `service.restart` handler — mutating; controller required.

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
            message: "service.restart takes no aux stream".into(),
        });
    }
    let call = dispatch::authorize(&ctx, &principal, HelperMethod::ServiceRestart).await?;
    let controller = dispatch::maybe_claim_controller(
        call.will_claim_controller,
        &call.principal,
        &*ctx.user_lookup,
    )?;
    let cfg = ctx.load_config().await?;
    let resp = crate::service::control::run(
        crate::service::control::Verb::Restart,
        &cfg.target_service,
        &*ctx.systemd,
    )
    .await?;
    dispatch::commit_controller_claim(&ctx.paths, controller).await?;
    serde_json::to_vec(&resp).map_err(|e| HelperError::Ipc {
        message: format!("encode: {e}"),
    })
}
