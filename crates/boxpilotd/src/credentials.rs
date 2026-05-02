//! Caller-identity extraction. **Identity must come from the IPC transport
//! (D-Bus on Linux, Named Pipe token on Windows), never from the request body**
//! — spec §6.1.
//!
//! The `CallerResolver` trait itself is platform-neutral so cross-platform
//! `HelperContext` can hold an `Arc<dyn CallerResolver>`. The concrete
//! `DBusCallerResolver` impl is Linux-only because it uses zbus to call
//! `org.freedesktop.DBus.GetConnectionUnixUser`. Windows wires up a
//! `NoopCallerResolver` in `entry/windows.rs` because SID resolution
//! happens at the Named Pipe layer instead of through this trait.

use async_trait::async_trait;
use boxpilot_ipc::HelperError;

#[allow(dead_code)] // used in plan #2 (caller identity tracking)
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
#[cfg(target_os = "linux")]
pub struct DBusCallerResolver {
    conn: zbus::Connection,
}

#[cfg(target_os = "linux")]
impl DBusCallerResolver {
    pub fn new(conn: zbus::Connection) -> Self {
        Self { conn }
    }
}

#[cfg(target_os = "linux")]
#[async_trait]
impl CallerResolver for DBusCallerResolver {
    async fn resolve(&self, sender: &str) -> Result<u32, HelperError> {
        let proxy = zbus::fdo::DBusProxy::new(&self.conn)
            .await
            .map_err(|e| HelperError::Ipc {
                message: format!("DBusProxy: {e}"),
            })?;
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
            Self(Mutex::new(
                rows.iter().map(|(s, u)| (s.to_string(), *u)).collect(),
            ))
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
