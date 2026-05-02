//! `service.install_managed` handler — mutating; may claim controller.

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
            message: "service.install_managed takes no aux stream".into(),
        });
    }
    let call = dispatch::authorize(&ctx, &principal, HelperMethod::ServiceInstallManaged).await?;
    let controller = dispatch::maybe_claim_controller(
        call.will_claim_controller,
        &call.principal,
        &*ctx.user_lookup,
    )?;
    let cfg = ctx.load_config().await?;
    let deps = crate::service::install::InstallDeps {
        paths: ctx.paths.clone(),
        systemd: &*ctx.systemd,
        fs: &*ctx.fs_meta,
    };
    let mut resp = crate::service::install::install_managed(&cfg, &deps).await?;
    resp.claimed_controller = controller.is_some();

    // Persist controller-name + boxpilot.toml + polkit drop-in if claiming.
    // T8 made StateCommit also write the polkit drop-in atomically with
    // controller-name. The install_state passed here is empty because
    // service.install_managed does not change the cores ledger; the guard
    // inside StateCommit::apply preserves any on-disk install-state.json.
    //
    // Atomicity note: the unit file + daemon-reload above are NOT atomic
    // with the StateCommit below. If the commit fails, the unit lives on
    // disk but the controller claim is not recorded; the next attempt
    // rewrites both, so the corner is benign rather than blocking.
    if let Some(c) = controller {
        let commit = crate::core::commit::StateCommit {
            paths: ctx.paths.clone(),
            toml_updates: crate::core::commit::TomlUpdates::default(),
            controller: Some(c),
            install_state: boxpilot_ipc::InstallState::empty(),
            current_core_update: None,
        };
        commit.apply().await?;
    }

    serde_json::to_vec(&resp).map_err(|e| HelperError::Ipc {
        message: format!("encode: {e}"),
    })
}
