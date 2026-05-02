//! Per-platform `boxpilotd` entry point. PR 13 of the platform abstraction
//! plan splits the original `main()` body into Linux + Windows variants:
//!
//! - **Linux**: same wiring as before — connects to the system bus,
//!   registers the helper interface, and blocks on `IpcServer::run` until
//!   SIGTERM/SIGINT.
//! - **Windows**: registers the binary as a Windows Service via
//!   `service_dispatcher::start` and bridges `ServiceControl::Stop` to a
//!   `Notify` that unblocks the named-pipe `IpcServer::run`. Logs go to a
//!   file sink under `%ProgramData%\BoxPilot\logs\` (no equivalent of
//!   journald in Sub-project #1).
//!
//! `main.rs` dispatches to the right `run()` by `cfg(target_os = ...)`.

#[cfg(target_os = "linux")]
pub mod linux;

#[cfg(target_os = "windows")]
pub mod windows;
