//! `boxpilotd` — privileged helper for BoxPilot. Activated on the system bus
//! by D-Bus; always runs as root. See spec §6.

mod authority;
mod context;
mod controller;
mod credentials;
mod dispatch;
mod iface;
mod lock;
mod paths;
mod systemd;

use anyhow::{Context, Result};
use std::sync::Arc;
use tracing::{error, info};

const BUS_NAME: &str = "app.boxpilot.Helper";
const OBJECT_PATH: &str = "/app/boxpilot/Helper";

#[tokio::main]
async fn main() -> Result<()> {
    init_tracing();
    info!(version = env!("CARGO_PKG_VERSION"), "boxpilotd starting");

    if let Err(e) = ensure_running_as_root() {
        error!("refusing to start: {e}");
        std::process::exit(2);
    }

    let conn = zbus::connection::Builder::system()
        .context("connect to system bus")?
        .build()
        .await
        .context("system bus build")?;

    let ctx = Arc::new(context::HelperContext::new(
        paths::Paths::system(),
        Arc::new(credentials::DBusCallerResolver::new(conn.clone())),
        Arc::new(authority::DBusAuthority::new(conn.clone())),
        Arc::new(systemd::DBusSystemd::new(conn.clone())),
        Arc::new(controller::PasswdLookup),
    ));

    let helper = iface::Helper::new(ctx);
    conn.object_server()
        .at(OBJECT_PATH, helper)
        .await
        .context("register Helper at object path")?;
    conn.request_name(BUS_NAME).await.context("acquire bus name")?;
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
