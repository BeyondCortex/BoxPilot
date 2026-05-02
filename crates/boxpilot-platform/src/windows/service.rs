//! Windows `ServiceManager` stub. Sub-project #1 ships unimplemented; the
//! real SCM-backed impl lands in Sub-project #2 alongside the trait reshape
//! (per COQ4).

use crate::traits::service::ServiceManager;
use async_trait::async_trait;
use boxpilot_ipc::{HelperError, UnitState};

pub struct ScmServiceManager;

#[async_trait]
impl ServiceManager for ScmServiceManager {
    async fn unit_state(&self, _unit_name: &str) -> Result<UnitState, HelperError> {
        Err(HelperError::NotImplemented)
    }
    async fn start_unit(&self, _: &str) -> Result<(), HelperError> {
        Err(HelperError::NotImplemented)
    }
    async fn stop_unit(&self, _: &str) -> Result<(), HelperError> {
        Err(HelperError::NotImplemented)
    }
    async fn restart_unit(&self, _: &str) -> Result<(), HelperError> {
        Err(HelperError::NotImplemented)
    }
    async fn enable_unit_files(&self, _: &[String]) -> Result<(), HelperError> {
        Err(HelperError::NotImplemented)
    }
    async fn disable_unit_files(&self, _: &[String]) -> Result<(), HelperError> {
        Err(HelperError::NotImplemented)
    }
    async fn reload(&self) -> Result<(), HelperError> {
        Err(HelperError::NotImplemented)
    }
    async fn fragment_path(&self, _: &str) -> Result<Option<String>, HelperError> {
        Err(HelperError::NotImplemented)
    }
    async fn unit_file_state(&self, _: &str) -> Result<Option<String>, HelperError> {
        Err(HelperError::NotImplemented)
    }
}
