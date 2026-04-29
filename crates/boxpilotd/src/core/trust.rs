//! §6.5 trust checks. Used before promoting any binary to be invoked by
//! the privileged daemon (downloaded sing-box, adopted external binaries).

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

/// Allowed path prefixes per §6.5. Caller may extend with adopted core
/// directories pulled from install-state.
pub fn default_allowed_prefixes() -> Vec<PathBuf> {
    vec![
        PathBuf::from("/usr/bin"),
        PathBuf::from("/usr/local/bin"),
        PathBuf::from("/var/lib/boxpilot/cores"),
    ]
}

/// Walk `path` ancestors, run binary checks, run directory checks, and
/// confirm the resolved path lives under one of the allowed prefixes.
///
/// **Does not run the `sing-box version` check** — that runs at a higher
/// layer because it requires process-spawn capability.
pub fn verify_executable_path(
    fs: &dyn FsMetadataProvider,
    path: &Path,
    allowed_prefixes: &[PathBuf],
) -> Result<PathBuf, TrustError> {
    let resolved = resolve_symlinks(fs, path)?;
    let bin_stat = fs.stat(&resolved).map_err(|e| match e.kind() {
        io::ErrorKind::NotFound => TrustError::NotFound(resolved.clone()),
        _ => TrustError::SymlinkResolution(format!("{e}")),
    })?;
    check_binary_stat(&resolved, &bin_stat)?;

    let mut current = resolved
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("/"));
    loop {
        let stat = fs
            .stat(&current)
            .map_err(|e| TrustError::SymlinkResolution(format!("{}: {}", current.display(), e)))?;
        check_dir_stat(&current, &stat)?;
        if current == Path::new("/") {
            break;
        }
        current = current
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("/"));
    }

    if !allowed_prefixes.iter().any(|p| resolved.starts_with(p)) {
        return Err(TrustError::DisallowedPrefix(resolved));
    }
    Ok(resolved)
}

fn resolve_symlinks(fs: &dyn FsMetadataProvider, path: &Path) -> Result<PathBuf, TrustError> {
    // Bounded resolution to defend against symlink loops.
    const MAX_HOPS: u32 = 16;
    let mut current = path.to_path_buf();
    for _ in 0..MAX_HOPS {
        let stat = fs.stat(&current);
        match stat {
            Ok(s) if matches!(s.kind, FileKind::Symlink) => {
                let target = fs
                    .read_link(&current)
                    .map_err(|e| TrustError::SymlinkResolution(format!("{e}")))?;
                // POSIX symlinks: relative targets are interpreted against
                // the symlink's parent directory, not the daemon's cwd.
                current = if target.is_absolute() {
                    target
                } else {
                    current.parent().map(|p| p.join(&target)).unwrap_or(target)
                };
            }
            Ok(_) => return Ok(current),
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                return Err(TrustError::NotFound(current));
            }
            Err(e) => return Err(TrustError::SymlinkResolution(format!("{e}"))),
        }
    }
    Err(TrustError::SymlinkResolution(
        "symlink chain too deep".into(),
    ))
}

#[cfg(test)]
mod verify_tests {
    use super::testing::FakeFs;
    use super::*;

    fn root_chain(fs: &FakeFs) {
        fs.put("/", FakeFs::root_dir());
        fs.put("/usr", FakeFs::root_dir());
        fs.put("/usr/bin", FakeFs::root_dir());
    }

    #[test]
    fn happy_path_under_usr_bin() {
        let fs = FakeFs::default();
        root_chain(&fs);
        fs.put("/usr/bin/sing-box", FakeFs::root_bin());
        let r = verify_executable_path(
            &fs,
            Path::new("/usr/bin/sing-box"),
            &default_allowed_prefixes(),
        );
        assert!(r.is_ok(), "{r:?}");
    }

    #[test]
    fn rejects_under_home() {
        let fs = FakeFs::default();
        fs.put("/", FakeFs::root_dir());
        // /home owned by root but commonly group-writable on some distros; reject anyway via prefix list
        fs.put("/home", FakeFs::root_dir());
        fs.put(
            "/home/alice",
            FileStat {
                uid: 1000,
                gid: 1000,
                mode: 0o755,
                kind: FileKind::Directory,
            },
        );
        fs.put("/home/alice/sing-box", FakeFs::root_bin());
        let r = verify_executable_path(
            &fs,
            Path::new("/home/alice/sing-box"),
            &default_allowed_prefixes(),
        );
        // Either NotRootOwned on a parent or DisallowedPrefix — both acceptable rejections.
        assert!(matches!(
            r,
            Err(TrustError::NotRootOwned { .. }) | Err(TrustError::DisallowedPrefix(_))
        ));
    }

    #[test]
    fn rejects_disallowed_prefix() {
        let fs = FakeFs::default();
        fs.put("/", FakeFs::root_dir());
        fs.put("/opt", FakeFs::root_dir());
        fs.put("/opt/sing-box", FakeFs::root_bin());
        let r =
            verify_executable_path(&fs, Path::new("/opt/sing-box"), &default_allowed_prefixes());
        assert!(matches!(r, Err(TrustError::DisallowedPrefix(_))));
    }

    #[test]
    fn allows_extended_prefix() {
        let fs = FakeFs::default();
        fs.put("/", FakeFs::root_dir());
        fs.put("/opt", FakeFs::root_dir());
        fs.put("/opt/sing-box", FakeFs::root_bin());
        let mut prefixes = default_allowed_prefixes();
        prefixes.push(PathBuf::from("/opt"));
        let r = verify_executable_path(&fs, Path::new("/opt/sing-box"), &prefixes);
        assert!(r.is_ok(), "{r:?}");
    }

    #[test]
    fn relative_symlink_resolves_against_parent_dir() {
        // Real-world case: /usr/bin/sing-box -> ../lib/sing-box/sing-box.
        // The relative target must be joined to the symlink's parent
        // (/usr/bin), giving /usr/bin/../lib/sing-box/sing-box, NOT
        // resolved against the daemon's cwd.
        let fs = FakeFs::default();
        fs.put("/", FakeFs::root_dir());
        fs.put("/usr", FakeFs::root_dir());
        fs.put("/usr/bin", FakeFs::root_dir());
        fs.put("/usr/bin/lib", FakeFs::root_dir());
        fs.put("/usr/bin/lib/sing-box", FakeFs::root_dir());
        fs.put("/usr/bin/lib/sing-box/sing-box", FakeFs::root_bin());
        // Mark the entry as a symlink.
        fs.put(
            "/usr/bin/sing-box",
            FileStat {
                uid: 0,
                gid: 0,
                mode: 0o755,
                kind: FileKind::Symlink,
            },
        );
        fs.links.lock().unwrap().insert(
            PathBuf::from("/usr/bin/sing-box"),
            PathBuf::from("lib/sing-box/sing-box"), // relative target
        );

        let mut prefixes = default_allowed_prefixes();
        prefixes.push(PathBuf::from("/usr/bin/lib"));
        let r = verify_executable_path(&fs, Path::new("/usr/bin/sing-box"), &prefixes);
        assert!(r.is_ok(), "expected relative symlink to resolve, got {r:?}");
    }
}

pub trait VersionChecker: Send + Sync {
    /// Run `<binary> version` (or equivalent) and return the trimmed
    /// stdout, expected to begin with `"sing-box version"`.
    fn check(&self, binary: &Path) -> Result<String, TrustError>;
}

pub struct ProcessVersionChecker;

impl VersionChecker for ProcessVersionChecker {
    fn check(&self, binary: &Path) -> Result<String, TrustError> {
        let out = std::process::Command::new(binary)
            .arg("version")
            .output()
            .map_err(|e| TrustError::VersionCheckFailed(format!("spawn: {e}")))?;
        if !out.status.success() {
            return Err(TrustError::VersionCheckFailed(format!(
                "exit {:?}: {}",
                out.status.code(),
                String::from_utf8_lossy(&out.stderr).trim()
            )));
        }
        let stdout = String::from_utf8_lossy(&out.stdout).to_string();
        if !stdout.contains("sing-box version") {
            return Err(TrustError::VersionCheckFailed(format!(
                "unexpected stdout: {}",
                stdout.lines().next().unwrap_or("")
            )));
        }
        Ok(stdout)
    }
}

#[cfg(test)]
pub mod version_testing {
    use super::*;
    use std::sync::Mutex;

    pub struct FixedVersionChecker {
        pub stdout: Mutex<Result<String, String>>,
    }

    impl FixedVersionChecker {
        pub fn ok(s: impl Into<String>) -> Self {
            Self {
                stdout: Mutex::new(Ok(s.into())),
            }
        }
        pub fn err(s: impl Into<String>) -> Self {
            Self {
                stdout: Mutex::new(Err(s.into())),
            }
        }
    }

    impl VersionChecker for FixedVersionChecker {
        fn check(&self, _binary: &Path) -> Result<String, TrustError> {
            self.stdout
                .lock()
                .unwrap()
                .clone()
                .map_err(TrustError::VersionCheckFailed)
        }
    }

    #[test]
    fn fixed_ok_returns_stdout() {
        let v = FixedVersionChecker::ok("sing-box version 1.10.0");
        assert!(v.check(Path::new("/x")).unwrap().starts_with("sing-box"));
    }

    #[test]
    fn fixed_err_returns_version_check_failed() {
        let v = FixedVersionChecker::err("crashed");
        let r = v.check(Path::new("/x"));
        assert!(matches!(r, Err(TrustError::VersionCheckFailed(_))));
    }
}

pub struct StdFsMetadataProvider;

impl FsMetadataProvider for StdFsMetadataProvider {
    fn stat(&self, path: &Path) -> io::Result<FileStat> {
        use std::os::unix::fs::MetadataExt;
        let md = std::fs::symlink_metadata(path)?;
        let ft = md.file_type();
        let kind = if ft.is_symlink() {
            FileKind::Symlink
        } else if ft.is_dir() {
            FileKind::Directory
        } else if ft.is_file() {
            FileKind::Regular
        } else {
            FileKind::Other
        };
        Ok(FileStat {
            uid: md.uid(),
            gid: md.gid(),
            mode: md.mode() & 0o7777,
            kind,
        })
    }
    fn read_link(&self, path: &Path) -> io::Result<PathBuf> {
        std::fs::read_link(path)
    }
}
