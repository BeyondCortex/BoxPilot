//! `boxpilotd` — privileged helper for BoxPilot. Activated on the system bus
//! by D-Bus; always runs as root. See spec §6.

mod authority;
mod context;
mod controller;
mod core;
mod credentials;
mod dispatch;
mod iface;
mod lock;
mod paths;
mod profile;
mod service;
mod systemd;

use anyhow::{Context, Result};
use std::sync::Arc;
use tracing::{error, info, warn};

use crate::core::download::ReqwestDownloader;
use crate::core::github::ReqwestGithubClient;
use crate::core::trust::{ProcessVersionChecker, StdFsMetadataProvider};

const BUS_NAME: &str = "app.boxpilot.Helper";
const OBJECT_PATH: &str = "/app/boxpilot/Helper";

async fn run_startup_recovery(paths: &paths::Paths) -> anyhow::Result<()> {
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

    let paths = paths::Paths::system();
    if let Err(e) = run_startup_recovery(&paths).await {
        error!("startup recovery failed: {e}");
    }

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

    let journal = Arc::new(crate::systemd::JournalctlProcess);
    let ctx = Arc::new(context::HelperContext::new(
        paths,
        Arc::new(credentials::DBusCallerResolver::new(conn.clone())),
        Arc::new(authority::DBusAuthority::new(conn.clone())),
        Arc::new(systemd::DBusSystemd::new(conn.clone())),
        journal,
        Arc::new(controller::PasswdLookup),
        github,
        downloader,
        fs_meta,
        version_checker,
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
