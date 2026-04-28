//! Caller-identity extraction. **Identity must come from the D-Bus
//! connection, never from the request body** — spec §6.1.

use async_trait::async_trait;
use boxpilot_ipc::HelperError;
use zbus::Connection;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallerIdentity {
    pub uid: u32,
    pub sender: String,
}

#[async_trait]
pub trait CallerResolver: Send + Sync {
    async fn resolve(&self, sender: &str) -> Result<u32, HelperError>;
}

/// Real resolver: calls `org.freedesktop.DBus.GetConnectionUnixUser` on the
/// system bus.
pub struct DBusCallerResolver {
    conn: Connection,
}

impl DBusCallerResolver {
    pub fn new(conn: Connection) -> Self {
        Self { conn }
    }
}

#[async_trait]
impl CallerResolver for DBusCallerResolver {
    async fn resolve(&self, sender: &str) -> Result<u32, HelperError> {
        let proxy = zbus::fdo::DBusProxy::new(&self.conn)
            .await
            .map_err(|e| HelperError::Ipc { message: format!("DBusProxy: {e}") })?;
        let uid = proxy
            .get_connection_unix_user(sender.try_into().map_err(|e| HelperError::Ipc {
                message: format!("bad sender name {sender}: {e}"),
            })?)
            .await
            .map_err(|e| HelperError::Ipc {
                message: format!("GetConnectionUnixUser({sender}): {e}"),
            })?;
        Ok(uid)
    }
}

#[cfg(test)]
pub mod testing {
    use super::*;
    use std::collections::HashMap;
    use std::sync::Mutex;

    pub struct FixedResolver(pub Mutex<HashMap<String, u32>>);

    impl FixedResolver {
        pub fn with(rows: &[(&str, u32)]) -> Self {
            Self(Mutex::new(rows.iter().map(|(s, u)| (s.to_string(), *u)).collect()))
        }
    }

    #[async_trait]
    impl CallerResolver for FixedResolver {
        async fn resolve(&self, sender: &str) -> Result<u32, HelperError> {
            self.0
                .lock()
                .unwrap()
                .get(sender)
                .copied()
                .ok_or_else(|| HelperError::Ipc {
                    message: format!("test: unknown sender {sender}"),
                })
        }
    }

    #[tokio::test]
    async fn fixed_resolver_returns_canned_uid() {
        let r = FixedResolver::with(&[(":1.42", 1000)]);
        assert_eq!(r.resolve(":1.42").await.unwrap(), 1000);
    }
}
