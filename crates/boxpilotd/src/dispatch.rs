//! Single chokepoint every interface method passes through:
//! 1. resolve caller UID from D-Bus connection credentials
//! 2. compute controller status, surface `controller_orphaned` (§6.6)
//! 3. ask polkit for authorization
//! 4. for mutating calls, acquire `/run/boxpilot/lock`
//! 5. invoke the action body
//!
//! Step 5 is generic over the action body so each interface method stays
//! a 1-2 line wrapper. When `controller == Unset` and the call is mutating
//! and polkit allowed it, `AuthorizedCall::will_claim_controller` is set
//! so the body can atomically claim the controller under the acquired lock.

use crate::context::HelperContext;
use crate::controller::ControllerState;
use crate::lock::{self, LockGuard};
use boxpilot_ipc::{HelperError, HelperMethod, HelperResult};

pub struct AuthorizedCall {
    pub caller_uid: u32,
    pub controller: ControllerState,
    /// True when the body should atomically claim the controller under the
    /// lock it holds.  Set only when `controller == Unset`, the call is
    /// mutating, and polkit allowed it.  Wired in Task 13 (`maybe_claim_controller`).
    pub will_claim_controller: bool,
    /// Held only when [`HelperMethod::is_mutating`] is true.
    _lock: Option<LockGuard>,
}

pub async fn authorize(
    ctx: &HelperContext,
    sender_bus_name: &str,
    method: HelperMethod,
) -> HelperResult<AuthorizedCall> {
    let caller_uid = ctx.callers.resolve(sender_bus_name).await?;
    let controller = ctx.controller_state().await?;

    if let ControllerState::Orphaned { .. } = controller {
        // Read-only methods are still allowed; mutating ones are blocked
        // until controller.transfer succeeds (§6.6).
        if method.is_mutating() {
            return Err(HelperError::ControllerOrphaned);
        }
    }

    // Spec §7.6: refuse mutating verbs when `install-state.json`'s
    // schema_version is incompatible with the compiled-in version. Mirrors
    // the orphan-controller pattern above — read-only verbs still succeed
    // so the GUI can surface the mismatch via service.status.
    if let Some(got) = ctx.state_schema_mismatch {
        if method.is_mutating() {
            return Err(HelperError::UnsupportedSchemaVersion { got });
        }
    }

    let action_id = method.polkit_action_id();
    let allowed = ctx.authority.check(action_id, sender_bus_name).await?;
    if !allowed {
        return Err(HelperError::NotAuthorized);
    }

    let will_claim_controller =
        matches!(controller, ControllerState::Unset) && method.is_mutating() && allowed;

    let lock = if method.is_mutating() {
        Some(lock::try_acquire(&ctx.paths.run_lock())?)
    } else {
        None
    };

    Ok(AuthorizedCall {
        caller_uid,
        controller,
        will_claim_controller,
        _lock: lock,
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ControllerWrites {
    pub uid: u32,
    pub username: String,
}

/// If `will_claim` is true, look up the caller's username and produce the
/// payload the body needs to write atomically (boxpilot.toml's
/// controller_uid + /etc/boxpilot/controller-name).
pub fn maybe_claim_controller(
    will_claim: bool,
    caller_uid: u32,
    user_lookup: &dyn crate::controller::UserLookup,
) -> HelperResult<Option<ControllerWrites>> {
    if !will_claim {
        return Ok(None);
    }
    match user_lookup.lookup_username(caller_uid) {
        Some(username) => Ok(Some(ControllerWrites {
            uid: caller_uid,
            username,
        })),
        None => Err(HelperError::ControllerOrphaned),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::authority::testing::CannedAuthority;
    use crate::context::testing::ctx_with;
    use boxpilot_ipc::UnitState;
    use tempfile::tempdir;

    #[tokio::test]
    async fn read_only_call_with_polkit_yes_succeeds() {
        let tmp = tempdir().unwrap();
        let ctx = ctx_with(
            &tmp,
            None,
            CannedAuthority::allowing(&["app.boxpilot.helper.service.status"]),
            UnitState::NotFound,
            &[(":1.42", 1000)],
        );
        let r = authorize(&ctx, ":1.42", HelperMethod::ServiceStatus).await;
        assert!(r.is_ok());
    }

    #[tokio::test]
    async fn read_only_call_with_polkit_no_returns_not_authorized() {
        let tmp = tempdir().unwrap();
        let ctx = ctx_with(
            &tmp,
            None,
            CannedAuthority::denying(&["app.boxpilot.helper.service.status"]),
            UnitState::NotFound,
            &[(":1.42", 1000)],
        );
        let r = authorize(&ctx, ":1.42", HelperMethod::ServiceStatus).await;
        assert!(matches!(r, Err(HelperError::NotAuthorized)));
    }

    #[tokio::test]
    async fn mutating_call_without_controller_signals_will_claim() {
        let tmp = tempdir().unwrap();
        let ctx = ctx_with(
            &tmp,
            None,
            CannedAuthority::allowing(&["app.boxpilot.helper.service.start"]),
            UnitState::NotFound,
            &[(":1.42", 1000)],
        );
        let call = authorize(&ctx, ":1.42", HelperMethod::ServiceStart)
            .await
            .unwrap();
        assert!(call.will_claim_controller);
    }

    #[tokio::test]
    async fn mutating_call_with_orphaned_controller_returns_orphaned() {
        let tmp = tempdir().unwrap();
        // 4_000_000_000 is virtually guaranteed not to map to a real user.
        let ctx = ctx_with(
            &tmp,
            Some("schema_version = 1\ncontroller_uid = 4000000000\n"),
            CannedAuthority::allowing(&["app.boxpilot.helper.service.start"]),
            UnitState::NotFound,
            &[(":1.42", 1000)],
        );
        let r = authorize(&ctx, ":1.42", HelperMethod::ServiceStart).await;
        assert!(matches!(r, Err(HelperError::ControllerOrphaned)));
    }

    #[tokio::test]
    async fn mutating_call_with_state_schema_mismatch_returns_unsupported_schema_version() {
        let tmp = tempdir().unwrap();
        let mut ctx = ctx_with(
            &tmp,
            None,
            CannedAuthority::allowing(&["app.boxpilot.helper.service.start"]),
            UnitState::NotFound,
            &[(":1.42", 1000)],
        );
        ctx.state_schema_mismatch = Some(99);
        let r = authorize(&ctx, ":1.42", HelperMethod::ServiceStart).await;
        assert!(matches!(
            r,
            Err(HelperError::UnsupportedSchemaVersion { got: 99 })
        ));
    }

    #[tokio::test]
    async fn read_only_call_with_state_schema_mismatch_still_succeeds() {
        let tmp = tempdir().unwrap();
        let mut ctx = ctx_with(
            &tmp,
            None,
            CannedAuthority::allowing(&["app.boxpilot.helper.service.status"]),
            UnitState::NotFound,
            &[(":1.42", 1000)],
        );
        ctx.state_schema_mismatch = Some(99);
        // Read-only verbs must still work so the GUI can fetch service.status
        // and surface the mismatch via its `state_schema_mismatch` field.
        let r = authorize(&ctx, ":1.42", HelperMethod::ServiceStatus).await;
        assert!(r.is_ok());
    }

    #[tokio::test]
    async fn read_only_call_with_orphaned_controller_still_succeeds() {
        let tmp = tempdir().unwrap();
        let ctx = ctx_with(
            &tmp,
            Some("schema_version = 1\ncontroller_uid = 4000000000\n"),
            CannedAuthority::allowing(&["app.boxpilot.helper.service.status"]),
            UnitState::NotFound,
            &[(":1.42", 1000)],
        );
        let r = authorize(&ctx, ":1.42", HelperMethod::ServiceStatus).await;
        assert!(r.is_ok());
    }

    #[test]
    fn no_claim_returns_none() {
        use crate::controller::testing::Fixed;
        let lookup = Fixed::new(&[(1000, "alice")]);
        let r = maybe_claim_controller(false, 1000, &lookup).unwrap();
        assert!(r.is_none());
    }

    #[test]
    fn claim_with_known_user_returns_writes() {
        use crate::controller::testing::Fixed;
        let lookup = Fixed::new(&[(1000, "alice")]);
        let r = maybe_claim_controller(true, 1000, &lookup).unwrap();
        assert_eq!(
            r.unwrap(),
            ControllerWrites {
                uid: 1000,
                username: "alice".into()
            }
        );
    }

    #[test]
    fn claim_with_unknown_user_errors_orphaned() {
        use crate::controller::testing::Fixed;
        let lookup = Fixed::new(&[]);
        let r = maybe_claim_controller(true, 1000, &lookup);
        assert!(matches!(r, Err(HelperError::ControllerOrphaned)));
    }
}
