use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AssetEntry {
    pub path: String,
    pub sha256: String,
    pub size: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SourceKind {
    #[serde(rename = "local")]
    Local,
    #[serde(rename = "local-dir")]
    LocalDir,
    #[serde(rename = "remote")]
    Remote,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActivationManifest {
    pub schema_version: u32,
    pub activation_id: String,
    pub profile_id: String,
    pub profile_sha256: String,
    pub config_sha256: String,
    pub source_kind: SourceKind,
    /// Always present for `Remote`, always `None` for `Local` / `LocalDir`.
    pub source_url_redacted: Option<String>,
    pub core_path_at_activation: String,
    pub core_version_at_activation: String,
    /// RFC3339 with timezone (matches plan #2 install-state timestamps).
    pub created_at: String,
    pub assets: Vec<AssetEntry>,
}

pub const ACTIVATION_MANIFEST_SCHEMA_VERSION: u32 = 1;

/// Spec §9.2 default size limits.
pub const BUNDLE_MAX_FILE_BYTES: u64 = 16 * 1024 * 1024;
pub const BUNDLE_MAX_TOTAL_BYTES: u64 = 64 * 1024 * 1024;
pub const BUNDLE_MAX_FILE_COUNT: u32 = 1024;
pub const BUNDLE_MAX_NESTING_DEPTH: u32 = 8;

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn source_kind_wire_form_matches_spec() {
        assert_eq!(serde_json::to_string(&SourceKind::Local).unwrap(), "\"local\"");
        assert_eq!(serde_json::to_string(&SourceKind::LocalDir).unwrap(), "\"local-dir\"");
        assert_eq!(serde_json::to_string(&SourceKind::Remote).unwrap(), "\"remote\"");
    }

    #[test]
    fn activation_manifest_round_trips() {
        let m = ActivationManifest {
            schema_version: ACTIVATION_MANIFEST_SCHEMA_VERSION,
            activation_id: "2026-04-30T00-00-00Z-abc123".into(),
            profile_id: "profile-id".into(),
            profile_sha256: "deadbeef".into(),
            config_sha256: "cafebabe".into(),
            source_kind: SourceKind::Remote,
            source_url_redacted: Some("https://host/path?token=***".into()),
            core_path_at_activation: "/var/lib/boxpilot/cores/current/sing-box".into(),
            core_version_at_activation: "1.10.0".into(),
            created_at: "2026-04-30T00:00:00-07:00".into(),
            assets: vec![AssetEntry {
                path: "geosite.db".into(),
                sha256: "abc".into(),
                size: 12345,
            }],
        };
        let s = serde_json::to_string(&m).unwrap();
        let back: ActivationManifest = serde_json::from_str(&s).unwrap();
        assert_eq!(back, m);
    }

    #[test]
    fn limits_match_spec_defaults() {
        assert_eq!(BUNDLE_MAX_FILE_BYTES, 16 * 1024 * 1024);
        assert_eq!(BUNDLE_MAX_TOTAL_BYTES, 64 * 1024 * 1024);
        assert_eq!(BUNDLE_MAX_FILE_COUNT, 1024);
        assert_eq!(BUNDLE_MAX_NESTING_DEPTH, 8);
    }
}
