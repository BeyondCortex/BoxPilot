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
            ClientError::Ipc(message) => CommandError {
                code: "ipc".into(),
                message,
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
pub async fn helper_home_status() -> Result<boxpilot_ipc::HomeStatusResponse, CommandError> {
    let client = HelperClient::connect().await?;
    Ok(client.home_status().await?)
}

#[tauri::command]
pub async fn helper_ping() -> Result<&'static str, CommandError> {
    let _client = HelperClient::connect().await?;
    Ok("ok")
}

#[tauri::command]
pub async fn helper_core_discover() -> Result<boxpilot_ipc::CoreDiscoverResponse, CommandError> {
    let c = HelperClient::connect().await?;
    Ok(c.core_discover().await?)
}

#[tauri::command]
pub async fn helper_core_install_managed(
    request: boxpilot_ipc::CoreInstallRequest,
) -> Result<boxpilot_ipc::CoreInstallResponse, CommandError> {
    let c = HelperClient::connect().await?;
    Ok(c.core_install_managed(&request).await?)
}

#[tauri::command]
pub async fn helper_core_upgrade_managed(
    request: boxpilot_ipc::CoreInstallRequest,
) -> Result<boxpilot_ipc::CoreInstallResponse, CommandError> {
    let c = HelperClient::connect().await?;
    Ok(c.core_upgrade_managed(&request).await?)
}

#[tauri::command]
pub async fn helper_core_rollback_managed(
    request: boxpilot_ipc::CoreRollbackRequest,
) -> Result<boxpilot_ipc::CoreInstallResponse, CommandError> {
    let c = HelperClient::connect().await?;
    Ok(c.core_rollback_managed(&request).await?)
}

#[tauri::command]
pub async fn helper_core_adopt(
    request: boxpilot_ipc::CoreAdoptRequest,
) -> Result<boxpilot_ipc::CoreInstallResponse, CommandError> {
    let c = HelperClient::connect().await?;
    Ok(c.core_adopt(&request).await?)
}

#[tauri::command]
pub async fn helper_service_start() -> Result<boxpilot_ipc::ServiceControlResponse, CommandError> {
    let c = HelperClient::connect().await?;
    Ok(c.service_start().await?)
}

#[tauri::command]
pub async fn helper_service_stop() -> Result<boxpilot_ipc::ServiceControlResponse, CommandError> {
    let c = HelperClient::connect().await?;
    Ok(c.service_stop().await?)
}

#[tauri::command]
pub async fn helper_service_restart() -> Result<boxpilot_ipc::ServiceControlResponse, CommandError>
{
    let c = HelperClient::connect().await?;
    Ok(c.service_restart().await?)
}

#[tauri::command]
pub async fn helper_service_enable() -> Result<boxpilot_ipc::ServiceControlResponse, CommandError> {
    let c = HelperClient::connect().await?;
    Ok(c.service_enable().await?)
}

#[tauri::command]
pub async fn helper_service_disable() -> Result<boxpilot_ipc::ServiceControlResponse, CommandError>
{
    let c = HelperClient::connect().await?;
    Ok(c.service_disable().await?)
}

#[tauri::command]
pub async fn helper_service_install_managed(
) -> Result<boxpilot_ipc::ServiceInstallManagedResponse, CommandError> {
    let c = HelperClient::connect().await?;
    Ok(c.service_install_managed().await?)
}

#[tauri::command]
pub async fn helper_service_logs(
    request: boxpilot_ipc::ServiceLogsRequest,
) -> Result<boxpilot_ipc::ServiceLogsResponse, CommandError> {
    let c = HelperClient::connect().await?;
    Ok(c.service_logs(&request).await?)
}

#[tauri::command]
pub async fn helper_legacy_observe_service(
) -> Result<boxpilot_ipc::LegacyObserveServiceResponse, CommandError> {
    let c = HelperClient::connect().await?;
    Ok(c.legacy_observe_service().await?)
}

#[tauri::command]
pub async fn helper_legacy_migrate_prepare(
) -> Result<boxpilot_ipc::LegacyMigratePrepareResponse, CommandError> {
    let c = HelperClient::connect().await?;
    let r = c
        .legacy_migrate_service(&boxpilot_ipc::LegacyMigrateRequest::Prepare)
        .await?;
    match r {
        boxpilot_ipc::LegacyMigrateResponse::Prepare(p) => Ok(p),
        boxpilot_ipc::LegacyMigrateResponse::Cutover(_) => Err(CommandError {
            code: "decode".into(),
            message: "expected Prepare response, got Cutover".into(),
        }),
    }
}

#[tauri::command]
pub async fn helper_legacy_migrate_cutover(
) -> Result<boxpilot_ipc::LegacyMigrateCutoverResponse, CommandError> {
    let c = HelperClient::connect().await?;
    let r = c
        .legacy_migrate_service(&boxpilot_ipc::LegacyMigrateRequest::Cutover)
        .await?;
    match r {
        boxpilot_ipc::LegacyMigrateResponse::Cutover(p) => Ok(p),
        boxpilot_ipc::LegacyMigrateResponse::Prepare(_) => Err(CommandError {
            code: "decode".into(),
            message: "expected Cutover response, got Prepare".into(),
        }),
    }
}

#[tauri::command]
pub async fn helper_diagnostics_export(
) -> Result<boxpilot_ipc::DiagnosticsExportResponse, CommandError> {
    let c = HelperClient::connect().await?;
    Ok(c.diagnostics_export_redacted().await?)
}
