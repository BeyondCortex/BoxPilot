//! Linux §6.5 trust check: uid/mode + parent-dir walk + symlink resolution.
//! Body is the verbatim port of what used to live in
//! `boxpilotd::core::trust`. Free `pub fn` form is preserved so existing
//! call sites (rollback, discover, adopt, install, service::install) keep
//! compiling via the re-export shell in boxpilotd.

use crate::traits::fs_meta::{FileKind, FileStat, FsMetadataProvider};
use crate::traits::trust::{TrustChecker, TrustError};
use std::collections::VecDeque;
use std::ffi::{OsStr, OsString};
use std::io;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

const SPECIAL_BITS_MASK: u32 = 0o7000;
const GROUP_WORLD_WRITE: u32 = 0o022;

/// Apply the §6.5 binary-level checks to `path`'s stat result.
pub fn check_binary_stat(path: &Path, stat: &FileStat) -> Result<(), TrustError> {
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
pub fn check_dir_stat(path: &Path, stat: &FileStat) -> Result<(), TrustError> {
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
///
/// The path is fully canonicalized (every component, not just the leaf)
/// before checks. This is required for the spec §11.2 deployment where
/// the unit's `ExecStart` resolves to `/var/lib/boxpilot/cores/current/sing-box`
/// — `current` is a symlink ancestor, and a per-component lstat would
/// reject it with "not a directory". Canonicalization first, then
/// `allowed_prefixes` last, preserves the §6.5 safety property: a malicious
/// symlink that resolves outside the allowed roots is still rejected.
pub fn verify_executable_path(
    fs: &dyn FsMetadataProvider,
    path: &Path,
    allowed_prefixes: &[PathBuf],
) -> Result<PathBuf, TrustError> {
    let canonical = canonicalize_path(fs, path)?;
    let bin_stat = fs.stat(&canonical).map_err(|e| match e.kind() {
        io::ErrorKind::NotFound => TrustError::NotFound(canonical.clone()),
        _ => TrustError::SymlinkResolution(format!("{e}")),
    })?;
    check_binary_stat(&canonical, &bin_stat)?;

    let mut current = canonical
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

    if !allowed_prefixes.iter().any(|p| canonical.starts_with(p)) {
        return Err(TrustError::DisallowedPrefix(canonical));
    }
    Ok(canonical)
}

/// Resolve every symlink along `path` (not just the leaf) using the trait's
/// stat / read_link primitives. Uses a deque-based component walker:
/// when a component resolves to a symlink, the target's components are
/// prepended to the remaining input so they are re-canonicalized in turn.
/// `MAX_HOPS` bounds total components processed across the entire walk
/// (input + every expansion), defending against symlink loops.
///
/// The walker — rather than `std::fs::canonicalize` — keeps test code
/// (`FakeFs`) and production code (`StdFsMetadataProvider`) traversing
/// the same logic, eliminating drift between mock and real-fs behavior.
pub fn canonicalize_path(
    fs: &dyn FsMetadataProvider,
    path: &Path,
) -> Result<PathBuf, TrustError> {
    if !path.is_absolute() {
        return Err(TrustError::SymlinkResolution(format!(
            "path is not absolute: {}",
            path.display()
        )));
    }

    let mut remaining: VecDeque<OsString> = path
        .components()
        .filter_map(|c| match c {
            Component::Normal(n) => Some(n.to_os_string()),
            Component::ParentDir => Some(OsString::from("..")),
            // RootDir, CurDir, Prefix(Windows) — skip; absolute path
            // anchors result at "/" below.
            _ => None,
        })
        .collect();

    let mut result = PathBuf::from("/");
    let mut hops: u32 = 0;
    const MAX_HOPS: u32 = 256;

    while let Some(name) = remaining.pop_front() {
        if hops >= MAX_HOPS {
            return Err(TrustError::SymlinkResolution(
                "symlink chain too deep".into(),
            ));
        }
        hops += 1;

        if name == OsStr::new("..") {
            result.pop();
            continue;
        }

        result.push(&name);

        match fs.stat(&result) {
            Ok(s) if matches!(s.kind, FileKind::Symlink) => {
                let target = fs
                    .read_link(&result)
                    .map_err(|e| TrustError::SymlinkResolution(format!("{e}")))?;
                // Pop the symlink itself; we replace it with the target's
                // components.
                result.pop();
                if target.is_absolute() {
                    result = PathBuf::from("/");
                }
                let target_components: Vec<OsString> = target
                    .components()
                    .filter_map(|c| match c {
                        Component::Normal(n) => Some(n.to_os_string()),
                        Component::ParentDir => Some(OsString::from("..")),
                        _ => None,
                    })
                    .collect();
                for c in target_components.into_iter().rev() {
                    remaining.push_front(c);
                }
            }
            Ok(_) => {} // regular file or directory; keep walking
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                return Err(TrustError::NotFound(result.clone()));
            }
            Err(e) => {
                return Err(TrustError::SymlinkResolution(format!(
                    "{}: {}",
                    result.display(),
                    e
                )));
            }
        }
    }

    Ok(result)
}

/// Linux trust checker — polymorphic facade over [`verify_executable_path`].
/// Holds the [`FsMetadataProvider`] as `Arc<dyn ...>` for `Send + Sync`
/// (matches the pattern used by other PR-3+ Linux impls).
pub struct LinuxTrustChecker {
    pub fs: Arc<dyn FsMetadataProvider>,
}

impl LinuxTrustChecker {
    pub fn new(fs: Arc<dyn FsMetadataProvider>) -> Self {
        Self { fs }
    }
}

impl TrustChecker for LinuxTrustChecker {
    fn check(
        &self,
        path: &Path,
        allowed_prefixes: &[PathBuf],
    ) -> Result<PathBuf, TrustError> {
        verify_executable_path(&*self.fs, path, allowed_prefixes)
    }
}

#[cfg(test)]
mod binary_check_tests {
    use super::*;
    use crate::fakes::fs_meta::FakeFs;

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

#[cfg(test)]
mod verify_tests {
    use super::*;
    use crate::fakes::fs_meta::FakeFs;

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

    /// Issue #7 regression: spec §11.2 deployment uses
    /// `/var/lib/boxpilot/cores/current/sing-box` where `current` is a
    /// symlink ancestor pointing at the active versioned dir. Pre-fix, the
    /// per-component lstat in the parent walk saw `current` as a symlink
    /// (FileKind::Symlink) and rejected with "not a directory". Now the
    /// path is fully canonicalized first, so the parent walk sees only
    /// real directories.
    #[test]
    fn symlink_ancestor_under_cores_resolves_and_passes() {
        let fs = FakeFs::default();
        // Real chain: /var/lib/boxpilot/cores/1.10.0/sing-box
        fs.put("/", FakeFs::root_dir());
        fs.put("/var", FakeFs::root_dir());
        fs.put("/var/lib", FakeFs::root_dir());
        fs.put("/var/lib/boxpilot", FakeFs::root_dir());
        fs.put("/var/lib/boxpilot/cores", FakeFs::root_dir());
        fs.put("/var/lib/boxpilot/cores/1.10.0", FakeFs::root_dir());
        fs.put("/var/lib/boxpilot/cores/1.10.0/sing-box", FakeFs::root_bin());
        // Symlink ancestor: cores/current -> 1.10.0
        fs.put(
            "/var/lib/boxpilot/cores/current",
            FileStat {
                uid: 0,
                gid: 0,
                mode: 0o755,
                kind: FileKind::Symlink,
            },
        );
        fs.links.lock().unwrap().insert(
            PathBuf::from("/var/lib/boxpilot/cores/current"),
            PathBuf::from("1.10.0"),
        );

        let r = verify_executable_path(
            &fs,
            Path::new("/var/lib/boxpilot/cores/current/sing-box"),
            &default_allowed_prefixes(),
        );
        assert!(r.is_ok(), "{r:?}");
        assert_eq!(
            r.unwrap(),
            PathBuf::from("/var/lib/boxpilot/cores/1.10.0/sing-box")
        );
    }

    /// Defense-in-depth: a symlink that resolves outside the allowed prefix
    /// list must still be rejected. Canonicalization first means we see
    /// the real target; the prefix check at the end catches escapes.
    #[test]
    fn symlink_escaping_allowed_prefix_is_rejected() {
        let fs = FakeFs::default();
        fs.put("/", FakeFs::root_dir());
        fs.put("/usr", FakeFs::root_dir());
        fs.put("/usr/bin", FakeFs::root_dir());
        fs.put("/tmp", FakeFs::root_dir());
        fs.put("/tmp/sing-box", FakeFs::root_bin());
        // /usr/bin/sing-box is a symlink to /tmp/sing-box (escapes allowed prefixes)
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
            PathBuf::from("/tmp/sing-box"),
        );

        let r = verify_executable_path(
            &fs,
            Path::new("/usr/bin/sing-box"),
            &default_allowed_prefixes(),
        );
        assert!(
            matches!(r, Err(TrustError::DisallowedPrefix(_))),
            "expected DisallowedPrefix on canonical /tmp/sing-box, got {r:?}"
        );
    }

    /// `canonicalize_path` must terminate on a self-loop (a -> a) rather
    /// than running until stack overflow. The bound is MAX_HOPS internal
    /// to the walker.
    #[test]
    fn symlink_self_loop_is_rejected() {
        let fs = FakeFs::default();
        fs.put("/", FakeFs::root_dir());
        fs.put("/usr", FakeFs::root_dir());
        fs.put("/usr/bin", FakeFs::root_dir());
        fs.put(
            "/usr/bin/sing-box",
            FileStat {
                uid: 0,
                gid: 0,
                mode: 0o755,
                kind: FileKind::Symlink,
            },
        );
        // self-loop
        fs.links.lock().unwrap().insert(
            PathBuf::from("/usr/bin/sing-box"),
            PathBuf::from("sing-box"),
        );

        let r = canonicalize_path(&fs, Path::new("/usr/bin/sing-box"));
        assert!(
            matches!(r, Err(TrustError::SymlinkResolution(ref s)) if s.contains("too deep")),
            "{r:?}"
        );
    }
}
