//! Tauri-side D-Bus client. Calls `app.boxpilot.Helper1` as the running GUI
//! user and surfaces the typed JSON response back to Vue.

use boxpilot_ipc::ServiceStatusResponse;
use thiserror::Error;
use zbus::{proxy, Connection};

#[proxy(
    interface = "app.boxpilot.Helper1",
    default_service = "app.boxpilot.Helper",
    default_path = "/app/boxpilot/Helper"
)]
trait Helper {
    fn service_status(&self) -> zbus::Result<String>;

    #[zbus(name = "CoreDiscover")]
    fn core_discover(&self) -> zbus::Result<String>;

    #[zbus(name = "CoreInstallManaged")]
    fn core_install_managed(&self, request_json: &str) -> zbus::Result<String>;

    #[zbus(name = "CoreUpgradeManaged")]
    fn core_upgrade_managed(&self, request_json: &str) -> zbus::Result<String>;

    #[zbus(name = "CoreRollbackManaged")]
    fn core_rollback_managed(&self, request_json: &str) -> zbus::Result<String>;

    #[zbus(name = "CoreAdopt")]
    fn core_adopt(&self, request_json: &str) -> zbus::Result<String>;

    #[zbus(name = "ServiceStart")]
    fn service_start(&self) -> zbus::Result<String>;
    #[zbus(name = "ServiceStop")]
    fn service_stop(&self) -> zbus::Result<String>;
    #[zbus(name = "ServiceRestart")]
    fn service_restart(&self) -> zbus::Result<String>;
    #[zbus(name = "ServiceEnable")]
    fn service_enable(&self) -> zbus::Result<String>;
    #[zbus(name = "ServiceDisable")]
    fn service_disable(&self) -> zbus::Result<String>;
    #[zbus(name = "ServiceInstallManaged")]
    fn service_install_managed(&self) -> zbus::Result<String>;
    #[zbus(name = "ServiceLogs")]
    fn service_logs(&self, request_json: &str) -> zbus::Result<String>;

    // ProfileActivateBundle is intentionally NOT in this typed proxy: it
    // takes a `UNIX_FD` payload which the `#[proxy]` macro can't express
    // cleanly. Activation goes through `profile_cmds::profile_activate`
    // which constructs a raw `zbus::Proxy` for that one method.
    #[zbus(name = "ProfileRollbackRelease")]
    fn profile_rollback_release(&self, request_json: &str) -> zbus::Result<String>;

    #[zbus(name = "LegacyObserveService")]
    fn legacy_observe_service(&self) -> zbus::Result<String>;
    #[zbus(name = "LegacyMigrateService")]
    fn legacy_migrate_service(&self, request_json: &str) -> zbus::Result<String>;
}

#[derive(Debug, Error)]
pub enum ClientError {
    /// Method-level error from the helper. `code` is the suffix of the
    /// helper's error name (e.g. `"not_authorized"`); `message` is the
    /// human-readable detail.
    #[error("{code}: {message}")]
    Method { code: String, message: String },
    #[error("connect to system bus: {0}")]
    Connect(zbus::Error),
    #[error("decode response: {0}")]
    Decode(String),
}

impl From<zbus::Error> for ClientError {
    fn from(e: zbus::Error) -> Self {
        // boxpilotd encodes typed errors as
        //   zbus::fdo::Error::Failed("app.boxpilot.Helper1.<Code>: <message>")
        // which arrives here as Error::MethodError with
        //   name   = "org.freedesktop.DBus.Error.Failed"
        //   detail = "app.boxpilot.Helper1.<Code>: <message>"
        if let zbus::Error::MethodError(_, Some(detail), _) = &e {
            const PREFIX: &str = "app.boxpilot.Helper1.";
            if let Some(rest) = detail.strip_prefix(PREFIX) {
                if let Some((code, message)) = rest.split_once(": ") {
                    return ClientError::Method {
                        code: snake_case(code),
                        message: message.to_string(),
                    };
                }
            }
        }
        ClientError::Connect(e)
    }
}

/// Convert a CamelCase or PascalCase code (e.g. "NotAuthorized") into
/// snake_case ("not_authorized") so the GUI matches the JSON tag from
/// `boxpilot_ipc::HelperError`.
fn snake_case(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 4);
    for (i, ch) in s.chars().enumerate() {
        if ch.is_ascii_uppercase() {
            if i > 0 {
                out.push('_');
            }
            out.push(ch.to_ascii_lowercase());
        } else {
            out.push(ch);
        }
    }
    out
}

pub struct HelperClient {
    conn: Connection,
}

impl HelperClient {
    pub async fn connect() -> Result<Self, ClientError> {
        Ok(Self {
            conn: Connection::system().await?,
        })
    }

    pub async fn service_status(&self) -> Result<ServiceStatusResponse, ClientError> {
        let proxy = HelperProxy::new(&self.conn).await?;
        let json = proxy.service_status().await?;
        serde_json::from_str(&json).map_err(|e| ClientError::Decode(e.to_string()))
    }

    pub async fn core_discover(&self) -> Result<boxpilot_ipc::CoreDiscoverResponse, ClientError> {
        let proxy = HelperProxy::new(&self.conn).await?;
        let json = proxy.core_discover().await?;
        serde_json::from_str(&json).map_err(|e| ClientError::Decode(e.to_string()))
    }

    pub async fn core_install_managed(
        &self,
        req: &boxpilot_ipc::CoreInstallRequest,
    ) -> Result<boxpilot_ipc::CoreInstallResponse, ClientError> {
        let proxy = HelperProxy::new(&self.conn).await?;
        let json = proxy
            .core_install_managed(&serde_json::to_string(req).unwrap())
            .await?;
        serde_json::from_str(&json).map_err(|e| ClientError::Decode(e.to_string()))
    }

    pub async fn core_upgrade_managed(
        &self,
        req: &boxpilot_ipc::CoreInstallRequest,
    ) -> Result<boxpilot_ipc::CoreInstallResponse, ClientError> {
        let proxy = HelperProxy::new(&self.conn).await?;
        let json = proxy
            .core_upgrade_managed(&serde_json::to_string(req).unwrap())
            .await?;
        serde_json::from_str(&json).map_err(|e| ClientError::Decode(e.to_string()))
    }

    pub async fn core_rollback_managed(
        &self,
        req: &boxpilot_ipc::CoreRollbackRequest,
    ) -> Result<boxpilot_ipc::CoreInstallResponse, ClientError> {
        let proxy = HelperProxy::new(&self.conn).await?;
        let json = proxy
            .core_rollback_managed(&serde_json::to_string(req).unwrap())
            .await?;
        serde_json::from_str(&json).map_err(|e| ClientError::Decode(e.to_string()))
    }

    pub async fn core_adopt(
        &self,
        req: &boxpilot_ipc::CoreAdoptRequest,
    ) -> Result<boxpilot_ipc::CoreInstallResponse, ClientError> {
        let proxy = HelperProxy::new(&self.conn).await?;
        let json = proxy
            .core_adopt(&serde_json::to_string(req).unwrap())
            .await?;
        serde_json::from_str(&json).map_err(|e| ClientError::Decode(e.to_string()))
    }

    pub async fn service_start(&self) -> Result<boxpilot_ipc::ServiceControlResponse, ClientError> {
        let proxy = HelperProxy::new(&self.conn).await?;
        let json = proxy.service_start().await?;
        serde_json::from_str(&json).map_err(|e| ClientError::Decode(e.to_string()))
    }

    pub async fn service_stop(&self) -> Result<boxpilot_ipc::ServiceControlResponse, ClientError> {
        let proxy = HelperProxy::new(&self.conn).await?;
        let json = proxy.service_stop().await?;
        serde_json::from_str(&json).map_err(|e| ClientError::Decode(e.to_string()))
    }

    pub async fn service_restart(
        &self,
    ) -> Result<boxpilot_ipc::ServiceControlResponse, ClientError> {
        let proxy = HelperProxy::new(&self.conn).await?;
        let json = proxy.service_restart().await?;
        serde_json::from_str(&json).map_err(|e| ClientError::Decode(e.to_string()))
    }

    pub async fn service_enable(
        &self,
    ) -> Result<boxpilot_ipc::ServiceControlResponse, ClientError> {
        let proxy = HelperProxy::new(&self.conn).await?;
        let json = proxy.service_enable().await?;
        serde_json::from_str(&json).map_err(|e| ClientError::Decode(e.to_string()))
    }

    pub async fn service_disable(
        &self,
    ) -> Result<boxpilot_ipc::ServiceControlResponse, ClientError> {
        let proxy = HelperProxy::new(&self.conn).await?;
        let json = proxy.service_disable().await?;
        serde_json::from_str(&json).map_err(|e| ClientError::Decode(e.to_string()))
    }

    pub async fn service_install_managed(
        &self,
    ) -> Result<boxpilot_ipc::ServiceInstallManagedResponse, ClientError> {
        let proxy = HelperProxy::new(&self.conn).await?;
        let json = proxy.service_install_managed().await?;
        serde_json::from_str(&json).map_err(|e| ClientError::Decode(e.to_string()))
    }

    pub async fn service_logs(
        &self,
        req: &boxpilot_ipc::ServiceLogsRequest,
    ) -> Result<boxpilot_ipc::ServiceLogsResponse, ClientError> {
        let proxy = HelperProxy::new(&self.conn).await?;
        let json = proxy
            .service_logs(&serde_json::to_string(req).unwrap())
            .await?;
        serde_json::from_str(&json).map_err(|e| ClientError::Decode(e.to_string()))
    }

    pub async fn profile_rollback_release(
        &self,
        req: &boxpilot_ipc::RollbackRequest,
    ) -> Result<boxpilot_ipc::ActivateBundleResponse, ClientError> {
        let proxy = HelperProxy::new(&self.conn).await?;
        let json = proxy
            .profile_rollback_release(&serde_json::to_string(req).unwrap())
            .await?;
        serde_json::from_str(&json).map_err(|e| ClientError::Decode(e.to_string()))
    }

    pub async fn legacy_observe_service(
        &self,
    ) -> Result<boxpilot_ipc::LegacyObserveServiceResponse, ClientError> {
        let proxy = HelperProxy::new(&self.conn).await?;
        let json = proxy.legacy_observe_service().await?;
        serde_json::from_str(&json).map_err(|e| ClientError::Decode(e.to_string()))
    }

    pub async fn legacy_migrate_service(
        &self,
        req: &boxpilot_ipc::LegacyMigrateRequest,
    ) -> Result<boxpilot_ipc::LegacyMigrateResponse, ClientError> {
        let proxy = HelperProxy::new(&self.conn).await?;
        let json = proxy
            .legacy_migrate_service(&serde_json::to_string(req).unwrap())
            .await?;
        serde_json::from_str(&json).map_err(|e| ClientError::Decode(e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snake_case_basic() {
        assert_eq!(snake_case("NotAuthorized"), "not_authorized");
        assert_eq!(snake_case("ControllerOrphaned"), "controller_orphaned");
        assert_eq!(snake_case("Busy"), "busy");
    }

    #[test]
    fn snake_case_already_snake_idempotent() {
        // We don't expect snake input but be defensive.
        assert_eq!(snake_case("not_found"), "not_found");
    }

    #[test]
    fn snake_case_consecutive_caps_split_per_char() {
        // Consecutive caps split on every uppercase boundary; this is fine
        // because our error codes are unambiguously CamelCase (no acronyms).
        assert_eq!(
            snake_case("UnsupportedSchemaVersion"),
            "unsupported_schema_version"
        );
    }
}
