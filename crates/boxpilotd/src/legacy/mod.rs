//! Spec §8 — existing-`sing-box.service` handling.
//!
//! All submodules depend on Linux systemd / D-Bus APIs. They are
//! cfg-gated; Windows batch ④ will provide real impls.

#[cfg(target_os = "linux")]
pub mod backup;
#[cfg(target_os = "linux")]
pub mod migrate;
#[cfg(target_os = "linux")]
pub mod observe;
#[cfg(target_os = "linux")]
pub mod path_safety;
#[cfg(target_os = "linux")]
pub mod unit_parser;
