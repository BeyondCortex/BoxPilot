//! IPC types for the five `core.*` methods (spec §11). Wire format is
//! JSON-encoded `String` per plan #1 convention.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CoreKind {
    External,
    ManagedInstalled,
    ManagedAdopted,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CoreSource {
    pub url: Option<String>,
    pub source_path: Option<String>,
    pub upstream_sha256_match: Option<bool>,
    pub computed_sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiscoveredCore {
    pub kind: CoreKind,
    pub path: String,
    pub version: String,
    pub sha256: String,
    pub installed_at: Option<String>,
    pub source: Option<CoreSource>,
    pub label: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CoreDiscoverResponse {
    pub cores: Vec<DiscoveredCore>,
    pub current: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum VersionRequest {
    Latest,
    Exact { version: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ArchRequest {
    Auto,
    Exact { arch: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CoreInstallRequest {
    pub version: VersionRequest,
    pub architecture: ArchRequest,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CoreInstallResponse {
    pub installed: DiscoveredCore,
    pub became_current: bool,
    pub upstream_sha256_match: Option<bool>,
    pub claimed_controller: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CoreRollbackRequest {
    pub to_label: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CoreAdoptRequest {
    pub source_path: String,
}

/// Per-core install-source.json schema (spec §5.1).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstallSourceJson {
    pub schema_version: u32,
    pub kind: CoreKind,
    pub version: String,
    pub architecture: String,
    pub url: Option<String>,
    pub source_path: Option<String>,
    pub upstream_sha256_match: Option<bool>,
    pub computed_sha256_tarball: Option<String>,
    pub computed_sha256_binary: String,
    pub installed_at: String,
    pub user_agent_used: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn version_request_latest_wire_form() {
        let v = serde_json::to_value(&VersionRequest::Latest).unwrap();
        assert_eq!(v, serde_json::json!({"kind": "latest"}));
    }

    #[test]
    fn version_request_exact_wire_form() {
        let v = serde_json::to_value(&VersionRequest::Exact {
            version: "1.10.0".into(),
        })
        .unwrap();
        assert_eq!(v, serde_json::json!({"kind": "exact", "version": "1.10.0"}));
    }

    #[test]
    fn arch_request_auto_wire_form() {
        let v = serde_json::to_value(&ArchRequest::Auto).unwrap();
        assert_eq!(v, serde_json::json!({"kind": "auto"}));
    }

    #[test]
    fn core_kind_uses_kebab_case() {
        let v = serde_json::to_value(&CoreKind::ManagedInstalled).unwrap();
        assert_eq!(v, serde_json::json!("managed-installed"));
    }

    #[test]
    fn install_request_round_trip() {
        let req = CoreInstallRequest {
            version: VersionRequest::Exact {
                version: "1.10.0".into(),
            },
            architecture: ArchRequest::Auto,
        };
        let json = serde_json::to_string(&req).unwrap();
        let back: CoreInstallRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(back, req);
    }

    #[test]
    fn install_source_json_round_trip() {
        let src = InstallSourceJson {
            schema_version: 1,
            kind: CoreKind::ManagedInstalled,
            version: "1.10.0".into(),
            architecture: "x86_64".into(),
            url: Some("https://example/x.tar.gz".into()),
            source_path: None,
            upstream_sha256_match: Some(true),
            computed_sha256_tarball: Some("abc".into()),
            computed_sha256_binary: "def".into(),
            installed_at: "2026-04-28T10:00:00-07:00".into(),
            user_agent_used: "boxpilot/0.2.0".into(),
        };
        let json = serde_json::to_string(&src).unwrap();
        let back: InstallSourceJson = serde_json::from_str(&json).unwrap();
        assert_eq!(back, src);
    }
}
