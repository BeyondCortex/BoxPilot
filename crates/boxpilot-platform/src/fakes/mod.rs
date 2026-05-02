//! Cross-platform test doubles for every trait. Compile on all targets so
//! helper-side unit tests pass on the Windows runner (AC4).

pub mod authority;
pub mod env;
pub mod fs_meta;
pub mod fs_perms;
pub mod service;
pub mod user_lookup;
pub mod version;
