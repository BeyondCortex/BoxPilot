//! Best-effort host-info collector for the diagnostics bundle. Each field
//! falls back to "unknown" rather than failing the whole export.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SystemInfo {
    pub kernel: String,
    pub os_id: String,
    pub os_version_id: String,
    pub os_pretty_name: String,
    pub boxpilot_version: String,
}

pub fn collect(os_release_path: &std::path::Path) -> SystemInfo {
    SystemInfo {
        kernel: kernel_release(),
        os_id: read_os_release_field(os_release_path, "ID").unwrap_or_else(|| "unknown".into()),
        os_version_id: read_os_release_field(os_release_path, "VERSION_ID")
            .unwrap_or_else(|| "unknown".into()),
        os_pretty_name: read_os_release_field(os_release_path, "PRETTY_NAME")
            .unwrap_or_else(|| "unknown".into()),
        boxpilot_version: env!("CARGO_PKG_VERSION").to_string(),
    }
}

fn kernel_release() -> String {
    nix::sys::utsname::uname()
        .ok()
        .and_then(|u| u.release().to_str().map(|s| s.to_string()))
        .unwrap_or_else(|| "unknown".into())
}

/// Parse a `KEY=value` (or `KEY="value with spaces"`) line from /etc/os-release.
fn read_os_release_field(path: &std::path::Path, key: &str) -> Option<String> {
    let content = std::fs::read_to_string(path).ok()?;
    for line in content.lines() {
        if let Some(rest) = line.strip_prefix(&format!("{key}=")) {
            let trimmed = rest.trim_matches('"').to_string();
            return Some(trimmed);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use tempfile::tempdir;

    #[test]
    fn parses_quoted_pretty_name() {
        let tmp = tempdir().unwrap();
        let p = tmp.path().join("os-release");
        std::fs::write(
            &p,
            "NAME=\"Ubuntu\"\nID=ubuntu\nVERSION_ID=\"24.04\"\nPRETTY_NAME=\"Ubuntu 24.04 LTS\"\n",
        )
        .unwrap();
        let info = collect(&p);
        assert_eq!(info.os_id, "ubuntu");
        assert_eq!(info.os_version_id, "24.04");
        assert_eq!(info.os_pretty_name, "Ubuntu 24.04 LTS");
    }

    #[test]
    fn missing_os_release_falls_back_to_unknown() {
        let tmp = tempdir().unwrap();
        let info = collect(&tmp.path().join("nonexistent"));
        assert_eq!(info.os_id, "unknown");
        assert_eq!(info.os_version_id, "unknown");
        assert_eq!(info.os_pretty_name, "unknown");
        assert!(!info.boxpilot_version.is_empty());
    }

    #[test]
    fn kernel_is_nonempty_on_real_host() {
        let k = kernel_release();
        assert!(!k.is_empty());
    }
}
