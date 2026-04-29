use serde::{Deserialize, Serialize};

use crate::response::UnitState;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServiceControlResponse {
    pub unit_state: UnitState,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServiceInstallManagedResponse {
    pub unit_state: UnitState,
    pub generated_unit_path: String,
    pub claimed_controller: bool,
}

/// `lines` is clamped to `1..=1000` by the helper before invoking journalctl.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServiceLogsRequest {
    pub lines: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServiceLogsResponse {
    pub lines: Vec<String>,
    /// True when the helper clamped the requested count down to the cap.
    pub truncated: bool,
}

pub const SERVICE_LOGS_MAX_LINES: u32 = 1000;
pub const SERVICE_LOGS_DEFAULT_LINES: u32 = 200;

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn control_response_round_trip() {
        let r = ServiceControlResponse {
            unit_state: UnitState::NotFound,
        };
        let s = serde_json::to_string(&r).unwrap();
        let back: ServiceControlResponse = serde_json::from_str(&s).unwrap();
        assert_eq!(back, r);
    }

    #[test]
    fn install_response_round_trip() {
        let r = ServiceInstallManagedResponse {
            unit_state: UnitState::NotFound,
            generated_unit_path: "/etc/systemd/system/boxpilot-sing-box.service".into(),
            claimed_controller: false,
        };
        let s = serde_json::to_string(&r).unwrap();
        let back: ServiceInstallManagedResponse = serde_json::from_str(&s).unwrap();
        assert_eq!(back, r);
    }

    #[test]
    fn logs_request_round_trip() {
        let r = ServiceLogsRequest { lines: 100 };
        let s = serde_json::to_string(&r).unwrap();
        let back: ServiceLogsRequest = serde_json::from_str(&s).unwrap();
        assert_eq!(back, r);
    }

    #[test]
    fn logs_response_round_trip() {
        let r = ServiceLogsResponse {
            lines: vec!["line1".into(), "line2".into()],
            truncated: false,
        };
        let s = serde_json::to_string(&r).unwrap();
        let back: ServiceLogsResponse = serde_json::from_str(&s).unwrap();
        assert_eq!(back, r);
    }

    // Compile-time check: the default is at or below the cap. Clippy's
    // `assertions_on_constants` lint correctly flags a runtime `assert!`
    // here as wasted work, so we hoist it into a `const _` block where
    // it's evaluated by rustc before the test ever runs.
    const _: () = assert!(SERVICE_LOGS_DEFAULT_LINES <= SERVICE_LOGS_MAX_LINES);

    #[test]
    fn cap_constants_match_spec() {
        assert_eq!(SERVICE_LOGS_MAX_LINES, 1000);
    }
}
