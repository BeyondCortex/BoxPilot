//! Managed sing-box core lifecycle (spec §11). Each submodule is small
//! and isolated behind trait seams so the entire layer can be unit-tested
//! without root, network, or systemd.

pub mod github;
pub mod state;
pub mod trust;
