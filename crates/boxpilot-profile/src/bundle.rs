use chrono::Utc;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

use boxpilot_ipc::{
    ActivationManifest, AssetEntry, SourceKind, ACTIVATION_MANIFEST_SCHEMA_VERSION,
    BUNDLE_MAX_FILE_BYTES, BUNDLE_MAX_FILE_COUNT, BUNDLE_MAX_NESTING_DEPTH,
    BUNDLE_MAX_TOTAL_BYTES,
};

use crate::asset_check::{verify_asset_refs, AssetCheckError};
use crate::list::ProfileStore;
use crate::redact::redact_url_strict;
use crate::remotes::read_remotes;
use crate::store::ensure_dir_0700;

#[derive(Debug)]
pub struct PreparedBundle {
    pub staging: tempfile::TempDir,
    pub manifest: ActivationManifest,
}

impl PreparedBundle {
    pub fn config_path(&self) -> PathBuf { self.staging.path().join("config.json") }
    pub fn assets_dir(&self) -> PathBuf { self.staging.path().join("assets") }
    pub fn manifest_path(&self) -> PathBuf { self.staging.path().join("manifest.json") }
}

#[derive(Debug, thiserror::Error)]
pub enum BundleError {
    #[error("profile {0} has no source.json on disk")]
    MissingSource(String),
    #[error("profile source is not valid JSON: {0}")]
    InvalidJson(serde_json::Error),
    #[error(transparent)]
    AssetCheck(#[from] AssetCheckError),
    #[error("file {path} too large ({size} bytes; per-file limit {limit})")]
    FileTooLarge { path: PathBuf, size: u64, limit: u64 },
    #[error("bundle exceeds total size {total} > {limit}")]
    TotalTooLarge { total: u64, limit: u64 },
    #[error("bundle exceeds file count {count} > {limit}")]
    TooManyFiles { count: u32, limit: u32 },
    #[error("bundle exceeds nesting depth {depth} > {limit}")]
    TooDeep { depth: u32, limit: u32 },
    #[error("remote profile {0} has no entry in remotes.json")]
    RemoteMissing(String),
    #[error("remote URL is not parseable; refusing to write a manifest")]
    UnparseableRemoteUrl,
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

/// Compose a §9.2 staging directory ready for plan #5 to transfer.
///
/// `core_path_at_activation` and `core_version_at_activation` are
/// passed in by the caller (plan #4 does not call into `boxpilotd`'s
/// `core.discover`; the GUI fetches that separately and forwards the
/// chosen core to this function).
pub fn prepare_bundle(
    store: &ProfileStore,
    profile_id: &str,
    core_path_at_activation: &str,
    core_version_at_activation: &str,
) -> Result<PreparedBundle, BundleError> {
    let meta = store.get(profile_id)
        .map_err(|_| BundleError::MissingSource(profile_id.to_string()))?;

    let source_path = store.paths().profile_source(profile_id);
    if !source_path.exists() {
        return Err(BundleError::MissingSource(profile_id.to_string()));
    }
    let config_bytes = std::fs::read(&source_path)?;
    let config_value: serde_json::Value =
        serde_json::from_slice(&config_bytes).map_err(BundleError::InvalidJson)?;

    let staging = tempfile::tempdir()?;
    let staging_path = staging.path().to_path_buf();
    let assets_dst = staging_path.join("assets");
    ensure_dir_0700(&assets_dst)?;

    // Copy assets out of the user's profile dir so verify_asset_refs runs
    // against the same view boxpilotd will see post-staging-rename.
    let assets_src = store.paths().profile_assets_dir(profile_id);
    let mut total: u64 = config_bytes.len() as u64;
    let mut file_count: u32 = 1;
    let mut max_depth: u32 = 0;
    let mut entries: Vec<AssetEntry> = Vec::new();
    if assets_src.exists() {
        copy_assets_into(&assets_src, &assets_dst, 0, &mut total, &mut file_count, &mut max_depth, &mut entries)?;
    }
    if max_depth > BUNDLE_MAX_NESTING_DEPTH {
        return Err(BundleError::TooDeep { depth: max_depth, limit: BUNDLE_MAX_NESTING_DEPTH });
    }

    // Write config.json
    let config_dst = staging_path.join("config.json");
    std::fs::write(&config_dst, &config_bytes)?;

    // §9.2 reference verification (after assets are in place).
    verify_asset_refs(&config_value, &assets_dst)?;

    // Sort manifest assets for determinism.
    entries.sort_by(|a, b| a.path.cmp(&b.path));

    let now = Utc::now();
    let activation_id = format!(
        "{}-{}",
        now.format("%Y-%m-%dT%H-%M-%SZ"),
        &hex::encode({
            let mut h = Sha256::new();
            h.update(now.timestamp_subsec_nanos().to_le_bytes());
            h.update(profile_id.as_bytes());
            h.update(std::process::id().to_le_bytes());
            h.finalize()
        })[..6]
    );

    let source_url_redacted = match meta.source_kind {
        SourceKind::Local | SourceKind::LocalDir => None,
        SourceKind::Remote => {
            let remote_id = meta.remote_id.clone()
                .ok_or_else(|| BundleError::RemoteMissing(profile_id.to_string()))?;
            let rfile = read_remotes(&store.paths().remotes_json()).unwrap_or_default();
            let entry = rfile.remotes.get(&remote_id)
                .ok_or_else(|| BundleError::RemoteMissing(profile_id.to_string()))?;
            Some(redact_url_strict(&entry.url).ok_or(BundleError::UnparseableRemoteUrl)?)
        }
    };

    let mut profile_hasher = Sha256::new();
    profile_hasher.update(&config_bytes);
    for e in &entries {
        profile_hasher.update(e.path.as_bytes());
        profile_hasher.update(e.sha256.as_bytes());
    }
    let profile_sha256 = hex::encode(profile_hasher.finalize());

    let mut config_hasher = Sha256::new();
    config_hasher.update(&config_bytes);
    let config_sha256 = hex::encode(config_hasher.finalize());

    let manifest = ActivationManifest {
        schema_version: ACTIVATION_MANIFEST_SCHEMA_VERSION,
        activation_id,
        profile_id: profile_id.to_string(),
        profile_sha256,
        config_sha256,
        source_kind: meta.source_kind,
        source_url_redacted,
        core_path_at_activation: core_path_at_activation.to_string(),
        core_version_at_activation: core_version_at_activation.to_string(),
        created_at: now.to_rfc3339(),
        assets: entries,
    };

    let manifest_bytes = serde_json::to_vec_pretty(&manifest)
        .map_err(BundleError::InvalidJson)?;
    std::fs::write(staging_path.join("manifest.json"), &manifest_bytes)?;

    let _ = (total, file_count); // already enforced inside copy_assets_into
    Ok(PreparedBundle { staging, manifest })
}

fn copy_assets_into(
    src: &Path,
    dst: &Path,
    depth: u32,
    total: &mut u64,
    file_count: &mut u32,
    max_depth: &mut u32,
    entries: &mut Vec<AssetEntry>,
) -> Result<(), BundleError> {
    *max_depth = (*max_depth).max(depth);
    if depth > BUNDLE_MAX_NESTING_DEPTH {
        return Err(BundleError::TooDeep { depth, limit: BUNDLE_MAX_NESTING_DEPTH });
    }
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let p = entry.path();
        let ft = std::fs::symlink_metadata(&p)?.file_type();
        if ft.is_symlink() {
            // Symlinks are refused by daemon-side; refuse here too for parity.
            return Err(BundleError::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("symlink in profile assets at {}", p.display()),
            )));
        }
        let rel = entry.file_name();
        let dst_child = dst.join(&rel);
        if ft.is_dir() {
            ensure_dir_0700(&dst_child)?;
            copy_assets_into(&p, &dst_child, depth + 1, total, file_count, max_depth, entries)?;
            continue;
        }
        if !ft.is_file() {
            return Err(BundleError::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("non-regular file in profile assets at {}", p.display()),
            )));
        }
        let bytes = std::fs::read(&p)?;
        let size = bytes.len() as u64;
        if size > BUNDLE_MAX_FILE_BYTES {
            return Err(BundleError::FileTooLarge { path: p.clone(), size, limit: BUNDLE_MAX_FILE_BYTES });
        }
        *total = (*total).saturating_add(size);
        if *total > BUNDLE_MAX_TOTAL_BYTES {
            return Err(BundleError::TotalTooLarge { total: *total, limit: BUNDLE_MAX_TOTAL_BYTES });
        }
        *file_count = (*file_count).saturating_add(1);
        if *file_count > BUNDLE_MAX_FILE_COUNT {
            return Err(BundleError::TooManyFiles { count: *file_count, limit: BUNDLE_MAX_FILE_COUNT });
        }
        std::fs::write(&dst_child, &bytes)?;
        let rel_str = dst_child.strip_prefix(dst.ancestors().last().unwrap_or(dst))
            .unwrap_or(&dst_child)
            .to_string_lossy()
            .to_string();
        let mut h = Sha256::new();
        h.update(&bytes);
        let sha = hex::encode(h.finalize());
        // The path inside the manifest must be relative to the bundle's
        // `assets/` directory. Compute that explicitly.
        // (Walk uses dst as the real assets root passed in by caller.)
        // We re-derive rel from `dst` here:
        let rel_from_assets_root = strip_to_assets_root(&dst_child, dst);
        entries.push(AssetEntry {
            path: rel_from_assets_root.unwrap_or(rel_str),
            sha256: sha,
            size,
        });
    }
    Ok(())
}

fn strip_to_assets_root(dst_child: &Path, _initial_dst: &Path) -> Option<String> {
    // Walk parents until we find a component named "assets" and return
    // everything below it, slash-joined.
    let mut found = false;
    let mut parts: Vec<String> = Vec::new();
    for c in dst_child.components() {
        let s = c.as_os_str().to_string_lossy().to_string();
        if found { parts.push(s); continue; }
        if s == "assets" { found = true; }
    }
    if found && !parts.is_empty() { Some(parts.join("/")) } else { None }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::import::{import_local_dir, import_local_file};
    use crate::store::ProfileStorePaths;
    use pretty_assertions::assert_eq;

    fn fixture() -> (tempfile::TempDir, ProfileStore) {
        let tmp = tempfile::tempdir().unwrap();
        let s = ProfileStore::new(ProfileStorePaths::new(tmp.path().to_path_buf()));
        (tmp, s)
    }

    #[test]
    fn prepare_bundle_local_no_assets_writes_layout() {
        let (tmp, s) = fixture();
        let src = tmp.path().join("in.json");
        std::fs::write(&src, br#"{"log":{"level":"info"}}"#).unwrap();
        let m = import_local_file(&s, &src, "P").unwrap();

        let b = prepare_bundle(&s, &m.id, "/var/lib/boxpilot/cores/current/sing-box", "1.10.0").unwrap();
        assert!(b.config_path().exists());
        assert!(b.assets_dir().exists());
        assert!(b.manifest_path().exists());
        assert!(b.manifest.activation_id.contains('Z'));
        assert!(matches!(b.manifest.source_kind, SourceKind::Local));
        assert!(b.manifest.source_url_redacted.is_none());
    }

    #[test]
    fn prepare_bundle_dir_carries_assets_and_manifest_entries() {
        let (tmp, s) = fixture();
        let src = tmp.path().join("bundle");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(src.join("config.json"),
            br#"{"route":{"rule_set":[{"path":"geosite.db"}]}}"#).unwrap();
        std::fs::write(src.join("geosite.db"), b"GEO").unwrap();
        let m = import_local_dir(&s, &src, "P").unwrap();

        let b = prepare_bundle(&s, &m.id, "/path/sing-box", "1.10.0").unwrap();
        assert_eq!(b.manifest.assets.len(), 1);
        assert_eq!(b.manifest.assets[0].path, "geosite.db");
        assert_eq!(b.manifest.assets[0].size, 3);
        assert!(b.assets_dir().join("geosite.db").exists());
    }

    #[test]
    fn prepare_bundle_refuses_when_asset_missing() {
        let (tmp, s) = fixture();
        let src = tmp.path().join("in.json");
        std::fs::write(&src, br#"{"route":{"rule_set":[{"path":"missing.db"}]}}"#).unwrap();
        let m = import_local_file(&s, &src, "P").unwrap();
        let err = prepare_bundle(&s, &m.id, "/p/sb", "1.10.0").unwrap_err();
        assert!(matches!(err, BundleError::AssetCheck(AssetCheckError::MissingFromBundle { .. })));
    }

    #[test]
    fn prepare_bundle_refuses_absolute_path_in_config() {
        let (tmp, s) = fixture();
        let src = tmp.path().join("in.json");
        std::fs::write(&src,
            br#"{"route":{"rule_set":[{"path":"/etc/passwd"}]}}"#).unwrap();
        let m = import_local_file(&s, &src, "P").unwrap();
        let err = prepare_bundle(&s, &m.id, "/p/sb", "1.10.0").unwrap_err();
        assert!(matches!(err, BundleError::AssetCheck(AssetCheckError::AbsolutePathRefused(_))));
    }
}
