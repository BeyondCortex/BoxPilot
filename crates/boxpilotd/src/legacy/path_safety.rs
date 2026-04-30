//! Spec §8 / §9.3 — refuse to keep a system-service config reference under
//! `/home`, `/tmp`, `/run/user`, etc.

use boxpilot_ipc::ConfigPathKind;
use std::path::Path;

/// Classify an absolute path. Relative paths are `Unknown` (we don't trust
/// them as system service references in the first place).
pub fn classify_config_path(p: &Path) -> ConfigPathKind {
    if !p.is_absolute() {
        return ConfigPathKind::Unknown;
    }
    let s = p.to_string_lossy();
    const UNSAFE_PREFIXES: &[&str] = &[
        "/home/",
        "/tmp/",
        "/var/tmp/",
        "/run/user/",
        "/dev/",
        "/proc/",
    ];
    for pre in UNSAFE_PREFIXES {
        if s.starts_with(pre) {
            return ConfigPathKind::UserOrEphemeral;
        }
    }
    const SAFE_PREFIXES: &[&str] = &[
        "/etc/", "/usr/", "/var/lib/", "/var/cache/", "/opt/", "/srv/",
    ];
    for pre in SAFE_PREFIXES {
        if s.starts_with(pre) {
            return ConfigPathKind::SystemPath;
        }
    }
    ConfigPathKind::Unknown
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn etc_is_system_path() {
        assert_eq!(
            classify_config_path(Path::new("/etc/sing-box/config.json")),
            ConfigPathKind::SystemPath
        );
    }

    #[test]
    fn home_is_user_or_ephemeral() {
        assert_eq!(
            classify_config_path(Path::new("/home/alice/.config/sing-box/config.json")),
            ConfigPathKind::UserOrEphemeral
        );
    }

    #[test]
    fn tmp_run_user_var_tmp_are_user_or_ephemeral() {
        for p in [
            "/tmp/sb.json",
            "/var/tmp/sb.json",
            "/run/user/1000/sb.json",
            "/dev/shm/sb.json",
            "/proc/self/fd/3",
        ] {
            assert_eq!(
                classify_config_path(Path::new(p)),
                ConfigPathKind::UserOrEphemeral,
                "{p} should be UserOrEphemeral"
            );
        }
    }

    #[test]
    fn relative_is_unknown() {
        assert_eq!(
            classify_config_path(Path::new("config.json")),
            ConfigPathKind::Unknown
        );
    }

    #[test]
    fn other_absolute_is_unknown() {
        assert_eq!(
            classify_config_path(Path::new("/mnt/nfs/sb.json")),
            ConfigPathKind::Unknown
        );
    }

    #[test]
    fn home_prefix_is_not_substring_match() {
        // "/homework" must not be classified as UserOrEphemeral
        // — we only match path component prefixes via the trailing slash.
        assert_eq!(
            classify_config_path(Path::new("/homework/sb.json")),
            ConfigPathKind::Unknown
        );
    }
}
