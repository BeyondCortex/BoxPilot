//! Version self-check for `sing-box` binaries (spec §6.5 last bullet). Linux
//! impl spawns `<binary> version`. Windows impl is a stub in Sub-project #1
//! and will exec `sing-box.exe --version` in Sub-project #2.
//!
//! Returns the full stdout on success so the caller can apply its own version
//! parsing (e.g. `parse_singbox_version`). The error is `io::Error` so the
//! trait stays platform-error-clean — the boxpilotd caller wraps any failure
//! into its own `TrustError::VersionCheckFailed` at the call site.

use std::io;
use std::path::Path;

pub trait VersionChecker: Send + Sync {
    /// Run `<binary> version` (or equivalent) and return the trimmed stdout,
    /// expected to begin with `"sing-box version"`.
    fn check(&self, binary: &Path) -> io::Result<String>;
}
