//! In-memory `FsMetadataProvider` test double. Mirrors the §6.5 trust walker
//! semantics: a path inserted via `put` becomes addressable by `stat`, and a
//! symlink target is registered via the `links` map.

use crate::traits::fs_meta::{FileKind, FileStat, FsMetadataProvider};
use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

#[derive(Default)]
#[allow(dead_code)]
pub struct FakeFs {
    pub stats: Mutex<HashMap<PathBuf, FileStat>>,
    pub links: Mutex<HashMap<PathBuf, PathBuf>>,
}

impl FakeFs {
    #[allow(dead_code)]
    pub fn root_dir() -> FileStat {
        FileStat {
            uid: 0,
            gid: 0,
            mode: 0o755,
            kind: FileKind::Directory,
        }
    }
    #[allow(dead_code)]
    pub fn root_bin() -> FileStat {
        FileStat {
            uid: 0,
            gid: 0,
            mode: 0o755,
            kind: FileKind::Regular,
        }
    }
    #[allow(dead_code)]
    pub fn put(&self, path: impl AsRef<Path>, stat: FileStat) {
        self.stats
            .lock()
            .unwrap()
            .insert(path.as_ref().to_path_buf(), stat);
    }
}

impl FsMetadataProvider for FakeFs {
    fn stat(&self, path: &Path) -> io::Result<FileStat> {
        self.stats
            .lock()
            .unwrap()
            .get(path)
            .cloned()
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, path.display().to_string()))
    }
    fn read_link(&self, path: &Path) -> io::Result<PathBuf> {
        self.links
            .lock()
            .unwrap()
            .get(path)
            .cloned()
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "not a symlink"))
    }
}
