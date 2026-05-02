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
    let _call = dispatch::authorize(&ctx, &principal, HelperMethod::LegacyMigrateService).await?;
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
    serde_json::to_vec(&resp).map_err(|e| HelperError::Ipc {
        message: format!("encode: {e}"),
    })
}
