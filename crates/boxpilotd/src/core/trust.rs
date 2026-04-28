//! §6.5 trust checks. Used before promoting any binary to be invoked by
//! the privileged daemon (downloaded sing-box, adopted external binaries).
#![allow(dead_code)] // scaffolding-only: tasks 5-7 add callers and remove this

use std::io;
use std::path::{Path, PathBuf};
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileStat {
    pub uid: u32,
    pub gid: u32,
    /// Lowest 12 bits of st_mode (permission + special bits).
    pub mode: u32,
    pub kind: FileKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileKind {
    Regular,
    Directory,
    Symlink,
    Other,
}

pub trait FsMetadataProvider: Send + Sync {
    fn stat(&self, path: &Path) -> io::Result<FileStat>;
    fn read_link(&self, path: &Path) -> io::Result<PathBuf>;
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum TrustError {
    #[error("file does not exist: {0}")]
    NotFound(PathBuf),
    #[error("not a regular file: {0}")]
    NotRegular(PathBuf),
    #[error("not owned by root (uid={uid}, gid={gid}): {path}")]
    NotRootOwned { path: PathBuf, uid: u32, gid: u32 },
    #[error("group/world writable: {path} (mode={mode:o})")]
    Writable { path: PathBuf, mode: u32 },
    #[error("setuid/setgid/sticky bit set: {path} (mode={mode:o})")]
    SpecialBits { path: PathBuf, mode: u32 },
    #[error("path outside allowed prefixes: {0}")]
    DisallowedPrefix(PathBuf),
    #[error("symlink resolution failed: {0}")]
    SymlinkResolution(String),
    #[error("sing-box version self-check failed: {0}")]
    VersionCheckFailed(String),
}

#[cfg(test)]
pub mod testing {
    use super::*;
    use std::collections::HashMap;
    use std::sync::Mutex;

    #[derive(Default)]
    #[allow(dead_code)]
    pub struct FakeFs {
        pub stats: Mutex<HashMap<PathBuf, FileStat>>,
        pub links: Mutex<HashMap<PathBuf, PathBuf>>,
    }

    impl FakeFs {
        #[allow(dead_code)]
        pub fn root_dir() -> FileStat {
            FileStat {
                uid: 0,
                gid: 0,
                mode: 0o755,
                kind: FileKind::Directory,
            }
        }
        #[allow(dead_code)]
        pub fn root_bin() -> FileStat {
            FileStat {
                uid: 0,
                gid: 0,
                mode: 0o755,
                kind: FileKind::Regular,
            }
        }
        #[allow(dead_code)]
        pub fn put(&self, path: impl AsRef<Path>, stat: FileStat) {
            self.stats
                .lock()
                .unwrap()
                .insert(path.as_ref().to_path_buf(), stat);
        }
    }

    impl FsMetadataProvider for FakeFs {
        fn stat(&self, path: &Path) -> io::Result<FileStat> {
            self.stats
                .lock()
                .unwrap()
                .get(path)
                .cloned()
                .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, path.display().to_string()))
        }
        fn read_link(&self, path: &Path) -> io::Result<PathBuf> {
            self.links
                .lock()
                .unwrap()
                .get(path)
                .cloned()
                .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "not a symlink"))
        }
    }
}

const SPECIAL_BITS_MASK: u32 = 0o7000;
const GROUP_WORLD_WRITE: u32 = 0o022;

/// Apply the §6.5 binary-level checks to `path`'s stat result.
pub(crate) fn check_binary_stat(path: &Path, stat: &FileStat) -> Result<(), TrustError> {
    if !matches!(stat.kind, FileKind::Regular) {
        return Err(TrustError::NotRegular(path.to_path_buf()));
    }
    if stat.uid != 0 || stat.gid != 0 {
        return Err(TrustError::NotRootOwned {
            path: path.to_path_buf(),
            uid: stat.uid,
            gid: stat.gid,
        });
    }
    if stat.mode & GROUP_WORLD_WRITE != 0 {
        return Err(TrustError::Writable {
            path: path.to_path_buf(),
            mode: stat.mode,
        });
    }
    if stat.mode & SPECIAL_BITS_MASK != 0 {
        return Err(TrustError::SpecialBits {
            path: path.to_path_buf(),
            mode: stat.mode,
        });
    }
    Ok(())
}

/// Apply the §6.5 directory-level checks (used for parent walks).
pub(crate) fn check_dir_stat(path: &Path, stat: &FileStat) -> Result<(), TrustError> {
    if !matches!(stat.kind, FileKind::Directory) {
        return Err(TrustError::SymlinkResolution(format!(
            "{path:?} is not a directory"
        )));
    }
    if stat.uid != 0 {
        return Err(TrustError::NotRootOwned {
            path: path.to_path_buf(),
            uid: stat.uid,
            gid: stat.gid,
        });
    }
    if stat.mode & GROUP_WORLD_WRITE != 0 {
        return Err(TrustError::Writable {
            path: path.to_path_buf(),
            mode: stat.mode,
        });
    }
    Ok(())
}

#[cfg(test)]
mod binary_check_tests {
    use super::testing::FakeFs;
    use super::*;

    #[test]
    fn rejects_non_root_uid() {
        let r = check_binary_stat(
            Path::new("/x"),
            &FileStat {
                uid: 1000,
                gid: 0,
                mode: 0o755,
                kind: FileKind::Regular,
            },
        );
        assert!(matches!(r, Err(TrustError::NotRootOwned { uid: 1000, .. })));
    }

    #[test]
    fn rejects_group_writable() {
        let r = check_binary_stat(
            Path::new("/x"),
            &FileStat {
                uid: 0,
                gid: 0,
                mode: 0o775,
                kind: FileKind::Regular,
            },
        );
        assert!(matches!(r, Err(TrustError::Writable { mode: 0o775, .. })));
    }

    #[test]
    fn rejects_world_writable() {
        let r = check_binary_stat(
            Path::new("/x"),
            &FileStat {
                uid: 0,
                gid: 0,
                mode: 0o757,
                kind: FileKind::Regular,
            },
        );
        assert!(matches!(r, Err(TrustError::Writable { mode: 0o757, .. })));
    }

    #[test]
    fn rejects_setuid() {
        let r = check_binary_stat(
            Path::new("/x"),
            &FileStat {
                uid: 0,
                gid: 0,
                mode: 0o4755,
                kind: FileKind::Regular,
            },
        );
        assert!(matches!(r, Err(TrustError::SpecialBits { .. })));
    }

    #[test]
    fn rejects_setgid() {
        let r = check_binary_stat(
            Path::new("/x"),
            &FileStat {
                uid: 0,
                gid: 0,
                mode: 0o2755,
                kind: FileKind::Regular,
            },
        );
        assert!(matches!(r, Err(TrustError::SpecialBits { .. })));
    }

    #[test]
    fn rejects_sticky() {
        let r = check_binary_stat(
            Path::new("/x"),
            &FileStat {
                uid: 0,
                gid: 0,
                mode: 0o1755,
                kind: FileKind::Regular,
            },
        );
        assert!(matches!(r, Err(TrustError::SpecialBits { .. })));
    }

    #[test]
    fn rejects_directory_as_binary() {
        let r = check_binary_stat(
            Path::new("/x"),
            &FileStat {
                uid: 0,
                gid: 0,
                mode: 0o755,
                kind: FileKind::Directory,
            },
        );
        assert!(matches!(r, Err(TrustError::NotRegular(_))));
    }

    #[test]
    fn happy_path_accepts_root_owned_0755() {
        let r = check_binary_stat(
            Path::new("/x"),
            &FileStat {
                uid: 0,
                gid: 0,
                mode: 0o755,
                kind: FileKind::Regular,
            },
        );
        assert!(r.is_ok());
    }

    #[test]
    fn dir_check_rejects_group_writable_parent() {
        let r = check_dir_stat(
            Path::new("/x"),
            &FileStat {
                uid: 0,
                gid: 0,
                mode: 0o775,
                kind: FileKind::Directory,
            },
        );
        assert!(matches!(r, Err(TrustError::Writable { .. })));
    }

    #[test]
    fn _suppress_unused_warning_fakefs() {
        let _ = FakeFs::root_bin();
    }
}
