//! Windows [`CurrentPointer`] stub. Real impl in Sub-project #2 PR 3.5 will
//! use a junction + `MoveFileExW(MOVEFILE_REPLACE_EXISTING)`. Per spec §7.2.

use crate::traits::current::CurrentPointer;
use std::path::Path;

pub struct JunctionCurrentPointer;

impl CurrentPointer for JunctionCurrentPointer {
    fn set_atomic(&self, _link: &Path, _target: &Path) -> std::io::Result<()> {
        Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "JunctionCurrentPointer: not yet implemented (Sub-project #2 PR 3.5)",
        ))
    }
}
