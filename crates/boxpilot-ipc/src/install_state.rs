//! `install-state.json` ledger (spec §5.4). Lives at
//! `/var/lib/boxpilot/install-state.json` and is the single source of
//! truth for which cores BoxPilot has installed or adopted.

use crate::error::{HelperError, HelperResult};
use serde::{Deserialize, Serialize};

pub const INSTALL_STATE_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct InstallState {
    pub schema_version: u32,
    #[serde(default)]
    pub managed_cores: Vec<ManagedCoreEntry>,
    #[serde(default)]
    pub adopted_cores: Vec<AdoptedCoreEntry>,
    #[serde(default)]
    pub current_managed_core: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManagedCoreEntry {
    pub version: String,
    pub path: String,
    pub sha256: String,
    pub installed_at: String,
    pub source: String, // e.g. "github-sagernet"
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdoptedCoreEntry {
    pub label: String,
    pub path: String,
    pub sha256: String,
    pub adopted_from: String,
    pub adopted_at: String,
}

impl InstallState {
    pub fn empty() -> Self {
        Self {
            schema_version: INSTALL_STATE_SCHEMA_VERSION,
            managed_cores: vec![],
            adopted_cores: vec![],
            current_managed_core: None,
        }
    }

    pub fn parse(text: &str) -> HelperResult<Self> {
        #[derive(Deserialize)]
        struct Peek {
            schema_version: u32,
        }
        let peek: Peek = serde_json::from_str(text).map_err(|e| HelperError::Ipc {
            message: format!("install-state parse: {e}"),
        })?;
        if peek.schema_version != INSTALL_STATE_SCHEMA_VERSION {
            return Err(HelperError::UnsupportedSchemaVersion {
                got: peek.schema_version,
            });
        }
        serde_json::from_str(text).map_err(|e| HelperError::Ipc {
            message: format!("install-state parse: {e}"),
        })
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).expect("InstallState serializes")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_round_trip() {
        let s = InstallState::empty();
        let text = s.to_json();
        let back = InstallState::parse(&text).unwrap();
        assert_eq!(back, s);
    }

    #[test]
    fn rejects_unknown_schema() {
        let r = InstallState::parse(r#"{"schema_version": 99}"#);
        assert!(matches!(
            r,
            Err(HelperError::UnsupportedSchemaVersion { got: 99 })
        ));
    }

    #[test]
    fn full_round_trip() {
        let s = InstallState {
            schema_version: 1,
            managed_cores: vec![ManagedCoreEntry {
                version: "1.10.0".into(),
                path: "/var/lib/boxpilot/cores/1.10.0/sing-box".into(),
                sha256: "abc".into(),
                installed_at: "2026-04-28T10:00:00-07:00".into(),
                source: "github-sagernet".into(),
            }],
            adopted_cores: vec![AdoptedCoreEntry {
                label: "adopted-2026-04-28T10-00-00Z".into(),
                path: "/var/lib/boxpilot/cores/adopted-2026-04-28T10-00-00Z/sing-box".into(),
                sha256: "def".into(),
                adopted_from: "/usr/local/bin/sing-box".into(),
                adopted_at: "2026-04-28T10:00:00-07:00".into(),
            }],
            current_managed_core: Some("1.10.0".into()),
        };
        let back = InstallState::parse(&s.to_json()).unwrap();
        assert_eq!(back, s);
    }
}
