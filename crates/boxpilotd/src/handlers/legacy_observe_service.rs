//! `legacy.observe_service` handler — read-only.

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
            message: "legacy.observe_service takes no aux stream".into(),
        });
    }
    let _call = dispatch::authorize(&ctx, &principal, HelperMethod::LegacyObserveService).await?;
    let cfg = ctx.load_config().await?;
    let deps = crate::legacy::observe::ObserveDeps {
        systemd: &*ctx.systemd,
        fs_read: &*ctx.fs_fragment_reader,
    };
    let resp = crate::legacy::observe::observe(&cfg, &deps).await?;
    serde_json::to_vec(&resp).map_err(|e| HelperError::Ipc {
        message: format!("encode: {e}"),
    })
}
