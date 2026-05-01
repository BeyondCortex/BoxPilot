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

    #[allow(dead_code)] // used in plan #2+ (install-state ledger read/write)
    pub fn install_state_json(&self) -> PathBuf {
        self.root.join("var/lib/boxpilot/install-state.json")
    }

    #[allow(dead_code)] // used in plan #2+ (cores tree management)
    pub fn cores_dir(&self) -> PathBuf {
        self.root.join("var/lib/boxpilot/cores")
    }

    #[allow(dead_code)] // used in plan #2+ (current symlink atomic swap)
    pub fn cores_current_symlink(&self) -> PathBuf {
        self.cores_dir().join("current")
    }

    #[allow(dead_code)] // used in plan #2+ (core download staging)
    pub fn cores_staging_dir(&self) -> PathBuf {
        self.root.join("var/lib/boxpilot/.staging-cores")
    }

    /// `/etc/systemd/system/<unit_name>`. Written by `service.install_managed`.
    /// The unit name comes from `BoxpilotConfig::target_service` so a
    /// non-default `boxpilot.toml` controls the same unit it installs.
    pub fn systemd_unit_path(&self, unit_name: &str) -> PathBuf {
        self.root.join("etc/systemd/system").join(unit_name)
    }

    /// `/etc/polkit-1/rules.d/48-boxpilot-controller.rules`. The daemon
    /// rewrites this file under `/run/boxpilot/lock` whenever the
    /// controller is claimed or transferred, so `49-boxpilot.rules` can
    /// read `BOXPILOT_CONTROLLER` directly instead of spawning `cat`.
    /// `48-` sorts before `49-` and polkit evaluates rules.d/* in lexical
    /// order, so the var is in scope when the main rule runs.
    pub fn polkit_controller_dropin_path(&self) -> PathBuf {
        self.root
            .join("etc/polkit-1/rules.d/48-boxpilot-controller.rules")
    }

    /// `/etc/boxpilot/releases` — root of versioned release dirs.
    pub fn releases_dir(&self) -> PathBuf {
        self.root.join("etc/boxpilot/releases")
    }

    /// `/etc/boxpilot/.staging` — short-lived per-activation unpack dirs.
    pub fn staging_dir(&self) -> PathBuf {
        self.root.join("etc/boxpilot/.staging")
    }

    /// `/etc/boxpilot/active` — symlink to the currently-active release.
    pub fn active_symlink(&self) -> PathBuf {
        self.root.join("etc/boxpilot/active")
    }

    /// `/etc/boxpilot/releases/<activation_id>`.
    pub fn release_dir(&self, activation_id: &str) -> PathBuf {
        self.releases_dir().join(activation_id)
    }

    /// `/etc/boxpilot/.staging/<activation_id>`.
    pub fn staging_subdir(&self, activation_id: &str) -> PathBuf {
        self.staging_dir().join(activation_id)
    }

    /// `/var/lib/boxpilot/backups/units` — destination for legacy-unit
    /// fragment backups taken before migrate-cutover. Spec §5.4.
    pub fn backups_units_dir(&self) -> PathBuf {
        self.root.join("var/lib/boxpilot/backups/units")
    }

    /// `/var/cache/boxpilot/diagnostics` — root of redacted diagnostics
    /// bundles, capped at `DIAGNOSTICS_BUNDLE_CAP_BYTES` (§5.5).
    pub fn cache_diagnostics_dir(&self) -> PathBuf {
        self.root.join("var/cache/boxpilot/diagnostics")
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

    #[test]
    fn systemd_unit_path_joins_unit_name_under_etc_systemd_system() {
        let p = Paths::system();
        assert_eq!(
            p.systemd_unit_path("boxpilot-sing-box.service"),
            PathBuf::from("/etc/systemd/system/boxpilot-sing-box.service")
        );
        // Non-default name is honored, not silently rewritten.
        assert_eq!(
            p.systemd_unit_path("custom.service"),
            PathBuf::from("/etc/systemd/system/custom.service")
        );
    }

    #[test]
    fn polkit_dropin_path_uses_48_prefix_so_it_loads_before_49() {
        let p = Paths::system();
        assert_eq!(
            p.polkit_controller_dropin_path(),
            PathBuf::from("/etc/polkit-1/rules.d/48-boxpilot-controller.rules")
        );
    }

    #[test]
    fn release_paths_under_etc_boxpilot() {
        let p = Paths::with_root("/tmp/fake");
        assert_eq!(
            p.releases_dir(),
            PathBuf::from("/tmp/fake/etc/boxpilot/releases")
        );
        assert_eq!(
            p.staging_dir(),
            PathBuf::from("/tmp/fake/etc/boxpilot/.staging")
        );
        assert_eq!(
            p.active_symlink(),
            PathBuf::from("/tmp/fake/etc/boxpilot/active")
        );
        assert_eq!(
            p.release_dir("2026-04-30T00-00-00Z-abc"),
            PathBuf::from("/tmp/fake/etc/boxpilot/releases/2026-04-30T00-00-00Z-abc"),
        );
        assert_eq!(
            p.staging_subdir("2026-04-30T00-00-00Z-abc"),
            PathBuf::from("/tmp/fake/etc/boxpilot/.staging/2026-04-30T00-00-00Z-abc"),
        );
    }

    #[test]
    fn backups_units_dir_under_var_lib_boxpilot() {
        let p = Paths::with_root("/tmp/fake");
        assert_eq!(
            p.backups_units_dir(),
            PathBuf::from("/tmp/fake/var/lib/boxpilot/backups/units")
        );
    }

    #[test]
    fn cache_diagnostics_dir_under_var_cache_boxpilot() {
        let p = Paths::with_root("/tmp/fake");
        assert_eq!(
            p.cache_diagnostics_dir(),
            PathBuf::from("/tmp/fake/var/cache/boxpilot/diagnostics")
        );
    }

    #[test]
    fn cache_diagnostics_dir_in_system_paths() {
        let p = Paths::system();
        assert_eq!(
            p.cache_diagnostics_dir(),
            PathBuf::from("/var/cache/boxpilot/diagnostics")
        );
    }
}
