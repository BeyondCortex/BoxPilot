//! Linux [`CurrentPointer`] backed by a symlink at `link.new` + `rename(2)`.
//! Matches the atomic pattern used for `cores/current` in spec §7.2 step 14e.

use crate::traits::current::CurrentPointer;
use std::path::Path;

pub struct SymlinkCurrentPointer;

impl CurrentPointer for SymlinkCurrentPointer {
    fn set_atomic(&self, link: &Path, target: &Path) -> std::io::Result<()> {
        let tmp = link.with_extension("new");
        // Best-effort cleanup of leftover .new from a prior crash.
        let _ = std::fs::remove_file(&tmp);
        std::os::unix::fs::symlink(target, &tmp)?;
        std::fs::rename(&tmp, link)?;
        Ok(())
    }
}
