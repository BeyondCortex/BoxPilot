//! `controller.transfer` handler — stub. Per spec §6.6, this verb runs the
//! authorize chokepoint (so denied callers see NotAuthorized, not
//! NotImplemented) and returns NotImplemented for the real transition that
//! later plans will fill in.

use crate::context::HelperContext;
use crate::dispatch;
use boxpilot_ipc::{HelperError, HelperMethod, HelperResult};
use boxpilot_platform::traits::authority::CallerPrincipal;
use boxpilot_platform::traits::bundle_aux::AuxStream;
use std::sync::Arc;
use tracing::warn;

pub async fn handle(
    ctx: Arc<HelperContext>,
    principal: CallerPrincipal,
    _body: Vec<u8>,
    aux: AuxStream,
) -> HelperResult<Vec<u8>> {
    if !aux.is_none() {
        return Err(HelperError::Ipc {
            message: "controller.transfer takes no aux stream".into(),
        });
    }
    dispatch::authorize(&ctx, &principal, HelperMethod::ControllerTransfer).await?;
    warn!(
        method = HelperMethod::ControllerTransfer.as_logical(),
        "called a not-yet-implemented helper method"
    );
    Err(HelperError::NotImplemented)
}
