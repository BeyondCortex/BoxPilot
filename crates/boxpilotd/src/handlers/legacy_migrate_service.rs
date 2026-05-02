//! `legacy.migrate_service` handler — high-risk; mutating.

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
            message: "legacy.migrate_service takes no aux stream".into(),
        });
    }
    let req: boxpilot_ipc::LegacyMigrateRequest =
        serde_json::from_slice(&body).map_err(|e| HelperError::Ipc {
            message: format!("decode: {e}"),
        })?;
    let call = dispatch::authorize(&ctx, &principal, HelperMethod::LegacyMigrateService).await?;
    let controller = dispatch::maybe_claim_controller(
        call.will_claim_controller,
        &call.principal,
        &*ctx.user_lookup,
    )?;
    let cfg = ctx.load_config().await?;
    let prep = crate::legacy::migrate::PrepareDeps {
        systemd: &*ctx.systemd,
        fs_read: &*ctx.fs_fragment_reader,
        config_reader: &*ctx.config_reader,
    };
    let backups_dir = ctx.paths.backups_units_dir();
    let cut = crate::legacy::migrate::CutoverDeps {
        systemd: &*ctx.systemd,
        backups_units_dir: &backups_dir,
        now_iso: || chrono::Utc::now().format("%Y-%m-%dT%H-%M-%SZ").to_string(),
    };
    let resp = crate::legacy::migrate::run(&cfg, req, &prep, &cut).await?;
    // §6.6: controller ownership is established by the first actual
    // mutation. `LegacyMigrateRequest::Prepare` is read-only with respect
    // to system state (per legacy::migrate module doc), so a will_claim
    // signal raised by dispatch::authorize for this method must NOT be
    // committed on a Prepare response — only Cutover performs the
    // mutating stop+disable+backup that earns the controller slot.
    if matches!(resp, boxpilot_ipc::LegacyMigrateResponse::Cutover(_)) {
        dispatch::commit_controller_claim(&ctx.paths, controller).await?;
    }
    serde_json::to_vec(&resp).map_err(|e| HelperError::Ipc {
        message: format!("encode: {e}"),
    })
}
