//! Canonical filesystem paths used by the helper. Tests construct
//! `Paths::with_root(tmpdir)` so unit tests can run as a normal user without
//! touching real `/etc` or `/run`.

use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct Paths {
    root: PathBuf,
}

impl Paths {
    /// Production paths rooted at `/`.
    pub fn system() -> Self {
        Self {
            root: PathBuf::from("/"),
        }
    }

    /// Test/dev paths rooted at an arbitrary directory.
    #[allow(dead_code)] // used in tests and plan #2+ dev helpers
    pub fn with_root(root: impl AsRef<Path>) -> Self {
        Self {
            root: root.as_ref().to_path_buf(),
        }
    }

    pub fn boxpilot_toml(&self) -> PathBuf {
        self.root.join("etc/boxpilot/boxpilot.toml")
    }

    #[allow(dead_code)] // used in plan #2+ (controller-name file read/write)
    pub fn controller_name_file(&self) -> PathBuf {
        // Plain-text username file consumed by the polkit JS rule
        // (49-boxpilot.rules). The rule reads it via `polkit.spawn(["cat",
        // …])` and compares against `subject.user`, which is itself the
        // caller's username string (NOT a UID). Plan #2's controller-claim
        // flow MUST write this file and boxpilot.toml's `controller_uid`
        // atomically under the same /run/boxpilot/lock acquisition — if
        // only the toml is updated, polkit keeps using the stale name
        // until the file is rewritten or the system reboots, silently
        // failing authorization for the new controller.
        self.root.join("etc/boxpilot/controller-name")
    }

    pub fn run_lock(&self) -> PathBuf {
        self.root.join("run/boxpilot/lock")
    }

    #[allow(dead_code)] // used in plan #2+ (run directory setup)
    pub fn run_dir(&self) -> PathBuf {
        self.root.join("run/boxpilot")
    }

    #[allow(dead_code)] // used in tests and plan #2+ (etc directory setup)
    pub fn etc_dir(&self) -> PathBuf {
        self.root.join("etc/boxpilot")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn system_paths_anchor_at_root() {
        let p = Paths::system();
        assert_eq!(
            p.boxpilot_toml(),
            PathBuf::from("/etc/boxpilot/boxpilot.toml")
        );
        assert_eq!(p.run_lock(), PathBuf::from("/run/boxpilot/lock"));
    }

    #[test]
    fn with_root_relocates_everything() {
        let p = Paths::with_root("/tmp/fake");
        assert_eq!(
            p.boxpilot_toml(),
            PathBuf::from("/tmp/fake/etc/boxpilot/boxpilot.toml")
        );
    }
}
