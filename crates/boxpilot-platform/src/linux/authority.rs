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
        action_id: &str,
        principal: &CallerPrincipal,
    ) -> Result<bool, HelperError> {
        // Defensive: a Linux DBusAuthority must never be invoked with a
        // non-Linux principal. The polkit call itself does not consume the
        // uid (it operates by D-Bus sender), but a mis-typed principal
        // signals a wiring bug elsewhere — fail loud rather than silently
        // dropping the principal info.
        let _uid = match principal {
            CallerPrincipal::LinuxUid(u) => *u,
            CallerPrincipal::WindowsSid(_) => {
                return Err(HelperError::Ipc {
                    message: "linux DBusAuthority received non-Linux principal".into(),
                });
            }
        };

        let sender = self
            .subject_provider
            .current_sender()
            .ok_or_else(|| HelperError::Ipc {
                message: "polkit subject (D-Bus sender) unknown".into(),
            })?;

        let proxy = PolkitAuthorityProxy::new(&self.conn)
            .await
            .map_err(|e| HelperError::Ipc {
                message: format!("polkit proxy: {e}"),
            })?;

        let mut subject_data: HashMap<&str, Value<'_>> = HashMap::new();
        let bus_name_value = Value::Str(sender.as_str().into());
        subject_data.insert("name", bus_name_value);

        let (is_authorized, _is_challenge, _details) = proxy
            .check_authorization(
                &("system-bus-name", subject_data),
                action_id,
                HashMap::new(),
                FLAG_ALLOW_USER_INTERACTION,
                "", // cancellation id (unused)
            )
            .await
            .map_err(|e| HelperError::Ipc {
                message: format!("polkit CheckAuthorization({action_id}): {e}"),
            })?;
        Ok(is_authorized)
    }
}
