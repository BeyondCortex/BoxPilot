//! Windows implementations of the traits in `crate::traits`. Most are
//! `unimplemented!()` stubs in Sub-project #1 (per
//! `docs/superpowers/specs/2026-05-01-boxpilot-platform-abstraction-design.md`
//! §5 trait inventory).

#![cfg(target_os = "windows")]

pub mod env;
pub mod fs_meta;
pub mod fs_perms;
pub mod user_lookup;
pub mod version;
