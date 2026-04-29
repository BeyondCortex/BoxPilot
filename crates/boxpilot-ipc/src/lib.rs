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

#[cfg(test)]
mod tests {
    #[test]
    fn crate_compiles() {
        assert_eq!(2 + 2, 4);
    }
}
