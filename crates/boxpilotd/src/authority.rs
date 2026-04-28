//! polkit authorization. Calls
//! `org.freedesktop.PolicyKit1.Authority.CheckAuthorization` on the system
//! bus. Subject is constructed from the caller's D-Bus bus name (`:x.y`)
//! using `kind = "system-bus-name"`.

use async_trait::async_trait;
use boxpilot_ipc::HelperError;
use std::collections::HashMap;
use zbus::{
    proxy,
    zvariant::{OwnedValue, Value},
    Connection,
};

#[async_trait]
pub trait Authority: Send + Sync {
    /// Returns Ok(true) if authorized, Ok(false) if denied (including auth
    /// dismissal), Err if polkit itself errors.
    async fn check(&self, action_id: &str, sender_bus_name: &str) -> Result<bool, HelperError>;
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
}

impl DBusAuthority {
    pub fn new(conn: Connection) -> Self {
        Self { conn }
    }
}

#[async_trait]
impl Authority for DBusAuthority {
    async fn check(&self, action_id: &str, sender_bus_name: &str) -> Result<bool, HelperError> {
        let proxy = PolkitAuthorityProxy::new(&self.conn)
            .await
            .map_err(|e| HelperError::Ipc { message: format!("polkit proxy: {e}") })?;

        let mut subject_data: HashMap<&str, Value<'_>> = HashMap::new();
        let bus_name_value = Value::Str(sender_bus_name.into());
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
        // Reference OwnedValue to silence unused-import warnings if zbus
        // changes its re-exports between minor versions.
        let _ = std::marker::PhantomData::<OwnedValue>;
        Ok(is_authorized)
    }
}

#[cfg(test)]
pub mod testing {
    use super::*;
    use std::collections::HashMap as Map;
    use std::sync::Mutex;

    pub struct CannedAuthority(pub Mutex<Map<String, bool>>);

    impl CannedAuthority {
        pub fn allowing(actions: &[&str]) -> Self {
            Self(Mutex::new(actions.iter().map(|a| (a.to_string(), true)).collect()))
        }
        pub fn denying(actions: &[&str]) -> Self {
            Self(Mutex::new(actions.iter().map(|a| (a.to_string(), false)).collect()))
        }
    }

    #[async_trait]
    impl Authority for CannedAuthority {
        async fn check(&self, action_id: &str, _sender: &str) -> Result<bool, HelperError> {
            let map = self.0.lock().unwrap();
            map.get(action_id).copied().ok_or_else(|| HelperError::Ipc {
                message: format!("test: unconfigured action {action_id}"),
            })
        }
    }

    #[tokio::test]
    async fn canned_allow() {
        let a = CannedAuthority::allowing(&["app.boxpilot.helper.service.status"]);
        assert!(a.check("app.boxpilot.helper.service.status", ":1.5").await.unwrap());
    }

    #[tokio::test]
    async fn canned_deny() {
        let a = CannedAuthority::denying(&["app.boxpilot.helper.service.start"]);
        assert!(!a.check("app.boxpilot.helper.service.start", ":1.5").await.unwrap());
    }
}
