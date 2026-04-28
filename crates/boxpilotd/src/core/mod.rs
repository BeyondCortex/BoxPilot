//! Managed sing-box core lifecycle (spec §11). Each submodule is small
//! and isolated behind trait seams so the entire layer can be unit-tested
//! without root, network, or systemd.

pub mod adopt;
pub mod commit;
pub mod discover;
pub mod download;
pub mod github;
pub mod install;
pub mod rollback;
pub mod state;
pub mod trust;
