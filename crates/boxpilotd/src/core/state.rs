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

#[allow(dead_code)]
pub async fn write_state(path: &Path, state: &InstallState) -> HelperResult<()> {
    let parent = path.parent().ok_or_else(|| HelperError::Ipc {
        message: format!("no parent: {path:?}"),
    })?;
    tokio::fs::create_dir_all(parent)
        .await
        .map_err(|e| HelperError::Ipc {
            message: format!("mkdir {parent:?}: {e}"),
        })?;
    let tmp = path.with_extension("json.new");
    let bytes = state.to_json();
    tokio::fs::write(&tmp, &bytes)
        .await
        .map_err(|e| HelperError::Ipc {
            message: format!("write {tmp:?}: {e}"),
        })?;
    // fsync the file before rename to ensure the bytes hit disk before
    // a concurrent crash exposes the new inode.
    let f = tokio::fs::OpenOptions::new()
        .read(true)
        .open(&tmp)
        .await
        .map_err(|e| HelperError::Ipc {
            message: format!("open for fsync {tmp:?}: {e}"),
        })?;
    f.sync_all().await.map_err(|e| HelperError::Ipc {
        message: format!("fsync {tmp:?}: {e}"),
    })?;
    drop(f);
    tokio::fs::rename(&tmp, path)
        .await
        .map_err(|e| HelperError::Ipc {
            message: format!("rename {tmp:?} -> {path:?}: {e}"),
        })?;
    Ok(())
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

#[cfg(test)]
mod write_tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn round_trip() {
        let dir = tempdir().unwrap();
        let p = dir.path().join("install-state.json");
        let mut s = InstallState::empty();
        s.current_managed_core = Some("1.10.0".into());
        write_state(&p, &s).await.unwrap();
        let back = read_state(&p).await.unwrap();
        assert_eq!(back, s);
    }

    #[tokio::test]
    async fn no_temp_left_after_success() {
        let dir = tempdir().unwrap();
        let p = dir.path().join("install-state.json");
        write_state(&p, &InstallState::empty()).await.unwrap();
        let entries: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().to_string())
            .collect();
        assert_eq!(entries, vec!["install-state.json".to_string()]);
    }
}
