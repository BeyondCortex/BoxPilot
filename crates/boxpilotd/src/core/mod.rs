//! Managed sing-box core lifecycle (spec §11). Each submodule is small
//! and isolated behind trait seams so the entire layer can be unit-tested
//! without root, network, or systemd.

#[cfg(target_os = "linux")]
pub mod adopt;
pub mod commit;
#[cfg(target_os = "linux")]
pub mod discover;
pub mod download;
pub mod github;
#[cfg(target_os = "linux")]
pub mod install;
#[cfg(target_os = "linux")]
pub mod rollback;
pub mod state;
pub mod trust;
