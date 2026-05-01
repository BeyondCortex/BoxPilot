//! Wire types for `diagnostics.export_redacted` (spec §6.3 / §14).

use serde::{Deserialize, Serialize};

/// Bumped when the response shape changes.
pub const DIAGNOSTICS_SCHEMA_VERSION: u32 = 1;

/// §5.5 retention cap for `/var/cache/boxpilot/diagnostics/`. The exporter
/// runs LRU eviction below this watermark before writing a new bundle.
pub const DIAGNOSTICS_BUNDLE_CAP_BYTES: u64 = 100 * 1024 * 1024;

/// Number of journal lines included in the bundle's `journal-tail.txt`.
/// Matches `boxpilot_ipc::service::SERVICE_LOGS_DEFAULT_LINES`.
pub const DIAGNOSTICS_JOURNAL_TAIL_LINES: u32 = 200;

/// Helper response for `diagnostics.export_redacted`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiagnosticsExportResponse {
    pub schema_version: u32,
    /// Absolute path to the freshly written `*.tar.gz`.
    pub bundle_path: String,
    pub bundle_size_bytes: u64,
    /// RFC3339 UTC timestamp the bundle was generated.
    pub generated_at: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn response_round_trips() {
        let r = DiagnosticsExportResponse {
            schema_version: DIAGNOSTICS_SCHEMA_VERSION,
            bundle_path: "/var/cache/boxpilot/diagnostics/x.tar.gz".into(),
            bundle_size_bytes: 4242,
            generated_at: "2026-04-30T22:00:00Z".into(),
        };
        let s = serde_json::to_string(&r).unwrap();
        let back: DiagnosticsExportResponse = serde_json::from_str(&s).unwrap();
        assert_eq!(back, r);
    }

    #[test]
    fn cap_is_100_mib() {
        assert_eq!(DIAGNOSTICS_BUNDLE_CAP_BYTES, 100 * 1024 * 1024);
    }
}
