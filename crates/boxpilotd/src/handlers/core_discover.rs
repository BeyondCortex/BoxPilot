//! `core.discover` handler — read-only.

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
            message: "core.discover takes no aux stream".into(),
        });
    }
    let _call = dispatch::authorize(&ctx, &principal, HelperMethod::CoreDiscover).await?;
    let deps = crate::core::discover::DiscoverDeps {
        paths: ctx.paths.clone(),
        fs: &*ctx.fs_meta,
        version_checker: &*ctx.version_checker,
    };
    let resp = crate::core::discover::discover(&deps).await?;
    serde_json::to_vec(&resp).map_err(|e| HelperError::Ipc {
        message: format!("encode: {e}"),
    })
}
