//! Linux `FsPermissions` impl: `chmod 0700` for directories, `chmod 0600` for
//! regular files.

use crate::traits::fs_perms::{FsPermissions, PathKind};
use async_trait::async_trait;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

pub struct ChmodFsPermissions;

#[async_trait]
impl FsPermissions for ChmodFsPermissions {
    async fn restrict_to_owner(&self, path: &Path, kind: PathKind) -> std::io::Result<()> {
        let mode = match kind {
            PathKind::Directory => 0o700,
            PathKind::File => 0o600,
        };
        let perms = std::fs::Permissions::from_mode(mode);
        tokio::fs::set_permissions(path, perms).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn restricts_dir_to_0700() {
        let tmp = tempdir().unwrap();
        let dir = tmp.path().join("d");
        std::fs::create_dir(&dir).unwrap();
        ChmodFsPermissions
            .restrict_to_owner(&dir, PathKind::Directory)
            .await
            .unwrap();
        let m = std::fs::metadata(&dir).unwrap();
        assert_eq!(m.permissions().mode() & 0o777, 0o700);
    }

    #[tokio::test]
    async fn restricts_file_to_0600() {
        let tmp = tempdir().unwrap();
        let f = tmp.path().join("f");
        std::fs::write(&f, b"x").unwrap();
        ChmodFsPermissions
            .restrict_to_owner(&f, PathKind::File)
            .await
            .unwrap();
        let m = std::fs::metadata(&f).unwrap();
        assert_eq!(m.permissions().mode() & 0o777, 0o600);
    }
}
