//! LRU eviction for /var/cache/boxpilot/diagnostics.

use std::path::Path;
use std::time::SystemTime;

/// Total size on disk (in bytes) of `*.tar.gz` files in `dir`. Tempfiles
/// (names beginning with `.`) are intentionally skipped — they belong to
/// in-progress writes and should not be counted.
pub fn dir_size(dir: &Path) -> std::io::Result<u64> {
    if !dir.exists() {
        return Ok(0);
    }
    let mut total = 0u64;
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        if !is_visible_tarball(&entry) {
            continue;
        }
        total = total.saturating_add(entry.metadata()?.len());
    }
    Ok(total)
}

/// Delete the oldest visible `*.tar.gz` files until the directory's total
/// size is at or below `cap_bytes`. Tempfiles are never touched. Per-file
/// deletion failures are logged and skipped — the loop tries the next
/// oldest file rather than aborting.
pub fn evict_to_cap(dir: &Path, cap_bytes: u64) -> std::io::Result<()> {
    if !dir.exists() {
        return Ok(());
    }
    loop {
        let total = dir_size(dir)?;
        if total <= cap_bytes {
            return Ok(());
        }
        let oldest = oldest_tarball(dir)?;
        let Some((path, _)) = oldest else {
            // Nothing visible to evict but we're still over cap — bail
            // rather than spin. The caller may want to log this.
            return Ok(());
        };
        if let Err(e) = std::fs::remove_file(&path) {
            tracing::warn!(target: "diagnostics::gc", path = %path.display(), error = %e, "evict failed");
            // Avoid an infinite loop: break to caller; it'll retry next call.
            return Ok(());
        }
    }
}

fn is_visible_tarball(entry: &std::fs::DirEntry) -> bool {
    let name = entry.file_name();
    let Some(s) = name.to_str() else { return false };
    !s.starts_with('.') && s.ends_with(".tar.gz")
}

fn oldest_tarball(dir: &Path) -> std::io::Result<Option<(std::path::PathBuf, SystemTime)>> {
    let mut oldest: Option<(std::path::PathBuf, SystemTime)> = None;
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        if !is_visible_tarball(&entry) {
            continue;
        }
        let mtime = entry.metadata()?.modified()?;
        match &oldest {
            Some((_, t)) if *t <= mtime => {}
            _ => oldest = Some((entry.path(), mtime)),
        }
    }
    Ok(oldest)
}

#[cfg(test)]
mod tests {
    use super::*;
    use filetime::{set_file_mtime, FileTime};
    use std::fs;
    use std::time::{Duration, UNIX_EPOCH};
    use tempfile::tempdir;

    fn make_tarball(dir: &Path, name: &str, size: usize, mtime_secs: u64) {
        let path = dir.join(name);
        fs::write(&path, vec![0u8; size]).unwrap();
        set_file_mtime(
            &path,
            FileTime::from_system_time(UNIX_EPOCH + Duration::from_secs(mtime_secs)),
        )
        .unwrap();
    }

    #[test]
    fn evict_drops_oldest_first_until_under_cap() {
        let tmp = tempdir().unwrap();
        // Three 100-byte files; cap = 250 → expect oldest (mtime=1) deleted.
        make_tarball(tmp.path(), "a.tar.gz", 100, 1);
        make_tarball(tmp.path(), "b.tar.gz", 100, 2);
        make_tarball(tmp.path(), "c.tar.gz", 100, 3);
        evict_to_cap(tmp.path(), 250).unwrap();
        assert!(!tmp.path().join("a.tar.gz").exists(), "oldest should go");
        assert!(tmp.path().join("b.tar.gz").exists());
        assert!(tmp.path().join("c.tar.gz").exists());
    }

    #[test]
    fn evict_skips_tempfiles_beginning_with_dot() {
        let tmp = tempdir().unwrap();
        make_tarball(tmp.path(), "a.tar.gz", 100, 1);
        make_tarball(tmp.path(), ".tmpXYZ", 9999, 0);
        evict_to_cap(tmp.path(), 0).unwrap();
        assert!(!tmp.path().join("a.tar.gz").exists());
        assert!(tmp.path().join(".tmpXYZ").exists(), "tempfile preserved");
    }

    #[test]
    fn evict_no_op_when_under_cap() {
        let tmp = tempdir().unwrap();
        make_tarball(tmp.path(), "a.tar.gz", 100, 1);
        evict_to_cap(tmp.path(), 1024).unwrap();
        assert!(tmp.path().join("a.tar.gz").exists());
    }

    #[test]
    fn evict_on_missing_dir_is_ok() {
        let tmp = tempdir().unwrap();
        let missing = tmp.path().join("does-not-exist");
        evict_to_cap(&missing, 100).unwrap();
        assert_eq!(dir_size(&missing).unwrap(), 0);
    }
}
