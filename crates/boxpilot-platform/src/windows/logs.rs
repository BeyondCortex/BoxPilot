//! Windows `LogReader` stub. Sub-project #1 returns a sentinel line so a
//! GUI request doesn't crash the helper; the real EventLog-backed impl
//! lands in Sub-project #2.

use crate::traits::logs::LogReader;
use async_trait::async_trait;
use boxpilot_ipc::HelperError;

pub struct EventLogReader;

#[async_trait]
impl LogReader for EventLogReader {
    async fn tail(&self, _unit_name: &str, _lines: u32) -> Result<Vec<String>, HelperError> {
        Ok(vec![
            "log reading not implemented on Windows in Sub-project #1".into(),
        ])
    }
}
