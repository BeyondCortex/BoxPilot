use chrono::Utc;
use sha2::{Digest, Sha256};
use std::path::Path;

use boxpilot_ipc::SourceKind;

use crate::list::ProfileStore;
use crate::meta::{write_metadata, ProfileMetadata, METADATA_SCHEMA_VERSION};
use crate::store::{ensure_dir_0700, write_file_0600_atomic};

#[derive(Debug, thiserror::Error)]
pub enum ImportError {
    #[error("source is not valid JSON: {0}")]
    InvalidJson(serde_json::Error),
    #[error("source file is too large ({size} bytes; limit {limit})")]
    TooLarge { size: u64, limit: u64 },
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

/// Cap an in-memory single-JSON import at the per-file limit so a huge
/// pasted blob can't OOM the GUI; matches §9.2's per-file cap.
pub const SINGLE_JSON_MAX_BYTES: u64 = boxpilot_ipc::BUNDLE_MAX_FILE_BYTES;

pub fn slugify(name: &str) -> String {
    let mut s: String = name
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '-' })
        .collect();
    while s.contains("--") { s = s.replace("--", "-"); }
    let trimmed = s.trim_matches('-');
    if trimmed.is_empty() { "profile".to_string() } else { trimmed.to_string() }
}

/// Stable-but-unique-on-this-machine. `name` only contributes the slug;
/// the timestamp + 8-hex random suffix guarantee no collisions across
/// repeated imports of profiles with the same name.
pub fn new_profile_id(name: &str, now: chrono::DateTime<Utc>) -> String {
    let ts = now.format("%Y%m%dT%H%M%SZ").to_string();
    let nanos = now.timestamp_subsec_nanos();
    let pid = std::process::id();
    let mut h = Sha256::new();
    h.update(ts.as_bytes());
    h.update(nanos.to_le_bytes());
    h.update(pid.to_le_bytes());
    h.update(name.as_bytes());
    let suffix = &hex::encode(h.finalize())[..8];
    format!("{}-{}-{}", slugify(name), ts, suffix)
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(bytes);
    hex::encode(h.finalize())
}

pub fn import_local_file(
    store: &ProfileStore,
    src_path: &Path,
    name: &str,
) -> Result<ProfileMetadata, ImportError> {
    let meta = std::fs::metadata(src_path)?;
    if meta.len() > SINGLE_JSON_MAX_BYTES {
        return Err(ImportError::TooLarge { size: meta.len(), limit: SINGLE_JSON_MAX_BYTES });
    }
    let bytes = std::fs::read(src_path)?;
    serde_json::from_slice::<serde_json::Value>(&bytes).map_err(ImportError::InvalidJson)?;

    let now = Utc::now();
    let id = new_profile_id(name, now);
    let dir = store.paths().profile_dir(&id);
    ensure_dir_0700(store.paths().root())?;
    ensure_dir_0700(&store.paths().profiles_dir())?;
    ensure_dir_0700(&dir)?;
    ensure_dir_0700(&store.paths().profile_assets_dir(&id))?;

    write_file_0600_atomic(&store.paths().profile_source(&id), &bytes)?;

    let now_str = now.to_rfc3339();
    let m = ProfileMetadata {
        schema_version: METADATA_SCHEMA_VERSION,
        id: id.clone(),
        name: name.to_string(),
        source_kind: SourceKind::Local,
        remote_id: None,
        created_at: now_str.clone(),
        updated_at: now_str,
        last_valid_activation_id: None,
        config_sha256: sha256_hex(&bytes),
    };
    write_metadata(&store.paths().profile_metadata(&id), &m)?;
    Ok(m)
}

use std::collections::VecDeque;

#[derive(Debug, thiserror::Error)]
pub enum DirImportError {
    #[error("directory has no config.json or source.json")]
    MissingConfig,
    #[error("source is not valid JSON: {0}")]
    InvalidJson(serde_json::Error),
    #[error("symlink not allowed at {0}")]
    SymlinkRejected(std::path::PathBuf),
    #[error("non-regular file rejected at {0}")]
    NotRegular(std::path::PathBuf),
    #[error("file {path} too large ({size} bytes; per-file limit {limit})")]
    FileTooLarge { path: std::path::PathBuf, size: u64, limit: u64 },
    #[error("bundle exceeds total size {total} > {limit}")]
    TotalTooLarge { total: u64, limit: u64 },
    #[error("bundle exceeds file count {count} > {limit}")]
    TooManyFiles { count: u32, limit: u32 },
    #[error("bundle exceeds nesting depth {depth} > {limit}")]
    TooDeep { depth: u32, limit: u32 },
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

pub fn import_local_dir(
    store: &ProfileStore,
    src_dir: &std::path::Path,
    name: &str,
) -> Result<ProfileMetadata, DirImportError> {
    use boxpilot_ipc::{
        BUNDLE_MAX_FILE_BYTES, BUNDLE_MAX_FILE_COUNT, BUNDLE_MAX_NESTING_DEPTH,
        BUNDLE_MAX_TOTAL_BYTES,
    };

    // Pick the source config file. Prefer source.json (sing-box-native),
    // fall back to config.json.
    let src_meta = std::fs::symlink_metadata(src_dir)?;
    if src_meta.file_type().is_symlink() {
        return Err(DirImportError::SymlinkRejected(src_dir.to_path_buf()));
    }
    let mut config_path = src_dir.join("source.json");
    if !config_path.exists() {
        config_path = src_dir.join("config.json");
    }
    if !config_path.exists() {
        return Err(DirImportError::MissingConfig);
    }
    let config_bytes = std::fs::read(&config_path)?;
    serde_json::from_slice::<serde_json::Value>(&config_bytes)
        .map_err(DirImportError::InvalidJson)?;

    // Walk to enumerate assets (every regular file in src_dir except the chosen config).
    struct WalkEntry { rel: std::path::PathBuf, abs: std::path::PathBuf }
    let mut entries: Vec<WalkEntry> = Vec::new();
    let mut total_bytes: u64 = config_bytes.len() as u64;
    let mut file_count: u32 = 1;
    let mut max_depth: u32 = 0;
    let mut queue: VecDeque<(std::path::PathBuf, u32)> = VecDeque::new();
    queue.push_back((src_dir.to_path_buf(), 0));

    while let Some((dir, depth)) = queue.pop_front() {
        max_depth = max_depth.max(depth);
        for entry in std::fs::read_dir(&dir)? {
            let entry = entry?;
            let abs = entry.path();
            let ft = std::fs::symlink_metadata(&abs)?.file_type();
            if ft.is_symlink() {
                return Err(DirImportError::SymlinkRejected(abs));
            }
            if ft.is_dir() {
                let child_depth = depth + 1;
                if child_depth > BUNDLE_MAX_NESTING_DEPTH {
                    return Err(DirImportError::TooDeep {
                        depth: child_depth,
                        limit: BUNDLE_MAX_NESTING_DEPTH,
                    });
                }
                queue.push_back((abs, child_depth));
                continue;
            }
            if !ft.is_file() {
                return Err(DirImportError::NotRegular(abs));
            }
            // Skip the chosen config file (we already loaded it).
            if abs == config_path { continue; }
            // Skip a stray sibling of the chosen config to avoid double-importing.
            if abs == src_dir.join("source.json") || abs == src_dir.join("config.json") {
                continue;
            }
            let size = entry.metadata()?.len();
            if size > BUNDLE_MAX_FILE_BYTES {
                return Err(DirImportError::FileTooLarge {
                    path: abs, size, limit: BUNDLE_MAX_FILE_BYTES,
                });
            }
            total_bytes = total_bytes.saturating_add(size);
            file_count = file_count.saturating_add(1);
            if total_bytes > BUNDLE_MAX_TOTAL_BYTES {
                return Err(DirImportError::TotalTooLarge {
                    total: total_bytes, limit: BUNDLE_MAX_TOTAL_BYTES,
                });
            }
            if file_count > BUNDLE_MAX_FILE_COUNT {
                return Err(DirImportError::TooManyFiles {
                    count: file_count, limit: BUNDLE_MAX_FILE_COUNT,
                });
            }
            let rel = abs.strip_prefix(src_dir).unwrap().to_path_buf();
            entries.push(WalkEntry { rel, abs });
        }
    }

    // Compose the new profile dir.
    let now = chrono::Utc::now();
    let id = new_profile_id(name, now);
    let dir = store.paths().profile_dir(&id);
    ensure_dir_0700(store.paths().root())?;
    ensure_dir_0700(&store.paths().profiles_dir())?;
    ensure_dir_0700(&dir)?;
    let assets_root = store.paths().profile_assets_dir(&id);
    ensure_dir_0700(&assets_root)?;

    write_file_0600_atomic(&store.paths().profile_source(&id), &config_bytes)?;

    for e in &entries {
        let dst = assets_root.join(&e.rel);
        if let Some(p) = dst.parent() { ensure_dir_0700(p)?; }
        let bytes = std::fs::read(&e.abs)?;
        write_file_0600_atomic(&dst, &bytes)?;
    }

    let now_str = now.to_rfc3339();
    let m = ProfileMetadata {
        schema_version: METADATA_SCHEMA_VERSION,
        id: id.clone(),
        name: name.to_string(),
        source_kind: SourceKind::LocalDir,
        remote_id: None,
        created_at: now_str.clone(),
        updated_at: now_str,
        last_valid_activation_id: None,
        config_sha256: sha256_hex(&config_bytes),
    };
    write_metadata(&store.paths().profile_metadata(&id), &m)?;
    Ok(m)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::ProfileStorePaths;
    use pretty_assertions::assert_eq;
    use std::os::unix::fs::PermissionsExt;

    fn fixture() -> (tempfile::TempDir, ProfileStore) {
        let tmp = tempfile::tempdir().unwrap();
        let s = ProfileStore::new(ProfileStorePaths::new(tmp.path().to_path_buf()));
        (tmp, s)
    }

    #[test]
    fn slugify_handles_punctuation_and_unicode() {
        assert_eq!(slugify("My Profile!"), "my-profile");
        assert_eq!(slugify("一二三 abc"), "abc");
        assert_eq!(slugify("---"), "profile");
    }

    #[test]
    fn id_is_collision_resistant_for_same_name_different_times() {
        let t1 = chrono::Utc::now();
        let t2 = t1 + chrono::Duration::seconds(1);
        assert_ne!(new_profile_id("same", t1), new_profile_id("same", t2));
    }

    #[test]
    fn import_local_file_writes_layout_and_perms() {
        let (tmp, s) = fixture();
        let src = tmp.path().join("input.json");
        std::fs::write(&src, r#"{"hello":"world"}"#).unwrap();

        let m = import_local_file(&s, &src, "Hello").unwrap();
        assert!(matches!(m.source_kind, SourceKind::Local));
        assert!(m.id.starts_with("hello-"));

        // source.json mode 0600
        let src_mode = std::fs::metadata(s.paths().profile_source(&m.id)).unwrap()
            .permissions().mode() & 0o7777;
        assert_eq!(src_mode, 0o600);

        // assets/ mode 0700
        let assets_mode = std::fs::metadata(s.paths().profile_assets_dir(&m.id)).unwrap()
            .permissions().mode() & 0o7777;
        assert_eq!(assets_mode, 0o700);

        // metadata.json mode 0600
        let mm = std::fs::metadata(s.paths().profile_metadata(&m.id)).unwrap()
            .permissions().mode() & 0o7777;
        assert_eq!(mm, 0o600);
    }

    #[test]
    fn import_local_file_rejects_invalid_json() {
        let (tmp, s) = fixture();
        let src = tmp.path().join("bad.json");
        std::fs::write(&src, b"{not json").unwrap();
        assert!(matches!(import_local_file(&s, &src, "n"), Err(ImportError::InvalidJson(_))));
    }

    #[test]
    fn import_local_dir_walks_and_copies() {
        let (tmp, s) = fixture();
        let src = tmp.path().join("bundle");
        std::fs::create_dir_all(src.join("rules")).unwrap();
        std::fs::write(src.join("config.json"), r#"{"v":1}"#).unwrap();
        std::fs::write(src.join("geosite.db"), b"GEO").unwrap();
        std::fs::write(src.join("rules/r1.srs"), b"SRS").unwrap();

        let m = import_local_dir(&s, &src, "B").unwrap();
        assert!(matches!(m.source_kind, SourceKind::LocalDir));
        let assets = s.paths().profile_assets_dir(&m.id);
        assert_eq!(std::fs::read(assets.join("geosite.db")).unwrap(), b"GEO");
        assert_eq!(std::fs::read(assets.join("rules/r1.srs")).unwrap(), b"SRS");
    }

    #[test]
    fn import_local_dir_rejects_symlink_inside() {
        let (tmp, s) = fixture();
        let src = tmp.path().join("bundle");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(src.join("config.json"), r#"{}"#).unwrap();
        std::os::unix::fs::symlink("/etc/passwd", src.join("evil")).unwrap();
        assert!(matches!(import_local_dir(&s, &src, "B"), Err(DirImportError::SymlinkRejected(_))));
    }

    #[test]
    fn import_local_dir_rejects_missing_config() {
        let (tmp, s) = fixture();
        let src = tmp.path().join("bundle");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(src.join("geosite.db"), b"x").unwrap();
        assert!(matches!(import_local_dir(&s, &src, "B"), Err(DirImportError::MissingConfig)));
    }

    #[test]
    fn import_local_dir_prefers_source_json_over_config_json() {
        let (tmp, s) = fixture();
        let src = tmp.path().join("bundle");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(src.join("source.json"), r#"{"chosen":true}"#).unwrap();
        std::fs::write(src.join("config.json"), r#"{"chosen":false}"#).unwrap();
        let m = import_local_dir(&s, &src, "B").unwrap();
        let saved = std::fs::read(s.paths().profile_source(&m.id)).unwrap();
        assert!(String::from_utf8_lossy(&saved).contains("\"chosen\":true"));
    }

    #[test]
    fn import_local_dir_rejects_excessive_nesting() {
        use boxpilot_ipc::BUNDLE_MAX_NESTING_DEPTH;
        let (tmp, s) = fixture();
        let src = tmp.path().join("bundle");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(src.join("config.json"), r#"{}"#).unwrap();
        // Build a chain of subdirectories one level deeper than the limit.
        let mut p = src.clone();
        for i in 0..(BUNDLE_MAX_NESTING_DEPTH as usize + 2) {
            p = p.join(format!("d{i}"));
            std::fs::create_dir_all(&p).unwrap();
        }
        std::fs::write(p.join("leaf"), b"x").unwrap();
        let err = import_local_dir(&s, &src, "deep").unwrap_err();
        assert!(matches!(err, DirImportError::TooDeep { .. }));
    }
}
