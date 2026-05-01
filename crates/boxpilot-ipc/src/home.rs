use crate::config::CoreState;
use crate::response::ServiceStatusResponse;
use serde::{Deserialize, Serialize};

pub const HOME_STATUS_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HomeStatusResponse {
    pub schema_version: u32,
    pub service: ServiceStatusResponse,
    #[serde(default)]
    pub active_profile: Option<ActiveProfileSnapshot>,
    pub core: CoreSnapshot,
    pub active_corrupt: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActiveProfileSnapshot {
    pub profile_id: String,
    #[serde(default)]
    pub profile_name: Option<String>,
    pub profile_sha256: String,
    pub release_id: String,
    pub activated_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CoreSnapshot {
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub state: Option<CoreState>,
    pub version: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::response::UnitState;
    use pretty_assertions::assert_eq;

    fn sample_service() -> ServiceStatusResponse {
        ServiceStatusResponse {
            unit_name: "boxpilot-sing-box.service".into(),
            unit_state: UnitState::NotFound,
            controller: crate::response::ControllerStatus::Unset,
            state_schema_mismatch: None,
        }
    }

    #[test]
    fn home_status_round_trips_with_active_profile() {
        let r = HomeStatusResponse {
            schema_version: HOME_STATUS_SCHEMA_VERSION,
            service: sample_service(),
            active_profile: Some(ActiveProfileSnapshot {
                profile_id: "p-1".into(),
                profile_name: Some("Daily".into()),
                profile_sha256: "abc".into(),
                release_id: "rel-1".into(),
                activated_at: "2026-04-30T00:00:00-07:00".into(),
            }),
            core: CoreSnapshot {
                path: Some("/var/lib/boxpilot/cores/current/sing-box".into()),
                state: Some(CoreState::ManagedInstalled),
                version: "1.10.0".into(),
            },
            active_corrupt: false,
        };
        let s = serde_json::to_string(&r).unwrap();
        let back: HomeStatusResponse = serde_json::from_str(&s).unwrap();
        assert_eq!(back, r);
    }

    #[test]
    fn home_status_round_trips_unactivated() {
        let r = HomeStatusResponse {
            schema_version: HOME_STATUS_SCHEMA_VERSION,
            service: sample_service(),
            active_profile: None,
            core: CoreSnapshot {
                path: None,
                state: None,
                version: "unknown".into(),
            },
            active_corrupt: false,
        };
        let s = serde_json::to_string(&r).unwrap();
        let back: HomeStatusResponse = serde_json::from_str(&s).unwrap();
        assert_eq!(back, r);
    }

    #[test]
    fn home_status_active_corrupt_flag_round_trips() {
        let r = HomeStatusResponse {
            schema_version: HOME_STATUS_SCHEMA_VERSION,
            service: sample_service(),
            active_profile: None,
            core: CoreSnapshot {
                path: None,
                state: None,
                version: "unknown".into(),
            },
            active_corrupt: true,
        };
        let s = serde_json::to_string(&r).unwrap();
        let back: HomeStatusResponse = serde_json::from_str(&s).unwrap();
        assert!(back.active_corrupt);
    }
}
