//! Windows `FsMetadataProvider` stub. Sub-project #2 wires this to
//! `GetFileInformationByHandleEx` + SID-based ownership lookup. PR 7 will
//! introduce a Windows-shaped `TrustChecker` trait that does not depend on
//! POSIX uid/gid/mode semantics.

use crate::traits::fs_meta::{FileStat, FsMetadataProvider};
use std::io;
use std::path::{Path, PathBuf};

pub struct StdFsMetadataProvider;

impl FsMetadataProvider for StdFsMetadataProvider {
    fn stat(&self, _path: &Path) -> io::Result<FileStat> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "FsMetadataProvider stub: implemented in Sub-project #2",
        ))
    }
    fn read_link(&self, _path: &Path) -> io::Result<PathBuf> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "FsMetadataProvider stub: implemented in Sub-project #2",
        ))
    }
}
