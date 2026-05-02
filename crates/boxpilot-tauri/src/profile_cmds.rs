//! Tauri commands for the user-side profile store. None of these talk
//! to `boxpilotd`; they run in-process as the desktop user.

use std::sync::Arc;

use boxpilot_ipc::ActivationManifest;
use boxpilot_profile::{
    import_local_dir, import_local_file, prepare_bundle, read_remotes, redact_url_for_display,
    refresh_remote, revert_to_last_valid, run_singbox_check, save_edits, BundleError, CheckError,
    CheckOutput, DirImportError, EditError, FetchError, ImportError, PreparedBundle,
    ProfileMetadata, ProfileStore, ReqwestFetcher, SnapshotError, StoreError,
};
use serde::{Deserialize, Serialize};
use tauri::State;

use crate::commands::CommandError;

pub struct ProfileState {
    pub store: Arc<ProfileStore>,
    pub fetcher: Arc<ReqwestFetcher>,
    /// Hold the most recent prepared bundle alive so plan #5 can re-use
    /// it; for plan #4 it is just for the GUI preview round-trip.
    pub last_bundle: tokio::sync::Mutex<Option<PreparedBundle>>,
}

trait ToCommandError {
    fn to_cmd(self) -> CommandError;
}

impl ToCommandError for std::io::Error {
    fn to_cmd(self) -> CommandError {
        CommandError {
            code: "io".into(),
            message: self.to_string(),
        }
    }
}
impl ToCommandError for ImportError {
    fn to_cmd(self) -> CommandError {
        CommandError {
            code: "profile.import".into(),
            message: self.to_string(),
        }
    }
}
impl ToCommandError for DirImportError {
    fn to_cmd(self) -> CommandError {
        CommandError {
            code: "profile.import_dir".into(),
            message: self.to_string(),
        }
    }
}
impl ToCommandError for FetchError {
    fn to_cmd(self) -> CommandError {
        CommandError {
            code: "profile.fetch".into(),
            message: self.to_string(),
        }
    }
}
impl ToCommandError for EditError {
    fn to_cmd(self) -> CommandError {
        CommandError {
            code: "profile.edit".into(),
            message: self.to_string(),
        }
    }
}
impl ToCommandError for StoreError {
    fn to_cmd(self) -> CommandError {
        CommandError {
            code: "profile.store".into(),
            message: self.to_string(),
        }
    }
}
impl ToCommandError for BundleError {
    fn to_cmd(self) -> CommandError {
        CommandError {
            code: "profile.bundle".into(),
            message: self.to_string(),
        }
    }
}
impl ToCommandError for SnapshotError {
    fn to_cmd(self) -> CommandError {
        CommandError {
            code: "profile.snapshot".into(),
            message: self.to_string(),
        }
    }
}
impl ToCommandError for CheckError {
    fn to_cmd(self) -> CommandError {
        CommandError {
            code: "profile.check".into(),
            message: self.to_string(),
        }
    }
}

#[derive(Debug, Serialize)]
pub struct ProfileSummary {
    pub id: String,
    pub name: String,
    pub source_kind: boxpilot_ipc::SourceKind,
    pub remote_id: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub last_valid_activation_id: Option<String>,
    pub config_sha256: String,
    pub remote_url_redacted: Option<String>,
}

fn summarize(store: &ProfileStore, m: ProfileMetadata) -> ProfileSummary {
    let remote_url_redacted = m.remote_id.as_ref().and_then(|rid| {
        let rfile = read_remotes(&store.paths().remotes_json()).ok()?;
        rfile
            .remotes
            .get(rid)
            .map(|e| redact_url_for_display(&e.url))
    });
    ProfileSummary {
        id: m.id,
        name: m.name,
        source_kind: m.source_kind,
        remote_id: m.remote_id,
        created_at: m.created_at,
        updated_at: m.updated_at,
        last_valid_activation_id: m.last_valid_activation_id,
        config_sha256: m.config_sha256,
        remote_url_redacted,
    }
}

#[tauri::command]
pub async fn profile_list(
    state: State<'_, ProfileState>,
) -> Result<Vec<ProfileSummary>, CommandError> {
    let store = state.store.clone();
    let res = tauri::async_runtime::spawn_blocking(move || store.list())
        .await
        .map_err(|e| CommandError {
            code: "join".into(),
            message: e.to_string(),
        })?
        .map_err(|e| e.to_cmd())?;
    Ok(res
        .into_iter()
        .map(|m| summarize(&state.store, m))
        .collect())
}

#[tauri::command]
pub async fn profile_get_source(
    state: State<'_, ProfileState>,
    id: String,
) -> Result<String, CommandError> {
    let store = state.store.clone();
    let bytes = tauri::async_runtime::spawn_blocking(move || store.read_source_bytes(&id))
        .await
        .map_err(|e| CommandError {
            code: "join".into(),
            message: e.to_string(),
        })?
        .map_err(|e| e.to_cmd())?;
    String::from_utf8(bytes).map_err(|e| CommandError {
        code: "utf8".into(),
        message: e.to_string(),
    })
}

#[tauri::command]
pub async fn profile_import_file(
    state: State<'_, ProfileState>,
    name: String,
    path: String,
) -> Result<ProfileSummary, CommandError> {
    let store = state.store.clone();
    let m = tauri::async_runtime::spawn_blocking(move || {
        import_local_file(&store, std::path::Path::new(&path), &name)
    })
    .await
    .map_err(|e| CommandError {
        code: "join".into(),
        message: e.to_string(),
    })?
    .map_err(|e| e.to_cmd())?;
    Ok(summarize(&state.store, m))
}

#[tauri::command]
pub async fn profile_import_dir(
    state: State<'_, ProfileState>,
    name: String,
    dir: String,
) -> Result<ProfileSummary, CommandError> {
    let store = state.store.clone();
    let m = tauri::async_runtime::spawn_blocking(move || {
        import_local_dir(&store, std::path::Path::new(&dir), &name)
    })
    .await
    .map_err(|e| CommandError {
        code: "join".into(),
        message: e.to_string(),
    })?
    .map_err(|e| e.to_cmd())?;
    Ok(summarize(&state.store, m))
}

#[tauri::command]
pub async fn profile_import_remote(
    state: State<'_, ProfileState>,
    name: String,
    url: String,
) -> Result<ProfileSummary, CommandError> {
    let m = boxpilot_profile::import_remote(&state.store, state.fetcher.as_ref(), &name, &url)
        .await
        .map_err(|e| e.to_cmd())?;
    Ok(summarize(&state.store, m))
}

#[tauri::command]
pub async fn profile_refresh_remote(
    state: State<'_, ProfileState>,
    id: String,
) -> Result<ProfileSummary, CommandError> {
    let m = refresh_remote(&state.store, state.fetcher.as_ref(), &id)
        .await
        .map_err(|e| e.to_cmd())?;
    Ok(summarize(&state.store, m))
}

#[tauri::command]
pub async fn profile_save_source(
    state: State<'_, ProfileState>,
    id: String,
    source: String,
) -> Result<(), CommandError> {
    let store = state.store.clone();
    let bytes = source.into_bytes();
    tauri::async_runtime::spawn_blocking(move || save_edits(&store, &id, &bytes))
        .await
        .map_err(|e| CommandError {
            code: "join".into(),
            message: e.to_string(),
        })?
        .map_err(|e| e.to_cmd())
}

#[tauri::command]
pub async fn profile_apply_patch_json(
    state: State<'_, ProfileState>,
    id: String,
    patch_json: String,
) -> Result<(), CommandError> {
    let patch: serde_json::Value = serde_json::from_str(&patch_json).map_err(|e| CommandError {
        code: "json".into(),
        message: e.to_string(),
    })?;
    let store = state.store.clone();
    tauri::async_runtime::spawn_blocking(move || {
        boxpilot_profile::patch_in_place(&store, &id, patch)
    })
    .await
    .map_err(|e| CommandError {
        code: "join".into(),
        message: e.to_string(),
    })?
    .map_err(|e| e.to_cmd())
}

#[tauri::command]
pub async fn profile_revert(
    state: State<'_, ProfileState>,
    id: String,
) -> Result<(), CommandError> {
    let store = state.store.clone();
    tauri::async_runtime::spawn_blocking(move || revert_to_last_valid(&store, &id))
        .await
        .map_err(|e| CommandError {
            code: "join".into(),
            message: e.to_string(),
        })?
        .map_err(|e| e.to_cmd())
}

#[derive(Debug, Deserialize)]
pub struct PrepareBundleRequest {
    pub profile_id: String,
    pub core_path: String,
    pub core_version: String,
}

#[derive(Debug, Serialize)]
pub struct PrepareBundleResponse {
    pub staging_path: String,
    pub manifest: ActivationManifest,
}

#[tauri::command]
pub async fn profile_prepare_bundle(
    state: State<'_, ProfileState>,
    request: PrepareBundleRequest,
) -> Result<PrepareBundleResponse, CommandError> {
    let store = state.store.clone();
    let prepared = prepare_bundle(
        &store,
        &request.profile_id,
        &request.core_path,
        &request.core_version,
    )
    .await
    .map_err(|e| e.to_cmd())?;
    let resp = PrepareBundleResponse {
        staging_path: prepared.staging.path().to_string_lossy().into_owned(),
        manifest: prepared.manifest.clone(),
    };
    *state.last_bundle.lock().await = Some(prepared);
    Ok(resp)
}

#[derive(Debug, Deserialize)]
pub struct CheckRequest {
    pub profile_id: String,
    pub core_path: String,
}

#[derive(Debug, Serialize)]
pub struct CheckResponse {
    pub success: bool,
    pub stdout: String,
    pub stderr: String,
}

#[tauri::command]
pub async fn profile_check(
    state: State<'_, ProfileState>,
    request: CheckRequest,
) -> Result<CheckResponse, CommandError> {
    let CheckRequest {
        profile_id,
        core_path,
    } = request;
    // `core_path` is moved into the first spawn_blocking closure, so
    // clone it now for the second call.
    let core_path_for_check = core_path.clone();
    let store = state.store.clone();
    let prepared = prepare_bundle(&store, &profile_id, &core_path, "best-effort")
        .await
        .map_err(|e| e.to_cmd())?;

    let core_path_buf = std::path::PathBuf::from(core_path_for_check);
    let staging = prepared.staging.path().to_path_buf();
    let out: CheckOutput =
        tauri::async_runtime::spawn_blocking(move || run_singbox_check(&core_path_buf, &staging))
            .await
            .map_err(|e| CommandError {
                code: "join".into(),
                message: e.to_string(),
            })?
            .map_err(|e| e.to_cmd())?;
    Ok(CheckResponse {
        success: out.success,
        stdout: out.stdout,
        stderr: out.stderr,
    })
}

#[derive(Debug, Deserialize)]
pub struct ActivateRequest {
    pub profile_id: String,
    pub core_path: String,
    pub core_version: String,
    #[serde(default)]
    pub verify_window_secs: Option<u32>,
}

#[derive(Debug, Serialize)]
pub struct ActivateResponse {
    pub outcome: String,
    pub activation_id: String,
    pub previous_activation_id: Option<String>,
    pub n_restarts_pre: u32,
    pub n_restarts_post: u32,
    pub window_used_ms: u64,
}

#[tauri::command]
pub async fn profile_activate(
    state: State<'_, ProfileState>,
    request: ActivateRequest,
) -> Result<ActivateResponse, CommandError> {
    let ActivateRequest {
        profile_id,
        core_path,
        core_version,
        verify_window_secs,
    } = request;
    let store = state.store.clone();
    let prepared = prepare_bundle(&store, &profile_id, &core_path, &core_version)
        .await
        .map_err(|e| e.to_cmd())?;

    let req = boxpilot_ipc::ActivateBundleRequest {
        verify_window_secs,
        expected_total_bytes: Some(prepared.tar_size),
    };

    // PR 11b: the AuxStream travels through the typed `HelperClient`
    // wrapper now — no more raw `zbus::Proxy` handling on the GUI side.
    let client = crate::helper_client::HelperClient::connect()
        .await
        .map_err(CommandError::from)?;
    let resp = client
        .profile_activate_bundle(&req, prepared.stream)
        .await
        .map_err(CommandError::from)?;
    let outcome = match resp.outcome {
        boxpilot_ipc::ActivateOutcome::Active => "active",
        boxpilot_ipc::ActivateOutcome::RolledBack => "rolled_back",
        boxpilot_ipc::ActivateOutcome::RollbackTargetMissing => "rollback_target_missing",
        boxpilot_ipc::ActivateOutcome::RollbackUnstartable => "rollback_unstartable",
    }
    .to_string();
    Ok(ActivateResponse {
        outcome,
        activation_id: resp.activation_id,
        previous_activation_id: resp.previous_activation_id,
        n_restarts_pre: resp.verify.n_restarts_pre,
        n_restarts_post: resp.verify.n_restarts_post,
        window_used_ms: resp.verify.window_used_ms,
    })
}

#[derive(Debug, Deserialize)]
pub struct RollbackArgs {
    pub target_activation_id: String,
    #[serde(default)]
    pub verify_window_secs: Option<u32>,
}

#[tauri::command]
pub async fn profile_rollback(request: RollbackArgs) -> Result<ActivateResponse, CommandError> {
    let req = boxpilot_ipc::RollbackRequest {
        target_activation_id: request.target_activation_id,
        verify_window_secs: request.verify_window_secs,
    };
    let client = crate::helper_client::HelperClient::connect()
        .await
        .map_err(|e| CommandError {
            code: "dbus_connect".into(),
            message: e.to_string(),
        })?;
    let resp = client.profile_rollback_release(&req).await.map_err(|e| {
        if let crate::helper_client::ClientError::Method { code, message } = &e {
            CommandError {
                code: code.clone(),
                message: message.clone(),
            }
        } else {
            CommandError {
                code: "dbus_call".into(),
                message: e.to_string(),
            }
        }
    })?;
    let outcome = match resp.outcome {
        boxpilot_ipc::ActivateOutcome::Active => "active",
        boxpilot_ipc::ActivateOutcome::RolledBack => "rolled_back",
        boxpilot_ipc::ActivateOutcome::RollbackTargetMissing => "rollback_target_missing",
        boxpilot_ipc::ActivateOutcome::RollbackUnstartable => "rollback_unstartable",
    }
    .to_string();
    Ok(ActivateResponse {
        outcome,
        activation_id: resp.activation_id,
        previous_activation_id: resp.previous_activation_id,
        n_restarts_pre: resp.verify.n_restarts_pre,
        n_restarts_post: resp.verify.n_restarts_post,
        window_used_ms: resp.verify.window_used_ms,
    })
}
