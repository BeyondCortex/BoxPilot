//! Windows [`ActivePointer`] stub. Real impl in PR 12 will use a marker
//! JSON file at `active.json` and `MoveFileEx(MOVEFILE_REPLACE_EXISTING)`
//! for the atomic swap (Windows does not have native rename-onto-symlink
//! atomics for our use case). Per spec §5.3.

use crate::traits::active::ActivePointer;
use async_trait::async_trait;
use boxpilot_ipc::HelperError;
use std::path::PathBuf;

pub struct MarkerFileActivePointer {
    /// On Windows this is the marker JSON path (e.g. `active.json`).
    pub active: PathBuf,
    pub releases_dir: PathBuf,
}

#[async_trait]
impl ActivePointer for MarkerFileActivePointer {
    async fn read(&self) -> Result<Option<String>, HelperError> {
        Err(HelperError::NotImplemented)
    }

    async fn set(&self, _release_id: &str) -> Result<(), HelperError> {
        Err(HelperError::NotImplemented)
    }

    async fn active_resolved(&self) -> Result<Option<PathBuf>, HelperError> {
        Err(HelperError::NotImplemented)
    }

    fn release_dir(&self, release_id: &str) -> PathBuf {
        self.releases_dir.join(release_id)
    }
}
