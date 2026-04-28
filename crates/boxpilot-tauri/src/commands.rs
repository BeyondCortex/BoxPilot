//! Tauri commands invoked from the Vue frontend via `invoke()`.

use crate::helper_client::HelperClient;
use boxpilot_ipc::ServiceStatusResponse;
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct CommandError {
    pub code: String,
    pub message: String,
}

impl<E: std::fmt::Display> From<E> for CommandError {
    fn from(e: E) -> Self {
        let s = e.to_string();
        // Split "app.boxpilot.Helper1.X: msg" into code/message if present.
        if let Some(rest) = s.strip_prefix("app.boxpilot.Helper1.") {
            if let Some((code, msg)) = rest.split_once(": ") {
                return CommandError {
                    code: code.into(),
                    message: msg.into(),
                };
            }
        }
        CommandError {
            code: "ipc".into(),
            message: s,
        }
    }
}

#[tauri::command]
pub async fn helper_service_status() -> Result<ServiceStatusResponse, CommandError> {
    let client = HelperClient::connect().await?;
    Ok(client.service_status().await?)
}

#[tauri::command]
pub async fn helper_ping() -> Result<&'static str, CommandError> {
    let _client = HelperClient::connect().await?;
    Ok("ok")
}
