use crate::error::{HelperError, HelperResult};
use serde::{Deserialize, Serialize};

pub const CURRENT_SCHEMA_VERSION: u32 = 1;

/// Maps to `/etc/boxpilot/boxpilot.toml` (spec §5.3). Optional fields stay
/// `None` until the corresponding install/activate plan adds them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BoxpilotConfig {
    pub schema_version: u32,
    #[serde(default = "default_target_service")]
    pub target_service: String,
    #[serde(default)]
    pub core_path: Option<String>,
    #[serde(default)]
    pub core_state: Option<CoreState>,
    #[serde(default)]
    pub controller_uid: Option<u32>,
    #[serde(default)]
    pub active_profile_id: Option<String>,
    #[serde(default)]
    pub active_profile_name: Option<String>,
    #[serde(default)]
    pub active_profile_sha256: Option<String>,
    #[serde(default)]
    pub active_release_id: Option<String>,
    #[serde(default)]
    pub activated_at: Option<String>,
}

fn default_target_service() -> String {
    "boxpilot-sing-box.service".to_string()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CoreState {
    External,
    ManagedInstalled,
    ManagedAdopted,
}

impl BoxpilotConfig {
    /// Parse from the on-disk TOML. Rejects unknown `schema_version` per §5.3.
    pub fn parse(text: &str) -> HelperResult<Self> {
        // Step 1: peek schema_version without committing to the full schema,
        // so a future-version file produces a clean `UnsupportedSchemaVersion`
        // error rather than an unrelated "unknown field" error.
        #[derive(Deserialize)]
        struct Peek {
            schema_version: u32,
        }
        let peek: Peek = toml::from_str(text)
            .map_err(|e| HelperError::Ipc { message: format!("config parse: {e}") })?;
        if peek.schema_version != CURRENT_SCHEMA_VERSION {
            return Err(HelperError::UnsupportedSchemaVersion { got: peek.schema_version });
        }
        let cfg: BoxpilotConfig = toml::from_str(text)
            .map_err(|e| HelperError::Ipc { message: format!("config parse: {e}") })?;
        Ok(cfg)
    }

    pub fn to_toml(&self) -> String {
        // Encoded back via `toml::to_string` for atomic-write callers in
        // future plans (the activation pipeline writes boxpilot.toml.new
        // and renames it into place — see spec §10 step 13).
        toml::to_string(self).expect("BoxpilotConfig serializes")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const V1_FULL: &str = r#"
schema_version = 1
target_service = "boxpilot-sing-box.service"
core_path = "/var/lib/boxpilot/cores/current/sing-box"
core_state = "managed-installed"
controller_uid = 1000
"#;

    #[test]
    fn parses_v1_with_optional_fields_missing() {
        let cfg = BoxpilotConfig::parse("schema_version = 1\n").unwrap();
        assert_eq!(cfg.schema_version, 1);
        assert_eq!(cfg.target_service, "boxpilot-sing-box.service");
        assert_eq!(cfg.controller_uid, None);
    }

    #[test]
    fn parses_v1_full() {
        let cfg = BoxpilotConfig::parse(V1_FULL).unwrap();
        assert_eq!(cfg.controller_uid, Some(1000));
        assert_eq!(cfg.core_state, Some(CoreState::ManagedInstalled));
    }

    #[test]
    fn rejects_unknown_schema_version() {
        let r = BoxpilotConfig::parse("schema_version = 2\n");
        assert!(matches!(r, Err(HelperError::UnsupportedSchemaVersion { got: 2 })));
    }

    #[test]
    fn rejects_zero_or_missing_schema_version() {
        let r = BoxpilotConfig::parse("");
        assert!(matches!(r, Err(HelperError::Ipc { .. })));
    }

    #[test]
    fn round_trip_via_toml() {
        let cfg = BoxpilotConfig::parse(V1_FULL).unwrap();
        let text = cfg.to_toml();
        let back = BoxpilotConfig::parse(&text).unwrap();
        assert_eq!(back, cfg);
    }
}
