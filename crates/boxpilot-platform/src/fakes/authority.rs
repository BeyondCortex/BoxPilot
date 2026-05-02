//! Test doubles for `Authority`. Cross-platform so helper-side unit tests
//! pass on the Windows runner (AC4).

use crate::traits::authority::{Authority, CallerPrincipal};
use async_trait::async_trait;
use boxpilot_ipc::HelperError;
use std::collections::HashMap;
use std::sync::Mutex;

/// `CannedAuthority` answers `check` with a pre-seeded yes/no per action_id.
/// Querying an unconfigured action_id returns `Err(HelperError::Ipc { .. })`
/// so a missed-test setup is loud rather than silently allowed.
pub struct CannedAuthority(pub Mutex<HashMap<String, bool>>);

impl CannedAuthority {
    pub fn allowing(actions: &[&str]) -> Self {
        Self(Mutex::new(
            actions.iter().map(|a| (a.to_string(), true)).collect(),
        ))
    }
    pub fn denying(actions: &[&str]) -> Self {
        Self(Mutex::new(
            actions.iter().map(|a| (a.to_string(), false)).collect(),
        ))
    }
}

#[async_trait]
impl Authority for CannedAuthority {
    async fn check(
        &self,
        action_id: &str,
        _principal: &CallerPrincipal,
    ) -> Result<bool, HelperError> {
        let map = self.0.lock().unwrap();
        map.get(action_id).copied().ok_or_else(|| HelperError::Ipc {
            message: format!("test: unconfigured action {action_id}"),
        })
    }
}

/// `AlwaysAllow` is the trivial Authority — every check returns `Ok(true)`.
/// Used by COQ3 fixtures where a caller-uid map and per-action seeding is
/// noise; the test only cares that dispatch reaches the body.
pub struct AlwaysAllow;

#[async_trait]
impl Authority for AlwaysAllow {
    async fn check(
        &self,
        _action_id: &str,
        _principal: &CallerPrincipal,
    ) -> Result<bool, HelperError> {
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn canned_allow() {
        let a = CannedAuthority::allowing(&["app.boxpilot.helper.service.status"]);
        assert!(a
            .check(
                "app.boxpilot.helper.service.status",
                &CallerPrincipal::LinuxUid(1000),
            )
            .await
            .unwrap());
    }

    #[tokio::test]
    async fn canned_deny() {
        let a = CannedAuthority::denying(&["app.boxpilot.helper.service.start"]);
        assert!(!a
            .check(
                "app.boxpilot.helper.service.start",
                &CallerPrincipal::LinuxUid(1000),
            )
            .await
            .unwrap());
    }

    #[tokio::test]
    async fn always_allow_returns_true_for_anything() {
        let a = AlwaysAllow;
        assert!(a
            .check("anything", &CallerPrincipal::LinuxUid(0))
            .await
            .unwrap());
    }
}
