//! `service.logs` (§6.3): bounded journalctl tail for the managed unit.

use crate::systemd::JournalReader;
use boxpilot_ipc::{
    HelperResult, ServiceLogsRequest, ServiceLogsResponse, SERVICE_LOGS_DEFAULT_LINES,
    SERVICE_LOGS_MAX_LINES,
};

pub async fn read(
    req: &ServiceLogsRequest,
    unit_name: &str,
    journal: &dyn JournalReader,
) -> HelperResult<ServiceLogsResponse> {
    let requested = if req.lines == 0 {
        SERVICE_LOGS_DEFAULT_LINES
    } else {
        req.lines
    };
    let clamped = requested.min(SERVICE_LOGS_MAX_LINES);
    let truncated = clamped < requested;
    let lines = journal.tail(unit_name, clamped).await?;
    Ok(ServiceLogsResponse { lines, truncated })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::systemd::testing::FixedJournal;

    #[tokio::test]
    async fn zero_request_uses_default() {
        let j = FixedJournal {
            lines: vec!["a".into()],
        };
        let r = read(&ServiceLogsRequest { lines: 0 }, "u", &j).await.unwrap();
        assert_eq!(r.lines, vec!["a".to_string()]);
        assert!(!r.truncated);
    }

    #[tokio::test]
    async fn over_max_is_clamped_and_truncated_flag_set() {
        let j = FixedJournal { lines: Vec::new() };
        let r = read(&ServiceLogsRequest { lines: 10_000 }, "u", &j).await.unwrap();
        assert!(r.truncated);
    }

    #[tokio::test]
    async fn under_max_passes_through_untruncated() {
        let j = FixedJournal { lines: Vec::new() };
        let r = read(&ServiceLogsRequest { lines: 50 }, "u", &j).await.unwrap();
        assert!(!r.truncated);
    }
}
