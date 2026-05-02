//! Log-tailing abstraction. Currently shaped 1:1 with the existing
//! `boxpilotd::systemd::JournalReader` trait. Windows EventLog shape
//! redesign is Sub-project #2.
//!
//! `lines` is `u32` to match the existing call sites and the on-the-wire
//! IPC contract; do not change without a schema bump.

use async_trait::async_trait;
use boxpilot_ipc::HelperError;

#[async_trait]
pub trait LogReader: Send + Sync {
    /// Return the last `lines` log entries for `unit_name`. Caller is
    /// responsible for clamping `lines` to a sane upper bound; this trait
    /// passes through whatever it gets.
    ///
    /// Linux: `journalctl -u <unit> -n <lines> --output=short`.
    /// Windows: `EvtQuery` filter (Sub-project #2).
    async fn tail(&self, unit_name: &str, lines: u32) -> Result<Vec<String>, HelperError>;
}

/// Backwards-compatible alias for callers that imported the old name.
/// Schedule for removal in Sub-project #2's trait redesign.
pub use LogReader as JournalReader;
