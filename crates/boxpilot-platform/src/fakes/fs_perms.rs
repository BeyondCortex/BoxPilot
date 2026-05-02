//! Recording test double: captures every `restrict_to_owner` call so tests
//! can assert which paths got locked down without touching real ACLs.

use crate::traits::fs_perms::{FsPermissions, PathKind};
use async_trait::async_trait;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

#[derive(Default)]
pub struct RecordingFsPermissions(Mutex<Vec<(PathBuf, PathKind)>>);

impl RecordingFsPermissions {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn calls(&self) -> Vec<(PathBuf, PathKind)> {
        self.0.lock().unwrap().clone()
    }
}

#[async_trait]
impl FsPermissions for RecordingFsPermissions {
    async fn restrict_to_owner(&self, path: &Path, kind: PathKind) -> std::io::Result<()> {
        self.0.lock().unwrap().push((path.to_path_buf(), kind));
        Ok(())
    }
}
