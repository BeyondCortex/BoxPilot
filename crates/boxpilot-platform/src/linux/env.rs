use crate::traits::env::{EnvError, EnvProvider};
use std::path::PathBuf;

/// Reads from the process environment using `std::env::var_os`.
pub struct StdEnv;

impl EnvProvider for StdEnv {
    fn system_root(&self) -> Result<PathBuf, EnvError> {
        Ok(PathBuf::from("/"))
    }

    fn user_root(&self) -> Result<PathBuf, EnvError> {
        if let Some(xdg) = std::env::var_os("XDG_DATA_HOME") {
            return Ok(PathBuf::from(xdg).join("boxpilot"));
        }
        let home = std::env::var_os("HOME").ok_or(EnvError::Missing("HOME"))?;
        Ok(PathBuf::from(home).join(".local/share/boxpilot"))
    }
}
