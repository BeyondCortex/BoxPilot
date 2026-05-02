//! Canonical filesystem paths. Constructors call `EnvProvider` once at boot
//! and cache the resulting roots.
//!
//! Platform layout (per spec §5.1 + §7):
//!
//! - **Linux:** `system_root = /`, paths under `/etc/boxpilot/`,
//!   `/var/lib/boxpilot/`, `/var/cache/boxpilot/`, `/run/boxpilot/`,
//!   `/etc/systemd/system/`, `/etc/polkit-1/rules.d/`.
//!   `user_root = $HOME/.local/share/boxpilot` (or `$XDG_DATA_HOME/boxpilot`).
//! - **Windows:** `system_root = %ProgramData%\BoxPilot`, paths flatten
//!   directly under that root (no `etc/`/`var/` segments — `boxpilot.toml`
//!   sits at `system_root.join("boxpilot.toml")`).
//!   `user_root = %LocalAppData%\BoxPilot`.

use crate::traits::env::{EnvError, EnvProvider};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct Paths {
    system_root: PathBuf,
    user_root: PathBuf,
}

impl Paths {
    pub fn from_env(env: &dyn EnvProvider) -> Result<Self, EnvError> {
        Ok(Self {
            system_root: env.system_root()?,
            user_root: env.user_root()?,
        })
    }

    /// Production constructor — uses [`crate::linux::env::StdEnv`] /
    /// [`crate::windows::env::StdEnv`] depending on target.
    pub fn system() -> Result<Self, EnvError> {
        #[cfg(target_os = "linux")]
        {
            return Self::from_env(&crate::linux::env::StdEnv);
        }
        #[cfg(target_os = "windows")]
        {
            return Self::from_env(&crate::windows::env::StdEnv);
        }
        #[allow(unreachable_code)]
        Err(EnvError::Missing("unsupported platform"))
    }

    /// Test/dev constructor — both roots under `tmp`.
    pub fn with_root(tmp: impl AsRef<Path>) -> Self {
        let tmp = tmp.as_ref().to_path_buf();
        Self {
            user_root: tmp.join("user"),
            system_root: tmp,
        }
    }

    pub fn user_root(&self) -> &Path {
        &self.user_root
    }

    // ---- §5.3 system runtime state ------------------------------------

    pub fn boxpilot_toml(&self) -> PathBuf {
        #[cfg(target_os = "linux")]
        { self.system_root.join("etc/boxpilot/boxpilot.toml") }
        #[cfg(target_os = "windows")]
        { self.system_root.join("boxpilot.toml") }
        #[cfg(not(any(target_os = "linux", target_os = "windows")))]
        { unreachable!("unsupported platform") }
    }

    pub fn controller_name_file(&self) -> PathBuf {
        // Linux-only on disk; on Windows the file is never written.
        // Method exists on both platforms for caller-uniformity; callers
        // that write it must be cfg(target_os = "linux")-gated.
        #[cfg(target_os = "linux")]
        { self.system_root.join("etc/boxpilot/controller-name") }
        #[cfg(target_os = "windows")]
        { self.system_root.join("controller-name") }
        #[cfg(not(any(target_os = "linux", target_os = "windows")))]
        { unreachable!("unsupported platform") }
    }

    pub fn run_lock(&self) -> PathBuf {
        #[cfg(target_os = "linux")]
        { self.system_root.join("run/boxpilot/lock") }
        #[cfg(target_os = "windows")]
        { self.system_root.join("run").join("lock") }
        #[cfg(not(any(target_os = "linux", target_os = "windows")))]
        { unreachable!("unsupported platform") }
    }

    pub fn run_dir(&self) -> PathBuf {
        #[cfg(target_os = "linux")]
        { self.system_root.join("run/boxpilot") }
        #[cfg(target_os = "windows")]
        { self.system_root.join("run") }
        #[cfg(not(any(target_os = "linux", target_os = "windows")))]
        { unreachable!("unsupported platform") }
    }

    pub fn etc_dir(&self) -> PathBuf {
        #[cfg(target_os = "linux")]
        { self.system_root.join("etc/boxpilot") }
        #[cfg(target_os = "windows")]
        { self.system_root.clone() }
        #[cfg(not(any(target_os = "linux", target_os = "windows")))]
        { unreachable!("unsupported platform") }
    }

    pub fn install_state_json(&self) -> PathBuf {
        #[cfg(target_os = "linux")]
        { self.system_root.join("var/lib/boxpilot/install-state.json") }
        #[cfg(target_os = "windows")]
        { self.system_root.join("install-state.json") }
        #[cfg(not(any(target_os = "linux", target_os = "windows")))]
        { unreachable!("unsupported platform") }
    }

    pub fn cores_dir(&self) -> PathBuf {
        #[cfg(target_os = "linux")]
        { self.system_root.join("var/lib/boxpilot/cores") }
        #[cfg(target_os = "windows")]
        { self.system_root.join("cores") }
        #[cfg(not(any(target_os = "linux", target_os = "windows")))]
        { unreachable!("unsupported platform") }
    }

    pub fn cores_current_symlink(&self) -> PathBuf {
        self.cores_dir().join("current")
    }

    pub fn cores_staging_dir(&self) -> PathBuf {
        #[cfg(target_os = "linux")]
        { self.system_root.join("var/lib/boxpilot/.staging-cores") }
        #[cfg(target_os = "windows")]
        { self.system_root.join(".staging-cores") }
        #[cfg(not(any(target_os = "linux", target_os = "windows")))]
        { unreachable!("unsupported platform") }
    }

    pub fn systemd_unit_path(&self, unit_name: &str) -> PathBuf {
        // Linux-only callers; Windows has no systemd. Method present for
        // call-site uniformity but should be cfg-gated by callers.
        #[cfg(target_os = "linux")]
        { self.system_root.join("etc/systemd/system").join(unit_name) }
        #[cfg(target_os = "windows")]
        { self.system_root.join("systemd-units").join(unit_name) }
        #[cfg(not(any(target_os = "linux", target_os = "windows")))]
        { unreachable!("unsupported platform") }
    }

    pub fn polkit_controller_dropin_path(&self) -> PathBuf {
        #[cfg(target_os = "linux")]
        { self.system_root.join("etc/polkit-1/rules.d/48-boxpilot-controller.rules") }
        #[cfg(target_os = "windows")]
        { self.system_root.join("polkit-controller.rules") } // unused on Windows
        #[cfg(not(any(target_os = "linux", target_os = "windows")))]
        { unreachable!("unsupported platform") }
    }

    pub fn releases_dir(&self) -> PathBuf {
        #[cfg(target_os = "linux")]
        { self.system_root.join("etc/boxpilot/releases") }
        #[cfg(target_os = "windows")]
        { self.system_root.join("releases") }
        #[cfg(not(any(target_os = "linux", target_os = "windows")))]
        { unreachable!("unsupported platform") }
    }

    pub fn staging_dir(&self) -> PathBuf {
        #[cfg(target_os = "linux")]
        { self.system_root.join("etc/boxpilot/.staging") }
        #[cfg(target_os = "windows")]
        { self.system_root.join(".staging") }
        #[cfg(not(any(target_os = "linux", target_os = "windows")))]
        { unreachable!("unsupported platform") }
    }

    pub fn active_symlink(&self) -> PathBuf {
        // Linux: symlink. Windows: marker JSON file at active.json (PR 8/COQ8 / spec §5.3).
        // Both platforms expose this method; PR 8's ActivePointer trait
        // deals with the semantic difference.
        #[cfg(target_os = "linux")]
        { self.system_root.join("etc/boxpilot/active") }
        #[cfg(target_os = "windows")]
        { self.system_root.join("active.json") }
        #[cfg(not(any(target_os = "linux", target_os = "windows")))]
        { unreachable!("unsupported platform") }
    }

    pub fn release_dir(&self, activation_id: &str) -> PathBuf {
        self.releases_dir().join(activation_id)
    }

    pub fn staging_subdir(&self, activation_id: &str) -> PathBuf {
        self.staging_dir().join(activation_id)
    }

    pub fn backups_units_dir(&self) -> PathBuf {
        #[cfg(target_os = "linux")]
        { self.system_root.join("var/lib/boxpilot/backups/units") }
        #[cfg(target_os = "windows")]
        { self.system_root.join("backups").join("units") }
        #[cfg(not(any(target_os = "linux", target_os = "windows")))]
        { unreachable!("unsupported platform") }
    }

    pub fn cache_diagnostics_dir(&self) -> PathBuf {
        #[cfg(target_os = "linux")]
        { self.system_root.join("var/cache/boxpilot/diagnostics") }
        #[cfg(target_os = "windows")]
        { self.system_root.join("cache").join("diagnostics") }
        #[cfg(not(any(target_os = "linux", target_os = "windows")))]
        { unreachable!("unsupported platform") }
    }

    // ---- §5.6 user profile store --------------------------------------

    pub fn user_profiles_dir(&self) -> PathBuf {
        self.user_root.join("profiles")
    }

    pub fn user_remotes_json(&self) -> PathBuf {
        self.user_root.join("remotes.json")
    }

    pub fn user_ui_state_json(&self) -> PathBuf {
        self.user_root.join("ui-state.json")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn with_root_relocates_system_paths() {
        let p = Paths::with_root("/tmp/fake");
        #[cfg(target_os = "linux")]
        {
            assert_eq!(
                p.boxpilot_toml(),
                PathBuf::from("/tmp/fake/etc/boxpilot/boxpilot.toml")
            );
            assert_eq!(p.run_lock(), PathBuf::from("/tmp/fake/run/boxpilot/lock"));
        }
        #[cfg(target_os = "windows")]
        {
            assert_eq!(p.boxpilot_toml(), PathBuf::from("/tmp/fake/boxpilot.toml"));
            assert_eq!(p.run_lock(), PathBuf::from("/tmp/fake/run/lock"));
        }
    }

    #[test]
    fn user_root_is_separate_subdir_under_with_root() {
        let p = Paths::with_root("/tmp/fake");
        assert_eq!(p.user_root(), Path::new("/tmp/fake/user"));
        assert_eq!(
            p.user_profiles_dir(),
            PathBuf::from("/tmp/fake/user/profiles")
        );
    }
}
