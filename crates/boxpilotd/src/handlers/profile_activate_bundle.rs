//! `profile.activate_bundle` handler — mutating; consumes a bundle aux
//! stream and may claim controller. The only verb whose AuxShape is
//! `Required`.

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
    if aux.is_none() {
        return Err(HelperError::Ipc {
            message: "profile.activate_bundle requires an aux stream".into(),
        });
    }
    let req: boxpilot_ipc::ActivateBundleRequest =
        serde_json::from_slice(&body).map_err(|e| HelperError::Ipc {
            message: format!("decode: {e}"),
        })?;
    let call = dispatch::authorize(&ctx, &principal, HelperMethod::ProfileActivateBundle).await?;
    let controller = dispatch::maybe_claim_controller(
        call.will_claim_controller,
        &call.principal,
        &*ctx.user_lookup,
    )?;
    // The activate pipeline still takes a raw OwnedFd; on Linux every
    // production AuxStream for this verb is fd-backed (memfd from the
    // GUI). Convert back to a raw fd here so the existing
    // `activate_bundle` signature stays untouched. Once the activate
    // pipeline accepts AuxStream end-to-end (follow-up), this conversion
    // can drop.
    #[cfg(target_os = "linux")]
    let fd = aux.into_owned_fd().ok_or_else(|| HelperError::Ipc {
        message: "profile.activate_bundle: aux stream is not fd-backed".into(),
    })?;
    #[cfg(not(target_os = "linux"))]
    {
        let _ = aux;
        return Err(HelperError::NotImplemented);
    }
    #[cfg(target_os = "linux")]
    {
        let deps = crate::profile::activate::ActivateDeps {
            paths: ctx.paths.clone(),
            systemd: &*ctx.systemd,
            verifier: &*ctx.verifier,
            checker: &*ctx.checker,
        };
        let resp = crate::profile::activate::activate_bundle(req, fd, controller, &deps).await?;
        serde_json::to_vec(&resp).map_err(|e| HelperError::Ipc {
            message: format!("encode: {e}"),
        })
    }
}
