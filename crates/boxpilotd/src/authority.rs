//! `boxpilotd`-side glue around `boxpilot_platform::Authority`.
//!
//! Re-exports the platform `Authority` trait + `CallerPrincipal` enum + the
//! Linux `DBusAuthority` impl. Adds `ZbusSubject`, the per-call sender
//! shuttle that the iface methods write into immediately before calling
//! `dispatch::authorize`. `DBusAuthority` reads it back via the
//! `SubjectProvider` trait when assembling the polkit subject.
//!
//! `ZbusSubject` is always present (even on Windows) so `HelperContext`
//! can hold it platform-neutrally; the `SubjectProvider` impl is
//! Linux-only because polkit is Linux-only.

#[cfg(target_os = "linux")]
pub use boxpilot_platform::linux::authority::DBusAuthority;
pub use boxpilot_platform::traits::authority::{Authority, CallerPrincipal};

#[cfg(test)]
pub mod testing {
    pub use boxpilot_platform::fakes::authority::CannedAuthority;
}

use std::sync::{Arc, RwLock};

/// Per-call D-Bus sender shuttle. The iface impl `set`s the sender just
/// before calling `dispatch::authorize`; the Linux `DBusAuthority` reads it
/// back via `SubjectProvider::current_sender` when building the polkit
/// subject. Wrapped in `RwLock` because zbus interface methods are `&self`
/// and may run concurrently — but each call writes the sender before doing
/// any awaits, so the value seen by `current_sender()` always belongs to a
/// recent in-flight call. Tightening this to a per-call `tokio::task_local`
/// is tracked as a follow-up.
///
/// Always present even on Windows (where the field is unused) so
/// `HelperContext` stays platform-neutral.
#[derive(Default)]
pub struct ZbusSubject {
    inner: Arc<RwLock<Option<String>>>,
}

impl ZbusSubject {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set(&self, sender: &str) {
        *self.inner.write().unwrap() = Some(sender.to_string());
    }
}

#[cfg(target_os = "linux")]
impl boxpilot_platform::linux::authority::SubjectProvider for ZbusSubject {
    fn current_sender(&self) -> Option<String> {
        self.inner.read().unwrap().clone()
    }
}
