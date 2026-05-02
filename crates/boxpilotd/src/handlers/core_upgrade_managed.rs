//! `core.upgrade_managed` handler — mutating; may claim controller.

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
            message: "core.upgrade_managed takes no aux stream".into(),
        });
    }
    let req: boxpilot_ipc::CoreInstallRequest =
        serde_json::from_slice(&body).map_err(|e| HelperError::Ipc {
            message: format!("decode: {e}"),
        })?;
    let call = dispatch::authorize(&ctx, &principal, HelperMethod::CoreUpgradeManaged).await?;
    let controller = dispatch::maybe_claim_controller(
        call.will_claim_controller,
        &call.principal,
        &*ctx.user_lookup,
    )?;
    let deps = crate::core::install::InstallDeps {
        paths: ctx.paths.clone(),
        github: &*ctx.github,
        downloader: &*ctx.downloader,
        fs: &*ctx.fs_meta,
        version_checker: &*ctx.version_checker,
        current_pointer: ctx.current_pointer.clone(),
    };
    let resp = crate::core::install::install_or_upgrade(&req, &deps, controller).await?;
    serde_json::to_vec(&resp).map_err(|e| HelperError::Ipc {
        message: format!("encode: {e}"),
    })
}
