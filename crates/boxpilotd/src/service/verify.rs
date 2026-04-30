//! Spec §7.2 runtime verification helper. Polls the unit until it
//! reaches active/running with `n_restarts` unchanged from the
//! pre-operation snapshot, or the deadline elapses.

use crate::systemd::Systemd;
use boxpilot_ipc::{HelperResult, UnitState};
use std::time::{Duration, Instant};

/// Default per spec §7.2: 5 seconds.
pub const DEFAULT_WINDOW: Duration = Duration::from_secs(5);
/// Spec §7.2 cap.
pub const MAX_WINDOW: Duration = Duration::from_secs(30);
/// Polling cadence inside the window.
pub const POLL_INTERVAL: Duration = Duration::from_millis(250);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VerifyOutcome {
    Running,
    Stuck { final_state: UnitState },
    NotFound,
}

pub async fn wait_for_running(
    unit_name: &str,
    pre_n_restarts: u32,
    window: Duration,
    systemd: &dyn Systemd,
) -> HelperResult<VerifyOutcome> {
    let window = window.min(MAX_WINDOW);
    let deadline = Instant::now() + window;
    loop {
        let state = systemd.unit_state(unit_name).await?;
        match &state {
            UnitState::NotFound => return Ok(VerifyOutcome::NotFound),
            UnitState::Known {
                active_state,
                sub_state,
                n_restarts,
                ..
            } if active_state == "active"
                && sub_state == "running"
                && *n_restarts == pre_n_restarts =>
            {
                return Ok(VerifyOutcome::Running);
            }
            _ => {}
        }
        if Instant::now() >= deadline {
            return Ok(VerifyOutcome::Stuck { final_state: state });
        }
        tokio::time::sleep(POLL_INTERVAL).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::systemd::testing::FixedSystemd;

    #[tokio::test]
    async fn returns_running_when_state_already_active() {
        let s = FixedSystemd {
            answer: UnitState::Known {
                active_state: "active".into(),
                sub_state: "running".into(),
                load_state: "loaded".into(),
                n_restarts: 0,
                exec_main_status: 0,
            },
        };
        let o = wait_for_running("u", 0, Duration::from_millis(50), &s)
            .await
            .unwrap();
        assert_eq!(o, VerifyOutcome::Running);
    }

    #[tokio::test]
    async fn returns_stuck_when_state_never_reaches_running_within_window() {
        let s = FixedSystemd {
            answer: UnitState::Known {
                active_state: "activating".into(),
                sub_state: "start".into(),
                load_state: "loaded".into(),
                n_restarts: 0,
                exec_main_status: 0,
            },
        };
        let o = wait_for_running("u", 0, Duration::from_millis(100), &s)
            .await
            .unwrap();
        assert!(matches!(o, VerifyOutcome::Stuck { .. }));
    }

    #[tokio::test]
    async fn returns_not_found_when_unit_missing() {
        let s = FixedSystemd {
            answer: UnitState::NotFound,
        };
        let o = wait_for_running("u", 0, Duration::from_millis(50), &s)
            .await
            .unwrap();
        assert_eq!(o, VerifyOutcome::NotFound);
    }

    #[tokio::test]
    async fn restarts_diff_from_pre_means_stuck_even_if_active() {
        // n_restarts incremented since pre-op → service crashed-then-relaunched
        // inside the window; treat as stuck rather than success (§7.2).
        let s = FixedSystemd {
            answer: UnitState::Known {
                active_state: "active".into(),
                sub_state: "running".into(),
                load_state: "loaded".into(),
                n_restarts: 3,
                exec_main_status: 0,
            },
        };
        let o = wait_for_running("u", 0, Duration::from_millis(50), &s)
            .await
            .unwrap();
        assert!(matches!(o, VerifyOutcome::Stuck { .. }));
    }
}
