//! Platform-neutral trait interfaces. Implementations live in `linux/`,
//! `windows/`, and `fakes/`. Traits arrive in later PRs:
//!
//! - PR 2: `EnvProvider`
//! - PR 3: `FsMetadataProvider`, `VersionChecker`, `UserLookup`, `FsPermissions`
//! - PR 4: `Authority`
//! - PR 5: `ServiceManager`, `LogReader`
//! - PR 6: `FileLock`
//! - PR 7: `TrustChecker`
//! - PR 8: `ActivePointer`
//! - PR 9: `CoreAssetNaming`, `CoreArchive`
//! - PR 10: `AuxStream` (struct, not trait)
//! - PR 11a: `IpcServer`, `IpcConnection`, `IpcClient`, `HelperDispatch`

pub mod env;
pub mod fs_meta;
pub mod fs_perms;
pub mod user_lookup;
pub mod version;
