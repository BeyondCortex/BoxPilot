//! Forward-compatible reservation for `~/.local/share/boxpilot/ui-state.json`.
//!
//! Plan #4 ships the schema, the read/write helpers, and the path; no Tauri
//! command wires this up yet. Plan #7 (GUI: Home / Profiles / Settings tabs)
//! will persist `selected_profile_id` and any later UI state across launches.

use serde::{Deserialize, Serialize};
use std::path::Path;

use crate::store::write_file_0600_atomic;

pub const UI_STATE_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UiState {
    pub schema_version: u32,
    #[serde(default)]
    pub selected_profile_id: Option<String>,
}

impl Default for UiState {
    fn default() -> Self {
        Self {
            schema_version: UI_STATE_SCHEMA_VERSION,
            selected_profile_id: None,
        }
    }
}

pub fn read_ui_state(path: &Path) -> std::io::Result<UiState> {
    match std::fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(UiState::default()),
        Err(e) => Err(e),
    }
}

pub fn write_ui_state(path: &Path, state: &UiState) -> std::io::Result<()> {
    let bytes = serde_json::to_vec_pretty(state)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    write_file_0600_atomic(path, &bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn missing_yields_default() {
        let tmp = tempfile::tempdir().unwrap();
        let s = read_ui_state(&tmp.path().join("ui-state.json")).unwrap();
        assert_eq!(s, UiState::default());
    }

    #[test]
    fn round_trip() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("ui-state.json");
        let s = UiState {
            selected_profile_id: Some("p1".into()),
            ..UiState::default()
        };
        write_ui_state(&path, &s).unwrap();
        assert_eq!(read_ui_state(&path).unwrap(), s);
    }

    #[test]
    fn unknown_fields_in_input_are_ignored() {
        let json = r#"{"schema_version":1,"selected_profile_id":"x","future_field":42}"#;
        let s: UiState = serde_json::from_str(json).unwrap();
        assert_eq!(s.selected_profile_id.as_deref(), Some("x"));
    }
}
