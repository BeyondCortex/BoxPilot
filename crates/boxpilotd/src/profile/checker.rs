//! Spec §10 step 7: `<core_path> check -c config.json` from inside the
//! release working directory. Trait-wrapped so activate.rs can run a
//! deterministic checker in unit tests.

use async_trait::async_trait;
use boxpilot_ipc::{HelperError, HelperResult};
use std::path::Path;
use tokio::process::Command;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckOutcome {
    pub exit: i32,
    pub stderr_tail: String,
}

#[async_trait]
pub trait SingboxChecker: Send + Sync {
    async fn check(&self, core_path: &Path, working_dir: &Path) -> HelperResult<CheckOutcome>;
}

pub struct ProcessChecker;

#[async_trait]
impl SingboxChecker for ProcessChecker {
    async fn check(&self, core_path: &Path, working_dir: &Path) -> HelperResult<CheckOutcome> {
        let output = Command::new(core_path)
            .arg("check")
            .arg("-c")
            .arg("config.json")
            .current_dir(working_dir)
            .output()
            .await
            .map_err(|e| HelperError::SingboxCheckFailed {
                exit: -1,
                stderr_tail: format!("spawn: {e}"),
            })?;
        let exit = output.status.code().unwrap_or(-1);
        let stderr = String::from_utf8_lossy(&output.stderr);
        let tail: String = stderr
            .chars()
            .rev()
            .take(256)
            .collect::<String>()
            .chars()
            .rev()
            .collect();
        Ok(CheckOutcome {
            exit,
            stderr_tail: redact_secrets(&tail),
        })
    }
}

/// Best-effort scrub of the stderr tail before we hand it back to the
/// caller. Schema-aware redaction (the §14 walker) only applies to JSON;
/// stderr is text-only, so a heuristic line-drop stays the right call here.
/// The shared implementation lives in [`crate::diagnostics::bundle::redact_journal_lines`].
fn redact_secrets(s: &str) -> String {
    crate::diagnostics::bundle::redact_journal_lines(s)
}

#[cfg(test)]
pub mod testing {
    use super::*;
    use std::sync::Mutex;

    pub struct FakeChecker {
        outcomes: Mutex<Vec<CheckOutcome>>,
    }

    impl FakeChecker {
        pub fn new(outcomes: Vec<CheckOutcome>) -> Self {
            Self {
                outcomes: Mutex::new(outcomes),
            }
        }
        pub fn ok() -> Self {
            Self::new(vec![CheckOutcome {
                exit: 0,
                stderr_tail: String::new(),
            }])
        }
        pub fn fail() -> Self {
            Self::new(vec![CheckOutcome {
                exit: 1,
                stderr_tail: "bad rule".into(),
            }])
        }
    }

    #[async_trait]
    impl SingboxChecker for FakeChecker {
        async fn check(&self, _core: &Path, _wd: &Path) -> HelperResult<CheckOutcome> {
            let mut g = self.outcomes.lock().unwrap();
            assert!(!g.is_empty(), "FakeChecker exhausted");
            Ok(g.remove(0))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redact_drops_password_lines() {
        let s = "ok line 1\npassword=hunter2\nok line 2";
        assert_eq!(redact_secrets(s), "ok line 1\nok line 2");
    }

    #[test]
    fn redact_drops_uuid_lines() {
        let s = "uuid=abc-def\ngood";
        assert_eq!(redact_secrets(s), "good");
    }

    #[test]
    fn redact_drops_private_key_lines() {
        let s = "private_key:foo\nstuff";
        assert_eq!(redact_secrets(s), "stuff");
    }
}
