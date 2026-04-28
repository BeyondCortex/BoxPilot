//! Read/write for `/var/lib/boxpilot/install-state.json` (spec §5.4).
//! Atomic writes via tempfile + rename(2).

use boxpilot_ipc::{HelperError, HelperResult, InstallState};
use std::path::Path;

#[allow(dead_code)]
pub async fn read_state(path: &Path) -> HelperResult<InstallState> {
    match tokio::fs::read_to_string(path).await {
        Ok(text) => InstallState::parse(&text),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(InstallState::empty()),
        Err(e) => Err(HelperError::Ipc {
            message: format!("read {path:?}: {e}"),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn missing_returns_empty() {
        let dir = tempdir().unwrap();
        let s = read_state(&dir.path().join("install-state.json"))
            .await
            .unwrap();
        assert_eq!(s, InstallState::empty());
    }

    #[tokio::test]
    async fn parses_v1_file() {
        let dir = tempdir().unwrap();
        let p = dir.path().join("install-state.json");
        tokio::fs::write(&p, r#"{"schema_version":1}"#)
            .await
            .unwrap();
        let s = read_state(&p).await.unwrap();
        assert_eq!(s.schema_version, 1);
    }

    #[tokio::test]
    async fn rejects_unknown_version() {
        let dir = tempdir().unwrap();
        let p = dir.path().join("install-state.json");
        tokio::fs::write(&p, r#"{"schema_version":99}"#)
            .await
            .unwrap();
        let r = read_state(&p).await;
        assert!(matches!(
            r,
            Err(HelperError::UnsupportedSchemaVersion { got: 99 })
        ));
    }
}
