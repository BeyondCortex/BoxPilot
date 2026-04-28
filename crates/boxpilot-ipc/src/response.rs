use serde::{Deserialize, Serialize};

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
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServiceStatusResponse {
    pub unit_name: String, // "boxpilot-sing-box.service"
    pub unit_state: UnitState,
    /// Snapshot of `controller_uid` resolution at call time. Useful for the
    /// Home page to surface `controller_orphaned` (§6.6) without a second RTT.
    pub controller: ControllerStatus,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum ControllerStatus {
    Unset,
    Set { uid: u32, username: String },
    Orphaned { uid: u32 },
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
