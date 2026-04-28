//! Tauri commands invoked from the Vue frontend via `invoke()`.
//! Stub bodies — Task 23 replaces this with the real helper-client wiring.

use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct CommandError {
    pub code: String,
    pub message: String,
}

#[tauri::command]
pub async fn helper_service_status() -> Result<String, CommandError> {
    Err(CommandError {
        code: "not_implemented".into(),
        message: "Task 22 stub; populated in Task 23".into(),
    })
}

#[tauri::command]
pub async fn helper_ping() -> Result<&'static str, CommandError> {
    Err(CommandError {
        code: "not_implemented".into(),
        message: "Task 22 stub; populated in Task 23".into(),
    })
}
