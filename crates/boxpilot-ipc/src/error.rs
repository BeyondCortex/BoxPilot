use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Wire error type returned to the GUI. Concrete strings match spec terminal
/// states (§6.6, §10) so the UI can branch on them deterministically.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Error)]
#[serde(tag = "code", rename_all = "snake_case")]
pub enum HelperError {
    /// Helper method exists but is not implemented in this build (plan #1
    /// returns this for everything except `service.status`).
    #[error("not implemented")]
    NotImplemented,

    #[error("not authorized by polkit")]
    NotAuthorized,

    /// Caller is a local user but is not the controller; mutating actions
    /// are refused.
    #[error("caller is not the controller user")]
    NotController,

    /// `controller_uid` resolves to a UID that no longer exists (§6.6).
    #[error("controller_uid points at a deleted user")]
    ControllerOrphaned,

    /// No controller has been claimed yet and the caller asked for a
    /// mutating action without going through the claim flow.
    #[error("no controller has been initialized")]
    ControllerNotSet,

    /// `boxpilot.toml`'s `schema_version` is unknown to this build.
    #[error("unsupported schema_version: {got}")]
    UnsupportedSchemaVersion { got: u32 },

    /// Could not acquire `/run/boxpilot/lock` — another mutating call is
    /// already in flight.
    #[error("helper busy: another privileged operation is in progress")]
    Busy,

    /// Anything systemd-related — querying a unit, parsing properties, etc.
    #[error("systemd error: {message}")]
    Systemd { message: String },

    /// Anything D-Bus-transport-related not covered above.
    #[error("ipc error: {message}")]
    Ipc { message: String },
}

pub type HelperResult<T> = Result<T, HelperError>;

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn discriminant_matches_spec_terminals() {
        let v = serde_json::to_value(HelperError::ControllerOrphaned).unwrap();
        assert_eq!(v, serde_json::json!({"code": "controller_orphaned"}));
    }

    #[test]
    fn parametric_error_round_trip() {
        let e = HelperError::UnsupportedSchemaVersion { got: 99 };
        let s = serde_json::to_string(&e).unwrap();
        let back: HelperError = serde_json::from_str(&s).unwrap();
        assert_eq!(back, e);
    }
}
