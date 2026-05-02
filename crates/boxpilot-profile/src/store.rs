use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct ProfileStorePaths {
    root: PathBuf,
}

impl ProfileStorePaths {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    /// Build from a `boxpilot_platform::Paths`. This is the production
    /// constructor used by Tauri command handlers (per spec §5.1 / COQ16).
    /// `root` becomes `paths.user_root()` so the legacy
    /// `profiles_dir()`/`remotes_json()`/`ui_state_json()` methods continue
    /// to resolve to spec-§5.6 layout (i.e.
    /// `~/.local/share/boxpilot/{profiles,remotes.json,ui-state.json}`).
    pub fn from_paths(paths: &boxpilot_platform::Paths) -> Self {
        Self {
            root: paths.user_root().to_path_buf(),
        }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }
    pub fn profiles_dir(&self) -> PathBuf {
        self.root.join("profiles")
    }
    pub fn profile_dir(&self, id: &str) -> PathBuf {
        self.profiles_dir().join(id)
    }
    pub fn profile_source(&self, id: &str) -> PathBuf {
        self.profile_dir(id).join("source.json")
    }
    pub fn profile_assets_dir(&self, id: &str) -> PathBuf {
        self.profile_dir(id).join("assets")
    }
    pub fn profile_metadata(&self, id: &str) -> PathBuf {
        self.profile_dir(id).join("metadata.json")
    }
    pub fn profile_last_valid_dir(&self, id: &str) -> PathBuf {
        self.profile_dir(id).join("last-valid")
    }
    pub fn profile_last_valid_config(&self, id: &str) -> PathBuf {
        self.profile_last_valid_dir(id).join("config.json")
    }
    pub fn profile_last_valid_assets_dir(&self, id: &str) -> PathBuf {
        self.profile_last_valid_dir(id).join("assets")
    }
    pub fn remotes_json(&self) -> PathBuf {
        self.root.join("remotes.json")
    }
    pub fn ui_state_json(&self) -> PathBuf {
        self.root.join("ui-state.json")
    }
}

/// Idempotent: creates `path` (and parents via `create_dir_all`) if missing,
/// then forces **the leaf** to `0700`. Intermediate directories created by
/// `create_dir_all` are NOT chmod'd — callers must call this helper on each
/// path component they own (e.g. root, then `profiles/`, then a profile's
/// own dir) if they need every level forced to `0700`.
pub fn ensure_dir_0700(path: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(path)?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))
}

/// Atomic write with `0600` mode. Writes to a uniquely-named temp file
/// in the destination's parent directory, fsyncs, and renames into
/// place. Concurrent writers cannot collide on the temp file.
pub fn write_file_0600_atomic(path: &Path, contents: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    let parent = match path.parent() {
        Some(p) if !p.as_os_str().is_empty() => p,
        _ => Path::new("."),
    };
    std::fs::create_dir_all(parent)?;
    let mut tmp = tempfile::NamedTempFile::new_in(parent)?;
    // Set 0600 BEFORE writing so even partial bytes are unreadable to others.
    std::fs::set_permissions(tmp.path(), std::fs::Permissions::from_mode(0o600))?;
    tmp.write_all(contents)?;
    tmp.as_file().sync_all()?;
    tmp.persist(path)
        .map_err(|e| std::io::Error::other(format!("persist temp file: {}", e.error)))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn paths_layout_matches_spec_5_6() {
        let p = ProfileStorePaths::new(PathBuf::from("/x"));
        assert_eq!(p.profiles_dir(), PathBuf::from("/x/profiles"));
        assert_eq!(p.profile_dir("abc"), PathBuf::from("/x/profiles/abc"));
        assert_eq!(
            p.profile_source("abc"),
            PathBuf::from("/x/profiles/abc/source.json")
        );
        assert_eq!(
            p.profile_assets_dir("abc"),
            PathBuf::from("/x/profiles/abc/assets")
        );
        assert_eq!(
            p.profile_metadata("abc"),
            PathBuf::from("/x/profiles/abc/metadata.json")
        );
        assert_eq!(
            p.profile_last_valid_config("abc"),
            PathBuf::from("/x/profiles/abc/last-valid/config.json")
        );
        assert_eq!(
            p.profile_last_valid_assets_dir("abc"),
            PathBuf::from("/x/profiles/abc/last-valid/assets")
        );
        assert_eq!(p.remotes_json(), PathBuf::from("/x/remotes.json"));
        assert_eq!(p.ui_state_json(), PathBuf::from("/x/ui-state.json"));
    }

    #[test]
    fn ensure_dir_0700_creates_and_chmods() {
        let tmp = tempfile::tempdir().unwrap();
        let target = tmp.path().join("a/b/c");
        ensure_dir_0700(&target).unwrap();
        let mode = std::fs::metadata(&target).unwrap().permissions().mode() & 0o7777;
        assert_eq!(mode, 0o700);
    }

    #[test]
    fn ensure_dir_0700_is_idempotent_and_repairs_perms() {
        let tmp = tempfile::tempdir().unwrap();
        let target = tmp.path().join("d");
        std::fs::create_dir_all(&target).unwrap();
        std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o755)).unwrap();
        ensure_dir_0700(&target).unwrap();
        let mode = std::fs::metadata(&target).unwrap().permissions().mode() & 0o7777;
        assert_eq!(mode, 0o700);
    }

    #[test]
    fn write_file_0600_atomic_creates_with_correct_mode() {
        let tmp = tempfile::tempdir().unwrap();
        let target = tmp.path().join("nested/secret.json");
        write_file_0600_atomic(&target, b"{}").unwrap();
        let mode = std::fs::metadata(&target).unwrap().permissions().mode() & 0o7777;
        assert_eq!(mode, 0o600);
        assert_eq!(std::fs::read(&target).unwrap(), b"{}");
        // The .tmp sidecar must have been renamed away.
        assert!(!target.with_extension("tmp").exists());
    }
}
