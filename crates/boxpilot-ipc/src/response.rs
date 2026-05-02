use serde::{Deserialize, Serialize};

/// Platform-specific SCM fields that don't fit the systemd-shaped `UnitState`
/// surface. The Linux variant carries no extra data because the existing fields
/// (`sub_state`, `load_state`, `n_restarts`, `exec_main_status`) already cover
/// the systemd shape. The Windows variant carries SCM-specific status fields.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "platform", rename_all = "lowercase")]
pub enum PlatformUnitExtra {
    /// Linux/systemd: existing UnitState fields (sub_state, load_state, n_restarts,
    /// exec_main_status) already cover the systemd shape, so this variant carries
    /// no extra data.
    Linux,
    /// Windows/SCM: SCM-specific status fields not representable in the
    /// systemd-shaped surface.
    Windows {
        /// SERVICE_STATUS_PROCESS::dwCheckPoint
        check_point: u32,
        /// SERVICE_STATUS_PROCESS::dwWaitHint (milliseconds)
        wait_hint_ms: u32,
        /// SERVICE_STATUS_PROCESS::dwControlsAccepted (bitmask of SERVICE_ACCEPT_*)
        controls_accepted: u32,
    },
}

fn default_platform_extra() -> PlatformUnitExtra {
    PlatformUnitExtra::Linux
}

/// Mirrors `systemctl show` `ActiveState`/`SubState`/`LoadState`/`NRestarts`
/// fields plus a sentinel for "the unit doesn't exist".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum UnitState {
    /// `systemctl` doesn't know about `boxpilot-sing-box.service` at all.
    NotFound,
    Known {
        active_state: String, // active | inactive | failed | activating | reloading | deactivating
        sub_state: String,    // running | dead | start-pre | failed | …
        load_state: String,   // loaded | not-found | error | masked | …
        n_restarts: u32,
        exec_main_status: i32,
        #[serde(default = "default_platform_extra")]
        platform_extra: PlatformUnitExtra,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServiceStatusResponse {
    pub unit_name: String, // "boxpilot-sing-box.service"
    pub unit_state: UnitState,
    /// Snapshot of `controller_uid` resolution at call time. Useful for the
    /// Home page to surface `controller_orphaned` (§6.6) without a second RTT.
    pub controller: ControllerStatus,
    /// Spec §7.6 startup recovery: the schema_version the daemon found in
    /// `install-state.json` at startup if it did not match the compiled-in
    /// `INSTALL_STATE_SCHEMA_VERSION`. `None` for the matching case (also
    /// when the file is missing, which is the fresh-install state). When
    /// `Some`, `dispatch::authorize` short-circuits all mutating verbs with
    /// `UnsupportedSchemaVersion`, so the GUI can surface a single banner
    /// rather than collecting per-action errors.
    #[serde(default)]
    pub state_schema_mismatch: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum ControllerStatus {
    Unset,
    Set { uid: u32, username: String },
    Orphaned { uid: u32 },
}

#[cfg(test)]
mod platform_extra_tests {
    use super::*;
    use serde_json;

    #[test]
    fn known_state_round_trips_with_linux_extra() {
        let s = UnitState::Known {
            active_state: "active".into(),
            sub_state: "running".into(),
            load_state: "loaded".into(),
            n_restarts: 3,
            exec_main_status: 0,
            platform_extra: PlatformUnitExtra::Linux,
        };
        let json = serde_json::to_string(&s).unwrap();
        let back: UnitState = serde_json::from_str(&json).unwrap();
        assert_eq!(s, back);
    }

    #[test]
    fn known_state_round_trips_with_windows_extra() {
        let s = UnitState::Known {
            active_state: "active".into(),
            sub_state: "running".into(),
            load_state: "loaded".into(),
            n_restarts: 0,
            exec_main_status: 0,
            platform_extra: PlatformUnitExtra::Windows {
                check_point: 0,
                wait_hint_ms: 30000,
                controls_accepted: 0x0000_0001, // SERVICE_ACCEPT_STOP
            },
        };
        let json = serde_json::to_string(&s).unwrap();
        let back: UnitState = serde_json::from_str(&json).unwrap();
        assert_eq!(s, back);
    }

    #[test]
    fn known_state_deserializes_payload_without_platform_extra() {
        // Forward-compat: old IPC clients (pre-PR 1.1) don't send platform_extra.
        // The #[serde(default)] attribute on the field must keep this working.
        let old = r#"{"kind":"known","active_state":"active","sub_state":"running","load_state":"loaded","n_restarts":0,"exec_main_status":0}"#;
        let s: UnitState = serde_json::from_str(old).unwrap();
        match s {
            UnitState::Known { platform_extra, .. } => {
                assert_eq!(platform_extra, PlatformUnitExtra::Linux);
            }
            _ => panic!("expected Known"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn unit_state_not_found_serialization() {
        let v = serde_json::to_value(UnitState::NotFound).unwrap();
        assert_eq!(v, serde_json::json!({"kind": "not_found"}));
    }

    #[test]
    fn unit_state_known_round_trip() {
        let s = UnitState::Known {
            active_state: "active".into(),
            sub_state: "running".into(),
            load_state: "loaded".into(),
            n_restarts: 0,
            exec_main_status: 0,
            platform_extra: PlatformUnitExtra::Linux,
        };
        let json = serde_json::to_string(&s).unwrap();
        let back: UnitState = serde_json::from_str(&json).unwrap();
        assert_eq!(back, s);
    }

    #[test]
    fn controller_status_orphaned_round_trip() {
        let c = ControllerStatus::Orphaned { uid: 1500 };
        let json = serde_json::to_string(&c).unwrap();
        let back: ControllerStatus = serde_json::from_str(&json).unwrap();
        assert_eq!(back, c);
    }
}
