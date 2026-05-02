//! Cross-platform test doubles for every trait. Compile on all targets so
//! helper-side unit tests pass on the Windows runner (AC4).

pub mod active;
pub mod authority;
pub mod bundle_aux;
pub mod core_assets;
pub mod env;
pub mod fs_meta;
pub mod fs_perms;
pub mod lock;
pub mod logs;
pub mod service;
pub mod trust;
pub mod user_lookup;
pub mod version;
