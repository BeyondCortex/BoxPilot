//! Tauri-side helper client. Uses [`boxpilot_platform::traits::ipc::IpcClient`]
//! for the transport (Linux: `ZbusIpcClient`; Windows: PR 12) and decodes
//! the JSON responses into the existing `boxpilot_ipc` types so callers
//! in `commands.rs` and `profile_cmds.rs` are unaffected.

use boxpilot_ipc::HelperError;
use boxpilot_platform::traits::bundle_aux::AuxStream;
use boxpilot_platform::traits::ipc::IpcClient;
use boxpilot_ipc::HelperMethod;
use std::sync::Arc;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ClientError {
    /// Method-level error from the helper. `code` is the snake_case
    /// `HelperError` discriminant (e.g. `"not_authorized"`); `message` is
    /// the human-readable detail rendered via `Display`.
    #[error("{code}: {message}")]
    Method { code: String, message: String },
    /// IPC transport / connection failure (no typed code surfaced).
    #[error("ipc: {0}")]
    Ipc(String),
    /// JSON decoding of a typed response failed.
    #[error("decode response: {0}")]
    Decode(String),
}

impl From<HelperError> for ClientError {
    fn from(e: HelperError) -> Self {
        let code = helper_error_code(&e);
        ClientError::Method {
            code: code.to_string(),
            message: e.to_string(),
        }
    }
}

/// snake_case discriminant for [`HelperError`]. Matches the wire form
/// produced by `#[serde(tag = "code", rename_all = "snake_case")]` on the
/// enum so the GUI's branch-on-code logic keeps working.
fn helper_error_code(e: &HelperError) -> &'static str {
    use HelperError::*;
    match e {
        NotImplemented => "not_implemented",
        NotAuthorized => "not_authorized",
        NotController => "not_controller",
        ControllerOrphaned => "controller_orphaned",
        ControllerNotSet => "controller_not_set",
        UnsupportedSchemaVersion { .. } => "unsupported_schema_version",
        Busy => "busy",
        Systemd { .. } => "systemd",
        Ipc { .. } => "ipc",
        BundleTooLarge { .. } => "bundle_too_large",
        BundleEntryRejected { .. } => "bundle_entry_rejected",
        BundleAssetMismatch { .. } => "bundle_asset_mismatch",
        SingboxCheckFailed { .. } => "singbox_check_failed",
        RollbackTargetMissing => "rollback_target_missing",
        RollbackUnstartable { .. } => "rollback_unstartable",
        ActiveCorrupt => "active_corrupt",
        ReleaseAlreadyActive => "release_already_active",
        ReleaseNotFound { .. } => "release_not_found",
        LegacyConfigPathUnsafe { .. } => "legacy_config_path_unsafe",
        LegacyUnitNotFound { .. } => "legacy_unit_not_found",
        LegacyExecStartUnparseable { .. } => "legacy_exec_start_unparseable",
        LegacyStopFailed { .. } => "legacy_stop_failed",
        LegacyDisableFailed { .. } => "legacy_disable_failed",
        LegacyConflictsWithManaged { .. } => "legacy_conflicts_with_managed",
        LegacyAssetTooLarge { .. } => "legacy_asset_too_large",
        LegacyTooManyAssets { .. } => "legacy_too_many_assets",
        DiagnosticsIoFailed { .. } => "diagnostics_io_failed",
        DiagnosticsEncodeFailed { .. } => "diagnostics_encode_failed",
    }
}

pub struct HelperClient {
    ipc: Arc<dyn IpcClient>,
}

impl HelperClient {
    pub async fn connect() -> Result<Self, ClientError> {
        #[cfg(target_os = "linux")]
        {
            let client = boxpilot_platform::linux::ipc::ZbusIpcClient::connect_system()
                .await
                .map_err(ClientError::from)?;
            Ok(Self {
                ipc: Arc::new(client),
            })
        }
        #[cfg(not(target_os = "linux"))]
        {
            Err(ClientError::Ipc(
                "HelperClient::connect: no IpcClient impl for this platform yet".into(),
            ))
        }
    }

    /// Convenience wrapper for verbs that take no `AuxStream`.
    async fn call_no_aux<R: serde::de::DeserializeOwned>(
        &self,
        method: HelperMethod,
        body: Vec<u8>,
    ) -> Result<R, ClientError> {
        let resp_bytes = self.ipc.call(method, body, AuxStream::none()).await?;
        serde_json::from_slice(&resp_bytes).map_err(|e| ClientError::Decode(e.to_string()))
    }

    fn encode_request<T: serde::Serialize>(req: &T) -> Vec<u8> {
        // The JSON shape is fully owned by us — `to_vec` only fails for
        // exotic types like maps with non-string keys, which our
        // request DTOs don't use.
        serde_json::to_vec(req).expect("request serializable to JSON")
    }

    pub async fn service_status(
        &self,
    ) -> Result<boxpilot_ipc::ServiceStatusResponse, ClientError> {
        self.call_no_aux(HelperMethod::ServiceStatus, vec![]).await
    }

    pub async fn home_status(&self) -> Result<boxpilot_ipc::HomeStatusResponse, ClientError> {
        self.call_no_aux(HelperMethod::HomeStatus, vec![]).await
    }

    pub async fn core_discover(
        &self,
    ) -> Result<boxpilot_ipc::CoreDiscoverResponse, ClientError> {
        self.call_no_aux(HelperMethod::CoreDiscover, vec![]).await
    }

    pub async fn core_install_managed(
        &self,
        req: &boxpilot_ipc::CoreInstallRequest,
    ) -> Result<boxpilot_ipc::CoreInstallResponse, ClientError> {
        self.call_no_aux(HelperMethod::CoreInstallManaged, Self::encode_request(req))
            .await
    }

    pub async fn core_upgrade_managed(
        &self,
        req: &boxpilot_ipc::CoreInstallRequest,
    ) -> Result<boxpilot_ipc::CoreInstallResponse, ClientError> {
        self.call_no_aux(HelperMethod::CoreUpgradeManaged, Self::encode_request(req))
            .await
    }

    pub async fn core_rollback_managed(
        &self,
        req: &boxpilot_ipc::CoreRollbackRequest,
    ) -> Result<boxpilot_ipc::CoreInstallResponse, ClientError> {
        self.call_no_aux(HelperMethod::CoreRollbackManaged, Self::encode_request(req))
            .await
    }

    pub async fn core_adopt(
        &self,
        req: &boxpilot_ipc::CoreAdoptRequest,
    ) -> Result<boxpilot_ipc::CoreInstallResponse, ClientError> {
        self.call_no_aux(HelperMethod::CoreAdopt, Self::encode_request(req))
            .await
    }

    pub async fn service_start(
        &self,
    ) -> Result<boxpilot_ipc::ServiceControlResponse, ClientError> {
        self.call_no_aux(HelperMethod::ServiceStart, vec![]).await
    }

    pub async fn service_stop(
        &self,
    ) -> Result<boxpilot_ipc::ServiceControlResponse, ClientError> {
        self.call_no_aux(HelperMethod::ServiceStop, vec![]).await
    }

    pub async fn service_restart(
        &self,
    ) -> Result<boxpilot_ipc::ServiceControlResponse, ClientError> {
        self.call_no_aux(HelperMethod::ServiceRestart, vec![]).await
    }

    pub async fn service_enable(
        &self,
    ) -> Result<boxpilot_ipc::ServiceControlResponse, ClientError> {
        self.call_no_aux(HelperMethod::ServiceEnable, vec![]).await
    }

    pub async fn service_disable(
        &self,
    ) -> Result<boxpilot_ipc::ServiceControlResponse, ClientError> {
        self.call_no_aux(HelperMethod::ServiceDisable, vec![]).await
    }

    pub async fn service_install_managed(
        &self,
    ) -> Result<boxpilot_ipc::ServiceInstallManagedResponse, ClientError> {
        self.call_no_aux(HelperMethod::ServiceInstallManaged, vec![])
            .await
    }

    pub async fn service_logs(
        &self,
        req: &boxpilot_ipc::ServiceLogsRequest,
    ) -> Result<boxpilot_ipc::ServiceLogsResponse, ClientError> {
        self.call_no_aux(HelperMethod::ServiceLogs, Self::encode_request(req))
            .await
    }

    /// Bundle activation — uses [`AuxStream`] to ship the tar payload via
    /// the platform's preferred byte-transfer (Linux: memfd FD-pass).
    pub async fn profile_activate_bundle(
        &self,
        req: &boxpilot_ipc::ActivateBundleRequest,
        aux: AuxStream,
    ) -> Result<boxpilot_ipc::ActivateBundleResponse, ClientError> {
        let body = Self::encode_request(req);
        let resp_bytes = self
            .ipc
            .call(HelperMethod::ProfileActivateBundle, body, aux)
            .await?;
        serde_json::from_slice(&resp_bytes).map_err(|e| ClientError::Decode(e.to_string()))
    }

    pub async fn profile_rollback_release(
        &self,
        req: &boxpilot_ipc::RollbackRequest,
    ) -> Result<boxpilot_ipc::ActivateBundleResponse, ClientError> {
        self.call_no_aux(
            HelperMethod::ProfileRollbackRelease,
            Self::encode_request(req),
        )
        .await
    }

    pub async fn legacy_observe_service(
        &self,
    ) -> Result<boxpilot_ipc::LegacyObserveServiceResponse, ClientError> {
        self.call_no_aux(HelperMethod::LegacyObserveService, vec![])
            .await
    }

    pub async fn legacy_migrate_service(
        &self,
        req: &boxpilot_ipc::LegacyMigrateRequest,
    ) -> Result<boxpilot_ipc::LegacyMigrateResponse, ClientError> {
        self.call_no_aux(
            HelperMethod::LegacyMigrateService,
            Self::encode_request(req),
        )
        .await
    }

    pub async fn diagnostics_export_redacted(
        &self,
    ) -> Result<boxpilot_ipc::DiagnosticsExportResponse, ClientError> {
        self.call_no_aux(HelperMethod::DiagnosticsExportRedacted, vec![])
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn helper_error_code_covers_every_variant() {
        // If a new HelperError variant is added without a code mapping,
        // this test will not catch it directly — instead
        // `helper_error_code` will fail to compile because match arms are
        // exhaustive. This test pins down the codes we already have so
        // accidental renames are caught.
        let cases: &[(HelperError, &str)] = &[
            (HelperError::NotImplemented, "not_implemented"),
            (HelperError::NotAuthorized, "not_authorized"),
            (HelperError::NotController, "not_controller"),
            (HelperError::ControllerOrphaned, "controller_orphaned"),
            (HelperError::ControllerNotSet, "controller_not_set"),
            (HelperError::Busy, "busy"),
            (HelperError::ActiveCorrupt, "active_corrupt"),
            (HelperError::ReleaseAlreadyActive, "release_already_active"),
            (HelperError::RollbackTargetMissing, "rollback_target_missing"),
        ];
        for (err, expected) in cases {
            assert_eq!(helper_error_code(err), *expected);
        }
    }

    #[test]
    fn helper_error_code_matches_serde_wire_tag() {
        // `HelperError` derives `Serialize` with `tag = "code", rename_all
        // = "snake_case"`. The mapping in `helper_error_code` must agree
        // with that wire form.
        let err = HelperError::UnsupportedSchemaVersion { got: 99 };
        let v = serde_json::to_value(&err).unwrap();
        assert_eq!(v["code"], helper_error_code(&err));

        let err = HelperError::SingboxCheckFailed {
            exit: 1,
            stderr_tail: "x".into(),
        };
        let v = serde_json::to_value(&err).unwrap();
        assert_eq!(v["code"], helper_error_code(&err));
    }
}
