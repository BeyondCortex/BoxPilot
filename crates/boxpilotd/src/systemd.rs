//! Re-export shell. Production impl lives in
//! `boxpilot-platform::linux::service`. The trait is renamed
//! `ServiceManager`; `Systemd` is a backwards-compat alias.
//!
//! `JournalReader` + `JournalctlProcess` stay inline here; they move to
//! `boxpilot-platform::{traits,linux}::logs` in PR 5 task 5.2.

pub use boxpilot_platform::linux::service::DBusSystemd;
pub use boxpilot_platform::traits::service::{ServiceManager, Systemd};

#[cfg(test)]
pub mod testing {
    pub use boxpilot_platform::fakes::service::*;

    use super::*;
    use async_trait::async_trait;
    use boxpilot_ipc::HelperError;

    pub struct FixedJournal {
        pub lines: Vec<String>,
    }

    #[async_trait]
    impl JournalReader for FixedJournal {
        async fn tail(&self, _: &str, _: u32) -> Result<Vec<String>, HelperError> {
            Ok(self.lines.clone())
        }
    }

    #[tokio::test]
    async fn fixed_journal_returns_canned_lines() {
        let j = FixedJournal {
            lines: vec!["a".into(), "b".into()],
        };
        assert_eq!(j.tail("u", 10).await.unwrap(), vec!["a", "b"]);
    }
}

// ---- KEEP for Task 5.2: JournalReader trait + JournalctlProcess ----

use async_trait::async_trait;
use boxpilot_ipc::HelperError;

#[async_trait]
pub trait JournalReader: Send + Sync {
    /// Return the last `lines` journal entries for `unit_name`. Caller is
    /// responsible for clamping `lines` to a sane upper bound; this trait
    /// passes through whatever it gets.
    async fn tail(&self, unit_name: &str, lines: u32) -> Result<Vec<String>, HelperError>;
}

pub struct JournalctlProcess;

#[async_trait]
impl JournalReader for JournalctlProcess {
    async fn tail(&self, unit_name: &str, lines: u32) -> Result<Vec<String>, HelperError> {
        let n_str = lines.to_string();
        let out = tokio::process::Command::new("journalctl")
            .arg("--no-pager")
            .arg("-u")
            .arg(unit_name)
            .arg("-n")
            .arg(&n_str)
            // --output=short keeps the format `Apr 28 12:34:56 host unit[pid]: msg`
            // which is what `journalctl` defaults to anyway, but pinning it makes
            // the format stable across distros that change defaults.
            .arg("--output=short")
            .output()
            .await
            .map_err(|e| HelperError::Ipc {
                message: format!("spawn journalctl: {e}"),
            })?;
        if !out.status.success() {
            return Err(HelperError::Ipc {
                message: format!(
                    "journalctl exit {:?}: {}",
                    out.status.code(),
                    String::from_utf8_lossy(&out.stderr).trim()
                ),
            });
        }
        let text = String::from_utf8_lossy(&out.stdout);
        Ok(text.lines().map(|l| l.to_string()).collect())
    }
}
