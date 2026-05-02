//! Cross-platform in-memory fake [`ActivePointer`]. State lives in a
//! `Mutex<Option<String>>` so tests don't need a real filesystem to
//! exercise activate/rollback flows.

use crate::traits::active::ActivePointer;
use async_trait::async_trait;
use boxpilot_ipc::HelperError;
use std::path::PathBuf;
use std::sync::Mutex;

pub struct InMemoryActive {
    pub releases_dir: PathBuf,
    pub state: Mutex<Option<String>>,
}

impl InMemoryActive {
    /// Constructor — pointer starts unset; call [`ActivePointer::set`] to
    /// seed an initial release id in tests.
    pub fn under(releases_dir: impl Into<PathBuf>) -> Self {
        Self {
            releases_dir: releases_dir.into(),
            state: Mutex::new(None),
        }
    }
}

#[async_trait]
impl ActivePointer for InMemoryActive {
    async fn read(&self) -> Result<Option<String>, HelperError> {
        Ok(self.state.lock().unwrap().clone())
    }

    async fn set(&self, release_id: &str) -> Result<(), HelperError> {
        *self.state.lock().unwrap() = Some(release_id.to_string());
        Ok(())
    }

    async fn active_resolved(&self) -> Result<Option<PathBuf>, HelperError> {
        Ok(self
            .state
            .lock()
            .unwrap()
            .as_ref()
            .map(|id| self.releases_dir.join(id)))
    }

    fn release_dir(&self, release_id: &str) -> PathBuf {
        self.releases_dir.join(release_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn under_starts_unset() {
        let a = InMemoryActive::under("/tmp/r");
        assert!(a.read().await.unwrap().is_none());
        assert!(a.active_resolved().await.unwrap().is_none());
    }

    #[tokio::test]
    async fn set_then_read_and_resolve() {
        let a = InMemoryActive::under("/tmp/r");
        a.set("rel-1").await.unwrap();
        assert_eq!(a.read().await.unwrap().as_deref(), Some("rel-1"));
        assert_eq!(
            a.active_resolved().await.unwrap(),
            Some(PathBuf::from("/tmp/r/rel-1"))
        );
        assert_eq!(a.release_dir("rel-1"), PathBuf::from("/tmp/r/rel-1"));
    }

    #[tokio::test]
    async fn set_overwrites() {
        let a = InMemoryActive::under("/tmp/r");
        a.set("rel-1").await.unwrap();
        a.set("rel-2").await.unwrap();
        assert_eq!(a.read().await.unwrap().as_deref(), Some("rel-2"));
    }
}
