//! Spec §10 retention policy:
//!  - always keep `active`
//!  - always keep `previous`
//!  - keep ≤10 most recent AND total ≤ 2 GiB; whichever bound hits first wins
//!  - delete oldest first
//!  - skip the active and previous targets always

use crate::paths::Paths;
use std::path::PathBuf;
use std::time::SystemTime;
use tracing::{info, warn};

const KEEP_COUNT: usize = 10;
pub const KEEP_BYTES: u64 = 2 * 1024 * 1024 * 1024; // 2 GiB

#[derive(Debug, Default, PartialEq, Eq)]
pub struct GcReport {
    pub deleted: Vec<String>,
    pub errors: u32,
}

pub fn run(paths: &Paths, keep_active: Option<&str>, keep_previous: Option<&str>) -> GcReport {
    let mut report = GcReport::default();
    let releases = paths.releases_dir();
    if !releases.exists() {
        return report;
    }
    let mut entries: Vec<(String, PathBuf, SystemTime, u64)> = Vec::new();
    let dir = match std::fs::read_dir(&releases) {
        Ok(d) => d,
        Err(e) => {
            warn!("read_dir releases: {e}");
            return report;
        }
    };
    for entry in dir.flatten() {
        let path = entry.path();
        let name = match entry.file_name().into_string() {
            Ok(n) => n,
            Err(_) => continue,
        };
        let mtime = entry
            .metadata()
            .and_then(|m| m.modified())
            .unwrap_or(SystemTime::UNIX_EPOCH);
        let size = dir_size(&path);
        entries.push((name, path, mtime, size));
    }
    entries.sort_by(|a, b| a.2.cmp(&b.2));

    let is_kept = |name: &str| {
        keep_active.map(|k| k == name).unwrap_or(false)
            || keep_previous.map(|k| k == name).unwrap_or(false)
    };
    let mut total: u64 = entries.iter().map(|e| e.3).sum();
    let mut deletable_count = entries.iter().filter(|e| !is_kept(&e.0)).count();

    for (name, path, _, size) in &entries {
        if is_kept(name) {
            continue;
        }
        let must_delete_for_count = deletable_count > KEEP_COUNT;
        let must_delete_for_size = total > KEEP_BYTES;
        if !must_delete_for_count && !must_delete_for_size {
            break;
        }
        match std::fs::remove_dir_all(path) {
            Ok(()) => {
                report.deleted.push(name.clone());
                deletable_count = deletable_count.saturating_sub(1);
                total = total.saturating_sub(*size);
                info!(release = %name, bytes = size, "gc deleted release");
            }
            Err(e) => {
                report.errors = report.errors.saturating_add(1);
                warn!(release = %name, "gc delete failed: {e}");
            }
        }
    }
    report
}

fn dir_size(p: &std::path::Path) -> u64 {
    let mut total: u64 = 0;
    if let Ok(rd) = std::fs::read_dir(p) {
        for entry in rd.flatten() {
            let path = entry.path();
            let md = match std::fs::symlink_metadata(&path) {
                Ok(m) => m,
                Err(_) => continue,
            };
            if md.is_dir() {
                total = total.saturating_add(dir_size(&path));
            } else {
                total = total.saturating_add(md.len());
            }
        }
    }
    total
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    use tempfile::tempdir;

    fn synth_release(paths: &Paths, name: &str, body_bytes: usize, mtime_offset: i64) {
        let dir = paths.release_dir(name);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("config.json"), vec![0u8; body_bytes]).unwrap();
        let new_mtime = std::time::SystemTime::UNIX_EPOCH
            + Duration::from_secs((1_700_000_000_i64 + mtime_offset) as u64);
        let _ = filetime::set_file_mtime(&dir, filetime::FileTime::from_system_time(new_mtime));
    }

    #[test]
    fn keeps_active_and_previous_even_if_count_exceeded() {
        let tmp = tempdir().unwrap();
        let paths = Paths::with_root(tmp.path());
        std::fs::create_dir_all(paths.releases_dir()).unwrap();
        for i in 0..15 {
            synth_release(&paths, &format!("r{i:02}"), 100, i);
        }
        let report = run(&paths, Some("r00"), Some("r01"));
        assert!(paths.release_dir("r00").exists());
        assert!(paths.release_dir("r01").exists());
        assert!(report.deleted.contains(&"r02".to_string()));
    }

    #[test]
    fn caps_count_to_10_among_non_kept() {
        let tmp = tempdir().unwrap();
        let paths = Paths::with_root(tmp.path());
        std::fs::create_dir_all(paths.releases_dir()).unwrap();
        for i in 0..14 {
            synth_release(&paths, &format!("r{i:02}"), 100, i);
        }
        let report = run(&paths, Some("r13"), Some("r12"));
        assert_eq!(report.deleted.len(), 2);
        assert!(report.deleted.contains(&"r00".to_string()));
        assert!(report.deleted.contains(&"r01".to_string()));
    }

    #[test]
    fn skips_when_under_caps() {
        let tmp = tempdir().unwrap();
        let paths = Paths::with_root(tmp.path());
        std::fs::create_dir_all(paths.releases_dir()).unwrap();
        for i in 0..3 {
            synth_release(&paths, &format!("r{i}"), 100, i);
        }
        let report = run(&paths, Some("r2"), Some("r1"));
        assert!(report.deleted.is_empty());
    }

    #[test]
    fn no_releases_dir_is_a_noop() {
        let tmp = tempdir().unwrap();
        let paths = Paths::with_root(tmp.path());
        let report = run(&paths, None, None);
        assert!(report.deleted.is_empty());
        assert_eq!(report.errors, 0);
    }
}
