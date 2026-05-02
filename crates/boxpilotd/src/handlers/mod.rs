//! Per-verb HelperDispatch handlers. Each module exports a uniform
//! `handle(ctx, principal, body, aux) -> HelperResult<Vec<u8>>` function.
//! The body extraction here decouples each verb from the IPC transport so
//! the cross-platform `HelperDispatch` impl (`dispatch_handler.rs`) can
//! route by `HelperMethod` without knowing about D-Bus or named pipes.
//!
//! The caller (`dispatch_handler` or the legacy `iface.rs` thin shell) is
//! responsible for:
//!   1. resolving the `CallerPrincipal` from the IPC connection
//!   2. setting `ctx.authority_subject` (Linux: D-Bus sender; Windows: SID)
//!   3. supplying `body` as JSON bytes (empty for nullary verbs)
//!   4. supplying `aux` (AuxStream::none() for non-bundle verbs)
//!
//! The handler then calls `dispatch::authorize` and runs the action body,
//! returning the typed response serialized to JSON bytes.
//!
//! Cross-platform handlers: `controller_transfer`, `diagnostics_export_redacted`,
//! `service_status`, `home_status` (partial — core discover arm Linux-only).
//! Linux-only handlers: all service control/install/logs, all core/profile/legacy
//! verbs. Windows batch ③/④ will replace the Linux-only stubs.

pub mod controller_transfer;
#[cfg(target_os = "linux")]
pub mod core_adopt;
#[cfg(target_os = "linux")]
pub mod core_discover;
#[cfg(target_os = "linux")]
pub mod core_install_managed;
#[cfg(target_os = "linux")]
pub mod core_rollback_managed;
#[cfg(target_os = "linux")]
pub mod core_upgrade_managed;
pub mod diagnostics_export_redacted;
pub mod home_status;
#[cfg(target_os = "linux")]
pub mod legacy_migrate_service;
#[cfg(target_os = "linux")]
pub mod legacy_observe_service;
#[cfg(target_os = "linux")]
pub mod profile_activate_bundle;
#[cfg(target_os = "linux")]
pub mod profile_rollback_release;
#[cfg(target_os = "linux")]
pub mod service_disable;
#[cfg(target_os = "linux")]
pub mod service_enable;
#[cfg(target_os = "linux")]
pub mod service_install_managed;
#[cfg(target_os = "linux")]
pub mod service_logs;
#[cfg(target_os = "linux")]
pub mod service_restart;
#[cfg(target_os = "linux")]
pub mod service_start;
pub mod service_status;
#[cfg(target_os = "linux")]
pub mod service_stop;
