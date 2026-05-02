//! Re-export shell. Production impls live in `boxpilot-platform`:
//!
//! - `ServiceManager` (formerly `Systemd`)  → `boxpilot_platform::traits::service`
//! - `LogReader`      (formerly `JournalReader`) → `boxpilot_platform::traits::logs`
//!
//! The old names (`Systemd`, `JournalReader`) are kept as backwards-compat
//! aliases so the rest of `boxpilotd` compiles unchanged. They are scheduled
//! for removal in Sub-project #2's trait redesign.

pub use boxpilot_platform::linux::logs::JournalctlProcess;
pub use boxpilot_platform::linux::service::DBusSystemd;
pub use boxpilot_platform::traits::logs::{JournalReader, LogReader};
pub use boxpilot_platform::traits::service::{ServiceManager, Systemd};

#[cfg(test)]
pub mod testing {
    //! Test fakes re-exported under the historical module path so existing
    //! `use crate::systemd::testing::{FixedSystemd, FixedJournal, ...}` call
    //! sites continue to work without churn.
    pub use boxpilot_platform::fakes::logs::FixedJournal;
    pub use boxpilot_platform::fakes::service::*;
}
