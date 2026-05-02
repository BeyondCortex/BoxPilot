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

pub mod controller_transfer;
pub mod core_adopt;
pub mod core_discover;
pub mod core_install_managed;
pub mod core_rollback_managed;
pub mod core_upgrade_managed;
pub mod diagnostics_export_redacted;
pub mod home_status;
pub mod legacy_migrate_service;
pub mod legacy_observe_service;
pub mod profile_activate_bundle;
pub mod profile_rollback_release;
pub mod service_disable;
pub mod service_enable;
pub mod service_install_managed;
pub mod service_logs;
pub mod service_restart;
pub mod service_start;
pub mod service_status;
pub mod service_stop;
