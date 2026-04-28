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
}

#[derive(Debug, Error)]
pub enum ClientError {
    #[error("connect to system bus: {0}")]
    Connect(#[from] zbus::Error),
    #[error("decode response: {0}")]
    Decode(String),
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
}
