use crate::UnitState;
use serde::{Deserialize, Serialize};

pub const LEGACY_UNIT_NAME: &str = "sing-box.service";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConfigPathKind {
    /// Path is under /etc, /usr, /var/lib, /var/cache, /opt, or /srv — safe
    /// to keep referencing as a system service config.
    SystemPath,
    /// Path is under /home, /tmp, /run/user, /var/tmp — refuse migration
    /// (spec §8 / §9.3).
    UserOrEphemeral,
    /// ExecStart did not contain a parseable -c/--config flag, or no path
    /// was extracted. The GUI must prompt the user to pick a profile manually.
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LegacyObserveServiceResponse {
    /// `false` when the unit is not loaded by systemd at all.
    pub detected: bool,
    /// Always `LEGACY_UNIT_NAME` when detected; carried in the response so
    /// future expansions can probe other names without changing the shape.
    #[serde(default)]
    pub unit_name: Option<String>,
    /// `org.freedesktop.systemd1.Unit::FragmentPath`. None when the unit is
    /// transient or its fragment was deleted.
    #[serde(default)]
    pub fragment_path: Option<String>,
    /// systemctl's `enabled / disabled / static / masked / not-found` view.
    /// None when the manager refused to report it.
    #[serde(default)]
    pub unit_file_state: Option<String>,
    /// Raw `ExecStart=` line (first one, after expansion) as read from
    /// `fragment_path`. None when the fragment has no ExecStart.
    #[serde(default)]
    pub exec_start_raw: Option<String>,
    /// Path extracted from `-c` / `--config` in `exec_start_raw`.
    #[serde(default)]
    pub config_path: Option<String>,
    pub config_path_kind: ConfigPathKind,
    pub unit_state: UnitState,
    /// `true` when `unit_name == cfg.target_service`, i.e. the legacy
    /// "sing-box.service" name happens to coincide with what BoxPilot
    /// already manages. Only relevant if a future deployment changes
    /// `target_service`; today this is always `false`.
    pub conflicts_with_managed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "step", rename_all = "snake_case")]
pub enum LegacyMigrateRequest {
    /// Read fragment + config + sibling assets from disk and return them.
    /// No system mutation. Refused if `config_path` is UserOrEphemeral.
    Prepare,
    /// Stop + disable the legacy unit, back up its fragment. The next
    /// `profile.activate_bundle` will then enable + start
    /// `boxpilot-sing-box.service` as part of the standard pipeline.
    Cutover,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MigratedAsset {
    /// Filename as it appeared next to the legacy config (no nested dirs).
    pub filename: String,
    /// Bytes; serde encodes as a JSON array of u8 — matches the existing
    /// per-file BUNDLE_MAX_FILE_BYTES cap (16 MiB).
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LegacyMigratePrepareResponse {
    pub unit_name: String,
    pub config_path_was: String,
    pub config_filename: String,
    /// The bytes of the legacy config file. The user-side Vue layer hands
    /// these to `boxpilot_profile::import_local_file` (or the dir variant
    /// when `assets` is non-empty).
    pub config_bytes: Vec<u8>,
    pub assets: Vec<MigratedAsset>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LegacyMigrateCutoverResponse {
    pub unit_name: String,
    pub backup_unit_path: String,
    /// Post-cutover state of the legacy unit. Should normally be Inactive
    /// or NotFound (after disable, GetUnit may report NoSuchUnit).
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
        assert_eq!(
            serde_json::to_string(&ConfigPathKind::Unknown).unwrap(),
            "\"unknown\""
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
            backup_unit_path:
                "/var/lib/boxpilot/backups/units/sing-box.service-2026-04-29T00-00-00Z".into(),
            final_unit_state: UnitState::NotFound,
        });
        let s = serde_json::to_string(&cut).unwrap();
        assert!(s.contains("\"step\":\"cutover\""));
        let back: LegacyMigrateResponse = serde_json::from_str(&s).unwrap();
        assert_eq!(back, cut);
    }
}
