//! `diagnostics.export_redacted` handler — read-only.

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
            message: "diagnostics.export_redacted takes no aux stream".into(),
        });
    }
    let _call = dispatch::authorize(&ctx, &principal, HelperMethod::DiagnosticsExportRedacted).await?;
    let cfg = ctx.load_config().await?;
    let resp = crate::diagnostics::compose(crate::diagnostics::ComposeInputs {
        paths: &ctx.paths,
        unit_name: &cfg.target_service,
        journal: &*ctx.journal,
        os_release_path: std::path::Path::new("/etc/os-release"),
        now_iso: || chrono::Utc::now().format("%Y-%m-%dT%H-%M-%SZ").to_string(),
    })
    .await?;
    serde_json::to_vec(&resp).map_err(|e| HelperError::Ipc {
        message: format!("encode: {e}"),
    })
}
