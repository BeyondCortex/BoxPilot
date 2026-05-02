//! `boxpilotd` — privileged helper for BoxPilot. PR 13 splits the entry
//! body into `entry::linux::run` (system-bus / zbus) and
//! `entry::windows::run` (SCM / named pipe). This file just dispatches
//! by `target_os`. See `entry/mod.rs` for the rationale.

mod authority;
mod context;
mod controller;
mod core;
mod credentials;
mod diagnostics;
mod dispatch;
mod dispatch_handler;
mod entry;
mod handlers;
#[cfg(target_os = "linux")]
mod iface;
mod legacy;
mod lock;
mod profile;
mod service;
mod systemd;


#[cfg(target_os = "linux")]
fn main() -> anyhow::Result<()> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    runtime.block_on(entry::linux::run())
}

#[cfg(target_os = "windows")]
fn main() -> anyhow::Result<()> {
    entry::windows::run()
}

#[cfg(not(any(target_os = "linux", target_os = "windows")))]
fn main() -> anyhow::Result<()> {
    Err(anyhow::anyhow!("boxpilotd: unsupported platform"))
}
