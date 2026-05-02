//! Environment-variable access abstracted so `Paths` (§5.1) can build
//! platform-correct roots without each caller doing OS-specific lookups.
//! Linux: reads `$XDG_DATA_HOME` and `$HOME`. Windows: reads
//! `%ProgramData%` and `%LocalAppData%`. Test fakes inject a static map.

use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum EnvError {
    #[error("required environment variable missing: {0}")]
    Missing(&'static str),
    #[error("env value is not valid UTF-8: {0}")]
    NotUtf8(&'static str),
}

pub trait EnvProvider: Send + Sync {
    /// System-wide data root.
    /// Linux: `/` (returned as `PathBuf::from("/")`).
    /// Windows: `%ProgramData%\BoxPilot` (typically `C:\ProgramData\BoxPilot`).
    fn system_root(&self) -> Result<PathBuf, EnvError>;

    /// Per-user data root.
    /// Linux: `$XDG_DATA_HOME/boxpilot` if `XDG_DATA_HOME` set, else
    /// `$HOME/.local/share/boxpilot`.
    /// Windows: `%LocalAppData%\BoxPilot`.
    fn user_root(&self) -> Result<PathBuf, EnvError>;
}
