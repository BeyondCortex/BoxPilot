//! `home.status` handler — bundles service status, active profile, core
//! snapshot, and active-symlink corruption flag for the GUI's first paint.

use crate::context::HelperContext;
use crate::dispatch;
use boxpilot_ipc::{HelperError, HelperMethod, HelperResult, ServiceStatusResponse};
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
            message: "home.status takes no aux stream".into(),
        });
    }
    let call = dispatch::authorize(&ctx, &principal, HelperMethod::HomeStatus).await?;

    let cfg = ctx.load_config().await?;
    let unit_name = cfg.target_service.clone();
    let unit_state = ctx.systemd.unit_state(&unit_name).await?;
    let controller = call.controller.to_status();
    let service = ServiceStatusResponse {
        unit_name,
        unit_state,
        controller,
        state_schema_mismatch: ctx.state_schema_mismatch,
    };

    let active_profile = match (
        cfg.active_profile_id.as_ref(),
        cfg.active_profile_sha256.as_ref(),
        cfg.active_release_id.as_ref(),
        cfg.activated_at.as_ref(),
    ) {
        (Some(id), Some(sha), Some(rel), Some(at)) => Some(boxpilot_ipc::ActiveProfileSnapshot {
            profile_id: id.clone(),
            profile_name: cfg.active_profile_name.clone(),
            profile_sha256: sha.clone(),
            release_id: rel.clone(),
            activated_at: at.clone(),
        }),
        _ => None,
    };

    #[cfg(target_os = "linux")]
    let core_version = match discover(&ctx).await {
        Ok(list) => cfg
            .core_path
            .as_deref()
            .and_then(|p| list.cores.iter().find(|c| c.path == p))
            .map(|c| c.version.clone())
            .unwrap_or_else(|| "unknown".to_string()),
        Err(_) => "unknown".to_string(),
    };
    #[cfg(not(target_os = "linux"))]
    let core_version = "unknown".to_string();
    let core = boxpilot_ipc::CoreSnapshot {
        path: cfg.core_path.clone(),
        state: cfg.core_state,
        version: core_version,
    };

    let active_corrupt = crate::profile::recovery::check_active_status(&ctx.paths)
        .await
        .corrupt;

    let resp = boxpilot_ipc::HomeStatusResponse {
        schema_version: boxpilot_ipc::HOME_STATUS_SCHEMA_VERSION,
        service,
        active_profile,
        core,
        active_corrupt,
    };
    serde_json::to_vec(&resp).map_err(|e| HelperError::Ipc {
        message: format!("encode: {e}"),
    })
}

/// Inline equivalent of the former `Helper::discover_for_home` — runs the
/// core discovery sub-pipeline so home.status can fold the active core's
/// version into its snapshot. Discovery failure is non-fatal at the call
/// site; the rest of the page still renders. Linux-only because
/// `core::discover` uses Linux trust APIs.
#[cfg(target_os = "linux")]
async fn discover(ctx: &HelperContext) -> Result<boxpilot_ipc::CoreDiscoverResponse, HelperError> {
    let deps = crate::core::discover::DiscoverDeps {
        paths: ctx.paths.clone(),
        fs: &*ctx.fs_meta,
        version_checker: &*ctx.version_checker,
    };
    crate::core::discover::discover(&deps).await
}
