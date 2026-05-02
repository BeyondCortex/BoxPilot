//! Windows `VersionChecker` stub. Sub-project #2 will exec
//! `sing-box.exe --version` (or `version`, matching upstream's actual flag) and
//! parse stdout the same way as Linux.

use crate::traits::version::VersionChecker;
use std::io;
use std::path::Path;

pub struct ProcessVersionChecker;

impl VersionChecker for ProcessVersionChecker {
    fn check(&self, _binary: &Path) -> io::Result<String> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "VersionChecker stub: implemented in Sub-project #2",
        ))
    }
}
