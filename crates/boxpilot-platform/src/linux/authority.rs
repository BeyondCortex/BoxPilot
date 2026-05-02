//! Linux `Authority` impl backed by polkit. Calls
//! `org.freedesktop.PolicyKit1.Authority.CheckAuthorization` on the system
//! bus. Subject is constructed from the caller's D-Bus bus name (`:x.y`)
//! using `kind = "system-bus-name"`.
//!
//! `SubjectProvider` is the seam that lets dispatch — which knows the
//! `CallerPrincipal` but not the platform-specific D-Bus sender — feed the
//! sender into the polkit subject without leaking the D-Bus type into the
//! cross-platform trait. `boxpilotd` provides a `ZbusSubject` impl that the
//! interface methods write the sender into immediately before calling
//! `dispatch::authorize`.

use crate::traits::authority::{Authority, CallerPrincipal};
use async_trait::async_trait;
use boxpilot_ipc::HelperError;
use std::collections::HashMap;
use std::sync::Arc;
use zbus::{proxy, zvariant::Value, Connection};

/// Seam for plumbing the per-call D-Bus sender bus name into the polkit
/// subject. The Linux interface impl writes the sender into a shared
/// `ZbusSubject` before calling `dispatch::authorize`, and `DBusAuthority`
/// reads it back here to build the polkit subject dict.
pub trait SubjectProvider: Send + Sync {
    fn current_sender(&self) -> Option<String>;
}

#[proxy(
    interface = "org.freedesktop.PolicyKit1.Authority",
    default_service = "org.freedesktop.PolicyKit1",
    default_path = "/org/freedesktop/PolicyKit1/Authority"
)]
trait PolkitAuthority {
    #[zbus(name = "CheckAuthorization")]
    fn check_authorization(
        &self,
        subject: &(&str, HashMap<&str, Value<'_>>),
        action_id: &str,
        details: HashMap<&str, &str>,
        flags: u32,
        cancellation_id: &str,
    ) -> zbus::Result<(bool, bool, HashMap<String, String>)>;
}

const FLAG_ALLOW_USER_INTERACTION: u32 = 1;

pub struct DBusAuthority {
    conn: Connection,
    subject_provider: Arc<dyn SubjectProvider>,
}

impl DBusAuthority {
    pub fn new(conn: Connection, subject_provider: Arc<dyn SubjectProvider>) -> Self {
        Self {
            conn,
            subject_provider,
        }
    }
}

#[async_trait]
impl Authority for DBusAuthority {
    async fn check(
        &self,
        _action_id: &str,
        _principal: &CallerPrincipal,
    ) -> Result<bool, HelperError> {
        todo!("filled in Task 4.2b")
    }
}
