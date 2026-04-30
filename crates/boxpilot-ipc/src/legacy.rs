use crate::UnitState;
use serde::{Deserialize, Serialize};

pub const LEGACY_UNIT_NAME: &str = "sing-box.service";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConfigPathKind {
    SystemPath,
    UserOrEphemeral,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LegacyObserveServiceResponse {
    pub detected: bool,
    #[serde(default)]
    pub unit_name: Option<String>,
    #[serde(default)]
    pub fragment_path: Option<String>,
    #[serde(default)]
    pub unit_file_state: Option<String>,
    #[serde(default)]
    pub exec_start_raw: Option<String>,
    #[serde(default)]
    pub config_path: Option<String>,
    pub config_path_kind: ConfigPathKind,
    pub unit_state: UnitState,
    pub conflicts_with_managed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "step", rename_all = "snake_case")]
pub enum LegacyMigrateRequest {
    Prepare,
    Cutover,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MigratedAsset {
    pub filename: String,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LegacyMigratePrepareResponse {
    pub unit_name: String,
    pub config_path_was: String,
    pub config_filename: String,
    pub config_bytes: Vec<u8>,
    pub assets: Vec<MigratedAsset>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LegacyMigrateCutoverResponse {
    pub unit_name: String,
    pub backup_unit_path: String,
    pub final_unit_state: UnitState,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "step", rename_all = "snake_case")]
pub enum LegacyMigrateResponse {
    Prepare(LegacyMigratePrepareResponse),
    Cutover(LegacyMigrateCutoverResponse),
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn legacy_unit_name_is_fixed() {
        assert_eq!(LEGACY_UNIT_NAME, "sing-box.service");
    }

    #[test]
    fn config_path_kind_uses_snake_case_on_wire() {
        assert_eq!(
            serde_json::to_string(&ConfigPathKind::UserOrEphemeral).unwrap(),
            "\"user_or_ephemeral\""
        );
        assert_eq!(
            serde_json::to_string(&ConfigPathKind::SystemPath).unwrap(),
            "\"system_path\""
        );
    }

    #[test]
    fn observe_response_round_trips() {
        let r = LegacyObserveServiceResponse {
            detected: true,
            unit_name: Some("sing-box.service".into()),
            fragment_path: Some("/etc/systemd/system/sing-box.service".into()),
            unit_file_state: Some("enabled".into()),
            exec_start_raw: Some("/usr/bin/sing-box run -c /etc/sing-box/config.json".into()),
            config_path: Some("/etc/sing-box/config.json".into()),
            config_path_kind: ConfigPathKind::SystemPath,
            unit_state: UnitState::NotFound,
            conflicts_with_managed: false,
        };
        let s = serde_json::to_string(&r).unwrap();
        let back: LegacyObserveServiceResponse = serde_json::from_str(&s).unwrap();
        assert_eq!(back, r);
    }

    #[test]
    fn migrate_request_uses_step_tag() {
        let s = serde_json::to_string(&LegacyMigrateRequest::Prepare).unwrap();
        assert_eq!(s, "{\"step\":\"prepare\"}");
        let s = serde_json::to_string(&LegacyMigrateRequest::Cutover).unwrap();
        assert_eq!(s, "{\"step\":\"cutover\"}");
    }

    #[test]
    fn migrate_response_round_trips_both_arms() {
        let prep = LegacyMigrateResponse::Prepare(LegacyMigratePrepareResponse {
            unit_name: "sing-box.service".into(),
            config_path_was: "/etc/sing-box/config.json".into(),
            config_filename: "config.json".into(),
            config_bytes: vec![1, 2, 3],
            assets: vec![MigratedAsset {
                filename: "geosite.db".into(),
                bytes: vec![4, 5, 6],
            }],
        });
        let s = serde_json::to_string(&prep).unwrap();
        assert!(s.contains("\"step\":\"prepare\""));
        let back: LegacyMigrateResponse = serde_json::from_str(&s).unwrap();
        assert_eq!(back, prep);

        let cut = LegacyMigrateResponse::Cutover(LegacyMigrateCutoverResponse {
            unit_name: "sing-box.service".into(),
            backup_unit_path: "/var/lib/boxpilot/backups/units/sing-box.service-2026-04-29T00-00-00Z"
                .into(),
            final_unit_state: UnitState::NotFound,
        });
        let s = serde_json::to_string(&cut).unwrap();
        assert!(s.contains("\"step\":\"cutover\""));
        let back: LegacyMigrateResponse = serde_json::from_str(&s).unwrap();
        assert_eq!(back, cut);
    }
}
