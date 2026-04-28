pub mod method;
pub use method::HelperMethod;

pub mod error;
pub use error::{HelperError, HelperResult};

pub mod response;
pub use response::{ControllerStatus, ServiceStatusResponse, UnitState};

pub mod config;
pub use config::{BoxpilotConfig, CoreState, CURRENT_SCHEMA_VERSION};

#[cfg(test)]
mod tests {
    #[test]
    fn crate_compiles() {
        assert_eq!(2 + 2, 4);
    }
}
