//! `boxpilotd` — privileged helper for BoxPilot. Activated on the system bus
//! by D-Bus; always runs as root. See spec §6.

mod authority;
mod context;
mod controller;
mod core;
mod credentials;
mod diagnostics;
mod dispatch;
mod dispatch_handler;
mod handlers;
mod iface;
mod legacy;
mod lock;
mod profile;
mod service;
mod systemd;

use anyhow::{Context, Result};
use std::sync::Arc;
use tracing::{error, info, warn};

use crate::core::download::ReqwestDownloader;
use crate::core::github::ReqwestGithubClient;
use crate::core::trust::{ProcessVersionChecker, StdFsMetadataProvider};

use crate::iface::{BUS_NAME, OBJECT_PATH};

/// Spec §7.6: validate `install-state.json`'s `schema_version` against the
/// compiled-in `INSTALL_STATE_SCHEMA_VERSION`. Returns `Some(got)` only on
/// version mismatch — that signal is plumbed into [`context::HelperContext`]
/// and consulted by [`dispatch::authorize`] to refuse mutating verbs until
/// a migration runs. The check intentionally treats *only* schema mismatch
/// as "block writes" — generic IO/JSON errors fall through to a warn-log
/// so a transient read failure does not paint the daemon as unwritable
/// (the next mutating call will surface the real error if it persists).
/// File-missing → `read_state` returns `InstallState::empty()` (schema=1)
/// which matches the constant; this is the fresh-install case and must
/// not block.
async fn check_install_state_schema(paths: &boxpilot_platform::Paths) -> Option<u32> {
    match crate::core::state::read_state(&paths.install_state_json()).await {
        Ok(_) => None,
        Err(boxpilot_ipc::HelperError::UnsupportedSchemaVersion { got }) => {
            error!(
                got,
                expected = boxpilot_ipc::INSTALL_STATE_SCHEMA_VERSION,
                "install-state.json schema mismatch — refusing mutating IPC until migration"
            );
            Some(got)
        }
        Err(e) => {
            warn!("install-state read at startup: {e:?}");
            None
        }
    }
}

async fn run_startup_recovery(paths: &boxpilot_platform::Paths) -> anyhow::Result<()> {
    let staging = paths.cores_staging_dir();
    if staging.exists() {
        match tokio::fs::read_dir(&staging).await {
            Ok(mut entries) => {
                while let Some(e) = entries.next_entry().await? {
                    let p = e.path();
                    let _ = tokio::fs::remove_dir_all(&p).await;
                    info!(path = %p.display(), "swept stale staging dir");
                }
            }
            Err(e) => warn!("read_dir staging: {e}"),
        }
    }

    let current = paths.cores_current_symlink();
    if current.exists() {
        let target = tokio::fs::read_link(&current).await?;
        let resolved = if target.is_absolute() {
            target.clone()
        } else {
            paths.cores_dir().join(&target)
        };
        if !resolved.exists() {
            warn!(target = %resolved.display(), "current symlink target is missing");
        }
    }

    // Upgrade-path backfill: pre-T8 builds claimed the controller without
    // writing the polkit drop-in, so an existing install whose controller
    // was claimed before this version starts up with the drop-in missing.
    // 49-boxpilot.rules then falls through to XML defaults until the next
    // controller transfer rewrites everything. Backfill on startup fixes
    // it without requiring any user action.
    let lookup = controller::PasswdLookup;
    match crate::core::commit::backfill_polkit_dropin(paths, &lookup).await {
        Ok(true) => info!("backfilled polkit controller drop-in"),
        Ok(false) => {} // already present, nothing to backfill, etc.
        Err(e) => warn!("polkit drop-in backfill failed: {e}"),
    }

    // Plan #5 §10 crash recovery: clean stale .staging/ subdirs (always
    // mid-call when present) and validate /etc/boxpilot/active resolves
    // under /etc/boxpilot/releases. Logged here; activation/rollback
    // verbs re-check `active_corrupt` themselves on each call.
    let activation_recovery = crate::profile::recovery::reconcile(paths).await;
    if activation_recovery.staging_dirs_swept > 0 {
        info!(
            count = activation_recovery.staging_dirs_swept,
            "swept stale activation .staging entries"
        );
    }
    if activation_recovery.active_corrupt {
        warn!("/etc/boxpilot/active is corrupt; activation/rollback will refuse until repaired");
    }
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    init_tracing();
    info!(version = env!("CARGO_PKG_VERSION"), "boxpilotd starting");

    if let Err(e) = ensure_running_as_root() {
        error!("refusing to start: {e}");
        std::process::exit(2);
    }

    let paths = boxpilot_platform::Paths::system().context("read system paths from env")?;
    if let Err(e) = run_startup_recovery(&paths).await {
        error!("startup recovery failed: {e}");
    }
    let state_schema_mismatch = check_install_state_schema(&paths).await;

    let conn = zbus::connection::Builder::system()
        .context("connect to system bus")?
        .build()
        .await
        .context("system bus build")?;

    let github =
        Arc::new(ReqwestGithubClient::new().map_err(|e| anyhow::anyhow!("github client: {e}"))?);
    let downloader =
        Arc::new(ReqwestDownloader::new().map_err(|e| anyhow::anyhow!("downloader: {e}"))?);
    let fs_meta = Arc::new(StdFsMetadataProvider);
    let version_checker = Arc::new(ProcessVersionChecker);

    let fragment_reader = Arc::new(crate::legacy::observe::StdFsFragmentReader);
    let config_reader = Arc::new(crate::legacy::migrate::StdConfigReader);
    let journal = Arc::new(crate::systemd::JournalctlProcess);
    let authority_subject = Arc::new(authority::ZbusSubject::new());
    let active = Arc::new(boxpilot_platform::linux::active::SymlinkActivePointer {
        active: paths.active_symlink(),
        releases_dir: paths.releases_dir(),
    });
    let ctx = Arc::new(context::HelperContext::new(
        paths,
        Arc::new(credentials::DBusCallerResolver::new(conn.clone())),
        Arc::new(authority::DBusAuthority::new(
            conn.clone(),
            authority_subject.clone(),
        )),
        authority_subject.clone(),
        Arc::new(systemd::DBusSystemd::new(conn.clone())),
        journal,
        Arc::new(controller::PasswdLookup),
        github,
        downloader,
        fs_meta,
        version_checker,
        Arc::new(crate::profile::checker::ProcessChecker),
        Arc::new(crate::profile::verifier::DefaultVerifier),
        fragment_reader,
        config_reader,
        active,
        state_schema_mismatch,
    ));

    let helper = iface::Helper::new(ctx);
    conn.object_server()
        .at(OBJECT_PATH, helper)
        .await
        .context("register Helper at object path")?;
    conn.request_name(BUS_NAME)
        .await
        .context("acquire bus name")?;
    info!(bus = BUS_NAME, "ready");

    // Block until SIGTERM / SIGINT.
    let mut sigterm = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
    let mut sigint = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt())?;
    tokio::select! {
        _ = sigterm.recv() => info!("SIGTERM received"),
        _ = sigint.recv()  => info!("SIGINT received"),
    }
    info!("shutting down");
    Ok(())
}

fn ensure_running_as_root() -> Result<()> {
    let uid = nix::unistd::Uid::current();
    if !uid.is_root() {
        anyhow::bail!("must run as root (uid 0); current uid is {uid}");
    }
    Ok(())
}

fn init_tracing() {
    use tracing_subscriber::{fmt, EnvFilter};
    let filter = EnvFilter::try_from_env("BOXPILOTD_LOG")
        .unwrap_or_else(|_| EnvFilter::new("boxpilotd=info"));
    fmt().with_env_filter(filter).with_target(false).init();
}
