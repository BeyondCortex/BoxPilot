//! Single chokepoint every interface method passes through:
//! 1. resolve caller UID from D-Bus connection credentials
//! 2. compute controller status, surface `controller_orphaned` (§6.6)
//! 3. for mutating calls without a controller, refuse (`ControllerNotSet`)
//! 4. ask polkit for authorization
//! 5. for mutating calls, acquire `/run/boxpilot/lock`
//! 6. invoke the action body
//!
//! Step 6 is generic over the action body so each interface method stays
//! a 1-2 line wrapper.

use crate::context::HelperContext;
use crate::controller::ControllerState;
use crate::lock::{self, LockGuard};
use boxpilot_ipc::{HelperError, HelperMethod, HelperResult};

pub struct AuthorizedCall {
    #[allow(dead_code)] // used in plan #2 (controller ownership checks)
    pub caller_uid: u32,
    #[allow(dead_code)] // used in plan #2 (controller state propagation)
    pub controller: ControllerState,
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

    if method.is_mutating() && matches!(controller, ControllerState::Unset) {
        // Plan #1 ships no path that would set the controller, so this
        // branch is reachable only by a non-status mutating method
        // (all of which return NotImplemented here). Keeping the check
        // wired makes the dispatch contract correct for plan #2 onward.
        return Err(HelperError::ControllerNotSet);
    }

    let action_id = method.polkit_action_id();
    let allowed = ctx.authority.check(&action_id, sender_bus_name).await?;
    if !allowed {
        return Err(HelperError::NotAuthorized);
    }

    let lock = if method.is_mutating() {
        Some(lock::try_acquire(&ctx.paths.run_lock())?)
    } else {
        None
    };

    Ok(AuthorizedCall { caller_uid, controller, _lock: lock })
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
    async fn mutating_call_without_controller_returns_controller_not_set() {
        let tmp = tempdir().unwrap();
        let ctx = ctx_with(
            &tmp,
            None,
            CannedAuthority::allowing(&["app.boxpilot.helper.service.start"]),
            UnitState::NotFound,
            &[(":1.42", 1000)],
        );
        let r = authorize(&ctx, ":1.42", HelperMethod::ServiceStart).await;
        assert!(matches!(r, Err(HelperError::ControllerNotSet)));
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
}
