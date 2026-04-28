//! Tauri commands invoked from the Vue frontend via `invoke()`.

use crate::helper_client::{ClientError, HelperClient};
use boxpilot_ipc::ServiceStatusResponse;
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct CommandError {
    pub code: String,
    pub message: String,
}

impl From<ClientError> for CommandError {
    fn from(e: ClientError) -> Self {
        match e {
            ClientError::Method { code, message } => CommandError { code, message },
            ClientError::Connect(inner) => CommandError {
                code: "ipc".into(),
                message: format!("connect to system bus: {inner}"),
            },
            ClientError::Decode(message) => CommandError {
                code: "decode".into(),
                message,
            },
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
