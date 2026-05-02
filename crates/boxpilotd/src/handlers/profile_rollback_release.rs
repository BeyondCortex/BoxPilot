//! `profile.rollback_release` handler — mutating; may claim controller.

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
            message: "profile.rollback_release takes no aux stream".into(),
        });
    }
    let req: boxpilot_ipc::RollbackRequest =
        serde_json::from_slice(&body).map_err(|e| HelperError::Ipc {
            message: format!("decode: {e}"),
        })?;
    let call = dispatch::authorize(&ctx, &principal, HelperMethod::ProfileRollbackRelease).await?;
    let controller = dispatch::maybe_claim_controller(
        call.will_claim_controller,
        &call.principal,
        &*ctx.user_lookup,
    )?;
    let deps = crate::profile::rollback::RollbackDeps {
        paths: ctx.paths.clone(),
        systemd: &*ctx.systemd,
        verifier: &*ctx.verifier,
    };
    let resp = crate::profile::rollback::rollback_release(req, controller, &deps).await?;
    serde_json::to_vec(&resp).map_err(|e| HelperError::Ipc {
        message: format!("encode: {e}"),
    })
}
