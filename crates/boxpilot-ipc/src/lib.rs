pub mod method;
pub use method::HelperMethod;

pub mod error;
pub use error::{HelperError, HelperResult};

pub mod response;
pub use response::{ControllerStatus, ServiceStatusResponse, UnitState};

pub mod config;
pub use config::{BoxpilotConfig, CoreState, CURRENT_SCHEMA_VERSION};

pub mod core;
pub use core::{
    ArchRequest, CoreAdoptRequest, CoreDiscoverResponse, CoreInstallRequest, CoreInstallResponse,
    CoreKind, CoreRollbackRequest, CoreSource, DiscoveredCore, InstallSourceJson, VersionRequest,
};

pub mod install_state;
pub use install_state::{
    AdoptedCoreEntry, InstallState, ManagedCoreEntry, INSTALL_STATE_SCHEMA_VERSION,
};

pub mod service;
pub use service::{
    ServiceControlResponse, ServiceInstallManagedResponse, ServiceLogsRequest, ServiceLogsResponse,
    SERVICE_LOGS_DEFAULT_LINES, SERVICE_LOGS_MAX_LINES,
};

pub mod profile;
pub use profile::{
    ActivateBundleRequest, ActivateBundleResponse, ActivateOutcome, ActivationManifest, AssetEntry,
    RollbackRequest, SourceKind, VerifySummary, ACTIVATION_MANIFEST_SCHEMA_VERSION,
    BUNDLE_MAX_FILE_BYTES, BUNDLE_MAX_FILE_COUNT, BUNDLE_MAX_NESTING_DEPTH, BUNDLE_MAX_TOTAL_BYTES,
};

pub mod legacy;
pub use legacy::{
    ConfigPathKind, LegacyMigrateCutoverResponse, LegacyMigratePrepareResponse,
    LegacyMigrateRequest, LegacyMigrateResponse, LegacyObserveServiceResponse, MigratedAsset,
    LEGACY_UNIT_NAME,
};

pub mod home;
pub use home::{
    ActiveProfileSnapshot, CoreSnapshot, HomeStatusResponse, HOME_STATUS_SCHEMA_VERSION,
};

pub mod redact;
pub use redact::{redact_singbox_config, MAX_DEPTH as REDACT_MAX_DEPTH, REDACTED};

#[cfg(test)]
mod tests {
    #[test]
    fn crate_compiles() {
        assert_eq!(2 + 2, 4);
    }
}
