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

    /// §9.2: total bundle size exceeded the cap.
    #[error("bundle exceeds total size {total} > {limit}")]
    BundleTooLarge { total: u64, limit: u64 },

    /// §9.2: a tar entry violated one of the structural rejection rules.
    #[error("bundle entry rejected: {reason}")]
    BundleEntryRejected { reason: String },

    /// §9.2: an asset's content sha256 did not match the manifest.
    #[error("asset {path} sha256 mismatch vs manifest")]
    BundleAssetMismatch { path: String },

    /// §10 step 7: `<core> check -c config.json` exited non-zero.
    #[error("sing-box check failed (exit {exit}): {stderr_tail}")]
    SingboxCheckFailed { exit: i32, stderr_tail: String },

    /// §7.2: the unit did not reach active/running within the window.
    #[error("activation verify stuck; final state {final_state}")]
    ActivationVerifyStuck { final_state: String },

    /// §10 step 14: rollback path entered but no previous release exists on disk.
    #[error("rollback target missing on disk")]
    RollbackTargetMissing,

    /// §10 step 15: rollback succeeded structurally but the previous release also fails to start.
    #[error("rollback target unstartable; final state {final_state}")]
    RollbackUnstartable { final_state: String },

    /// Daemon startup recovery flagged `/etc/boxpilot/active` as corrupt.
    #[error("/etc/boxpilot/active is corrupt; refusing activation")]
    ActiveCorrupt,

    /// Manual rollback target equals the current active release.
    #[error("requested release is already active")]
    ReleaseAlreadyActive,

    /// Manual rollback target is not present under `/etc/boxpilot/releases/`.
    #[error("release {activation_id} not found")]
    ReleaseNotFound { activation_id: String },
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

    #[test]
    fn new_variants_round_trip() {
        use HelperError::*;
        for v in [
            BundleTooLarge {
                total: 100,
                limit: 50,
            },
            BundleEntryRejected {
                reason: "abs path".into(),
            },
            BundleAssetMismatch {
                path: "geosite.db".into(),
            },
            SingboxCheckFailed {
                exit: 1,
                stderr_tail: "bad rule".into(),
            },
            ActivationVerifyStuck {
                final_state: "NotFound".into(),
            },
            RollbackTargetMissing,
            RollbackUnstartable {
                final_state: "NotFound".into(),
            },
            ActiveCorrupt,
            ReleaseAlreadyActive,
            ReleaseNotFound {
                activation_id: "id".into(),
            },
        ] {
            let s = serde_json::to_string(&v).unwrap();
            let back: HelperError = serde_json::from_str(&s).unwrap();
            assert_eq!(back, v);
        }
    }
}
