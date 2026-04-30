//! Indirection over `service::verify::wait_for_running`. Plan #5
//! introduces this trait so `activate.rs` can be unit-tested with a
//! deterministic verifier instead of polling real systemd.

use crate::service::verify::{self, VerifyOutcome};
use crate::systemd::Systemd;
use async_trait::async_trait;
use boxpilot_ipc::HelperResult;
use std::time::Duration;

#[async_trait]
pub trait ServiceVerifier: Send + Sync {
    async fn wait_for_running(
        &self,
        unit_name: &str,
        pre_n_restarts: u32,
        window: Duration,
        systemd: &dyn Systemd,
    ) -> HelperResult<VerifyOutcome>;
}

pub struct DefaultVerifier;

#[async_trait]
impl ServiceVerifier for DefaultVerifier {
    async fn wait_for_running(
        &self,
        unit_name: &str,
        pre_n_restarts: u32,
        window: Duration,
        systemd: &dyn Systemd,
    ) -> HelperResult<VerifyOutcome> {
        verify::wait_for_running(unit_name, pre_n_restarts, window, systemd).await
    }
}

#[cfg(test)]
pub mod testing {
    use super::*;
    use std::sync::Mutex;

    pub struct ScriptedVerifier {
        pub answers: Mutex<Vec<VerifyOutcome>>,
    }

    impl ScriptedVerifier {
        pub fn new(answers: Vec<VerifyOutcome>) -> Self {
            Self {
                answers: Mutex::new(answers),
            }
        }
    }

    #[async_trait]
    impl ServiceVerifier for ScriptedVerifier {
        async fn wait_for_running(
            &self,
            _unit_name: &str,
            _pre_n_restarts: u32,
            _window: Duration,
            _systemd: &dyn Systemd,
        ) -> HelperResult<VerifyOutcome> {
            let mut g = self.answers.lock().unwrap();
            assert!(!g.is_empty(), "ScriptedVerifier exhausted");
            Ok(g.remove(0))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::testing::ScriptedVerifier;
    use super::*;

    #[tokio::test]
    async fn scripted_returns_in_order() {
        let v = ScriptedVerifier::new(vec![
            VerifyOutcome::Running,
            VerifyOutcome::Stuck {
                final_state: boxpilot_ipc::UnitState::NotFound,
            },
        ]);
        let s = crate::systemd::testing::FixedSystemd {
            answer: boxpilot_ipc::UnitState::NotFound,
        };
        let r1 = v
            .wait_for_running("u", 0, Duration::from_millis(1), &s)
            .await
            .unwrap();
        assert_eq!(r1, VerifyOutcome::Running);
        let r2 = v
            .wait_for_running("u", 0, Duration::from_millis(1), &s)
            .await
            .unwrap();
        assert!(matches!(r2, VerifyOutcome::Stuck { .. }));
    }
}
