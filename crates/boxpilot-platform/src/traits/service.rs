//! Service-control abstraction. Currently shaped 1:1 with the existing
//! `boxpilotd::systemd::Systemd` trait. SCM (Windows) shape redesign is
//! Sub-project #2's first task per COQ4.
//!
//! Method names and `UnitState` are part of the GUI's wire protocol via
//! `boxpilot-ipc`; do not rename without a schema bump.

use async_trait::async_trait;
use boxpilot_ipc::{HelperError, UnitState};

#[async_trait]
pub trait ServiceManager: Send + Sync {
    async fn unit_state(&self, unit_name: &str) -> Result<UnitState, HelperError>;
    async fn start_unit(&self, unit_name: &str) -> Result<(), HelperError>;
    async fn stop_unit(&self, unit_name: &str) -> Result<(), HelperError>;
    async fn restart_unit(&self, unit_name: &str) -> Result<(), HelperError>;
    async fn enable_unit_files(&self, unit_names: &[String]) -> Result<(), HelperError>;
    async fn disable_unit_files(&self, unit_names: &[String]) -> Result<(), HelperError>;
    /// Equivalent to `systemctl daemon-reload`. Required after writing a
    /// new unit file so systemd parses it before the next StartUnit.
    async fn reload(&self) -> Result<(), HelperError>;

    /// `org.freedesktop.systemd1.Unit::FragmentPath` — the on-disk unit file
    /// for `unit_name`. `None` for transient units or when the fragment has
    /// been deleted from disk.
    async fn fragment_path(&self, unit_name: &str) -> Result<Option<String>, HelperError>;

    /// `systemctl is-enabled` view: `enabled` / `disabled` / `static` /
    /// `masked` / `not-found`. Surfaced as a string because the systemd
    /// vocabulary is itself open-ended; consumers branch on the canonical
    /// values they care about.
    async fn unit_file_state(&self, unit_name: &str) -> Result<Option<String>, HelperError>;
}

/// Backwards-compatible alias for callers that imported the old name.
/// Schedule for removal in Sub-project #2's trait redesign.
pub use ServiceManager as Systemd;
