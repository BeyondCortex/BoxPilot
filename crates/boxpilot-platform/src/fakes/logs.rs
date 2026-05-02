//! Test double for `LogReader`. Moved verbatim from
//! `boxpilotd::systemd::testing::FixedJournal` so call sites can keep
//! constructing `FixedJournal { lines: ... }` through the re-export shell.

use crate::traits::logs::LogReader;
use async_trait::async_trait;
use boxpilot_ipc::HelperError;

pub struct FixedJournal {
    pub lines: Vec<String>,
}

#[async_trait]
impl LogReader for FixedJournal {
    async fn tail(&self, _: &str, _: u32) -> Result<Vec<String>, HelperError> {
        Ok(self.lines.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn fixed_journal_returns_canned_lines() {
        let j = FixedJournal {
            lines: vec!["a".into(), "b".into()],
        };
        assert_eq!(j.tail("u", 10).await.unwrap(), vec!["a", "b"]);
    }
}
