//! Linux `FsMetadataProvider` impl backed by `std::fs::symlink_metadata` and
//! `std::os::unix::fs::MetadataExt`.

use crate::traits::fs_meta::{FileKind, FileStat, FsMetadataProvider};
use std::io;
use std::path::{Path, PathBuf};

pub struct StdFsMetadataProvider;

impl FsMetadataProvider for StdFsMetadataProvider {
    fn stat(&self, path: &Path) -> io::Result<FileStat> {
        use std::os::unix::fs::MetadataExt;
        let md = std::fs::symlink_metadata(path)?;
        let ft = md.file_type();
        let kind = if ft.is_symlink() {
            FileKind::Symlink
        } else if ft.is_dir() {
            FileKind::Directory
        } else if ft.is_file() {
            FileKind::Regular
        } else {
            FileKind::Other
        };
        Ok(FileStat {
            uid: md.uid(),
            gid: md.gid(),
            mode: md.mode() & 0o7777,
            kind,
        })
    }
    fn read_link(&self, path: &Path) -> io::Result<PathBuf> {
        std::fs::read_link(path)
    }
}
