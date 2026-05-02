//! Re-export shell. Trust types + functions live in
//! `boxpilot_platform::{traits,linux}::trust`; the FsMetadataProvider /
//! VersionChecker re-exports from PR 3 stay here so existing call sites
//! keep importing from `crate::core::trust::*`.

pub use boxpilot_platform::traits::fs_meta::{FileKind, FileStat, FsMetadataProvider};
pub use boxpilot_platform::traits::trust::{TrustChecker, TrustError};
pub use boxpilot_platform::traits::version::VersionChecker;

#[cfg(target_os = "linux")]
pub use boxpilot_platform::linux::fs_meta::StdFsMetadataProvider;
#[cfg(target_os = "windows")]
pub use boxpilot_platform::windows::fs_meta::StdFsMetadataProvider;

#[cfg(target_os = "linux")]
pub use boxpilot_platform::linux::version::ProcessVersionChecker;
#[cfg(target_os = "windows")]
pub use boxpilot_platform::windows::version::ProcessVersionChecker;

#[cfg(target_os = "linux")]
pub use boxpilot_platform::linux::trust::{
    default_allowed_prefixes, verify_executable_path, LinuxTrustChecker,
};

#[cfg(test)]
pub mod testing {
    pub use boxpilot_platform::fakes::fs_meta::*;
    pub use boxpilot_platform::fakes::trust::*;
}

#[cfg(test)]
pub mod version_testing {
    pub use boxpilot_platform::fakes::version::*;
}
