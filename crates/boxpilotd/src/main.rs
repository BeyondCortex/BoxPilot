//! `boxpilotd` — privileged helper for BoxPilot. Activated on the system bus
//! by D-Bus; always runs as root. See spec §6.

mod paths;

use anyhow::Result;
use tracing::info;

#[tokio::main]
async fn main() -> Result<()> {
    init_tracing();
    info!(version = env!("CARGO_PKG_VERSION"), "boxpilotd starting");
    // Real D-Bus / signal-handling wiring lands in task 18.
    Ok(())
}

fn init_tracing() {
    use tracing_subscriber::{fmt, EnvFilter};
    let filter = EnvFilter::try_from_env("BOXPILOTD_LOG")
        .unwrap_or_else(|_| EnvFilter::new("boxpilotd=info"));
    fmt().with_env_filter(filter).with_target(false).init();
}
