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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ActivateBundleRequest {
    /// 1..=30 seconds; `None` means take the daemon default (5 s).
    #[serde(default)]
    pub verify_window_secs: Option<u32>,
    /// Soft hint to short-circuit oversized bundles before mmap. Daemon
    /// still enforces hard `BUNDLE_MAX_TOTAL_BYTES` while walking.
    #[serde(default)]
    pub expected_total_bytes: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActivateOutcome {
    Active,
    RolledBack,
    RollbackTargetMissing,
    RollbackUnstartable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VerifySummary {
    pub window_used_ms: u64,
    pub n_restarts_pre: u32,
    pub n_restarts_post: u32,
    /// `None` when verify never read state (e.g. early failure path).
    #[serde(default)]
    pub final_unit_state: Option<crate::UnitState>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActivateBundleResponse {
    pub outcome: ActivateOutcome,
    pub activation_id: String,
    #[serde(default)]
    pub previous_activation_id: Option<String>,
    pub verify: VerifySummary,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RollbackRequest {
    pub target_activation_id: String,
    #[serde(default)]
    pub verify_window_secs: Option<u32>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn source_kind_wire_form_matches_spec() {
        assert_eq!(
            serde_json::to_string(&SourceKind::Local).unwrap(),
            "\"local\""
        );
        assert_eq!(
            serde_json::to_string(&SourceKind::LocalDir).unwrap(),
            "\"local-dir\""
        );
        assert_eq!(
            serde_json::to_string(&SourceKind::Remote).unwrap(),
            "\"remote\""
        );
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

    #[test]
    fn activate_request_round_trip() {
        let r = ActivateBundleRequest {
            verify_window_secs: Some(5),
            expected_total_bytes: Some(12345),
        };
        let s = serde_json::to_string(&r).unwrap();
        let back: ActivateBundleRequest = serde_json::from_str(&s).unwrap();
        assert_eq!(back, r);
    }

    #[test]
    fn activate_request_defaults_when_fields_missing() {
        let r: ActivateBundleRequest = serde_json::from_str("{}").unwrap();
        assert_eq!(r.verify_window_secs, None);
        assert_eq!(r.expected_total_bytes, None);
    }

    #[test]
    fn activate_outcome_serializes_snake_case() {
        assert_eq!(
            serde_json::to_string(&ActivateOutcome::Active).unwrap(),
            "\"active\""
        );
        assert_eq!(
            serde_json::to_string(&ActivateOutcome::RolledBack).unwrap(),
            "\"rolled_back\""
        );
        assert_eq!(
            serde_json::to_string(&ActivateOutcome::RollbackTargetMissing).unwrap(),
            "\"rollback_target_missing\""
        );
        assert_eq!(
            serde_json::to_string(&ActivateOutcome::RollbackUnstartable).unwrap(),
            "\"rollback_unstartable\""
        );
    }

    #[test]
    fn activate_response_round_trip() {
        let r = ActivateBundleResponse {
            outcome: ActivateOutcome::Active,
            activation_id: "id-1".into(),
            previous_activation_id: Some("id-0".into()),
            verify: VerifySummary {
                window_used_ms: 4321,
                n_restarts_pre: 2,
                n_restarts_post: 2,
                final_unit_state: None,
            },
        };
        let s = serde_json::to_string(&r).unwrap();
        let back: ActivateBundleResponse = serde_json::from_str(&s).unwrap();
        assert_eq!(back, r);
    }

    #[test]
    fn rollback_request_round_trip() {
        let r = RollbackRequest {
            target_activation_id: "id-0".into(),
            verify_window_secs: Some(5),
        };
        let s = serde_json::to_string(&r).unwrap();
        let back: RollbackRequest = serde_json::from_str(&s).unwrap();
        assert_eq!(back, r);
    }
}
