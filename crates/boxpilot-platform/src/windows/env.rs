use crate::traits::env::{EnvError, EnvProvider};
use std::path::PathBuf;

pub struct StdEnv;

impl EnvProvider for StdEnv {
    fn system_root(&self) -> Result<PathBuf, EnvError> {
        let pd = std::env::var_os("ProgramData").ok_or(EnvError::Missing("ProgramData"))?;
        Ok(PathBuf::from(pd).join("BoxPilot"))
    }

    fn user_root(&self) -> Result<PathBuf, EnvError> {
        let lad = std::env::var_os("LocalAppData").ok_or(EnvError::Missing("LocalAppData"))?;
        Ok(PathBuf::from(lad).join("BoxPilot"))
    }
}
