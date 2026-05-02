//! Atomic "active release" pointer. Linux: symlink with rename(2). Windows:
//! marker JSON file with MoveFileEx(MOVEFILE_REPLACE_EXISTING). Per spec §5.3.

use async_trait::async_trait;
use boxpilot_ipc::HelperError;
use std::path::PathBuf;

#[async_trait]
pub trait ActivePointer: Send + Sync {
    /// Returns the current active release id, or `None` when no active
    /// pointer exists. Linux reads the symlink target's file_name; Windows
    /// reads the marker JSON's `release_id` field.
    async fn read(&self) -> Result<Option<String>, HelperError>;

    /// Atomically install `release_id` as the active pointer. Linux creates
    /// `active.new` and `rename(2)`s onto `active`. Windows writes
    /// `active.json.new` and `MoveFileEx(MOVEFILE_REPLACE_EXISTING)`s onto
    /// `active.json`.
    async fn set(&self, release_id: &str) -> Result<(), HelperError>;

    /// Resolve the active pointer to the on-disk release directory it
    /// points at, or `None` when missing. The returned path is the symlink
    /// target on Linux (NOT canonicalized) or `releases_dir/<release_id>`
    /// reconstructed from the marker JSON on Windows.
    async fn active_resolved(&self) -> Result<Option<PathBuf>, HelperError>;

    /// Path math: `releases_dir.join(release_id)`. Lives on the trait so
    /// callers don't need to plumb both the pointer and the releases root.
    fn release_dir(&self, release_id: &str) -> PathBuf;
}
