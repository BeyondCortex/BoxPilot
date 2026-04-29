use serde_json::Value;
use std::collections::BTreeSet;

const ASSET_PATH_KEYS: &[&str] = &["path", "geosite_path", "geoip_path"];

/// Recursively walk `config` and return the list of relative asset
/// paths it references. Absolute paths are *not* returned here —
/// callers should run [`detect_absolute_paths`] separately so the two
/// concerns can be reported with distinct error codes.
pub fn extract_asset_refs(config: &Value) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    walk_refs(config, &mut out);
    out
}

fn walk_refs(v: &Value, out: &mut BTreeSet<String>) {
    match v {
        Value::Object(map) => {
            for (k, child) in map {
                if ASSET_PATH_KEYS.iter().any(|target| *target == k.as_str()) {
                    if let Value::String(s) = child {
                        if !s.is_empty() && !is_absolute_or_url(s) {
                            out.insert(s.clone());
                        }
                    }
                } else {
                    walk_refs(child, out);
                }
            }
        }
        Value::Array(arr) => {
            for child in arr { walk_refs(child, out); }
        }
        _ => {}
    }
}

fn is_absolute_or_url(s: &str) -> bool {
    s.starts_with('/') || s.contains("://")
}

/// Returns the list of absolute filesystem paths the config references
/// (anything starting with `/`). URL-like values (`http://`, `https://`)
/// are NOT considered "absolute paths" for this check — they are remote
/// fetch targets handled by sing-box itself.
pub fn detect_absolute_paths(config: &Value) -> Vec<String> {
    let mut out = Vec::new();
    walk_abs(config, &mut out);
    out
}

fn walk_abs(v: &Value, out: &mut Vec<String>) {
    match v {
        Value::Object(map) => {
            for (k, child) in map {
                if ASSET_PATH_KEYS.iter().any(|t| *t == k.as_str()) {
                    if let Value::String(s) = child {
                        if s.starts_with('/') {
                            out.push(s.clone());
                        }
                    }
                } else {
                    walk_abs(child, out);
                }
            }
        }
        Value::Array(arr) => for c in arr { walk_abs(c, out); }
        _ => {}
    }
}

#[derive(Debug, thiserror::Error)]
pub enum AssetCheckError {
    #[error("config references {missing} asset(s) not present in assets/: {paths}", paths = .paths.join(", "))]
    MissingFromBundle { missing: usize, paths: Vec<String> },
    #[error("config references absolute path(s) refused per §9.3: {}", .0.join(", "))]
    AbsolutePathRefused(Vec<String>),
}

/// Walks `assets_dir` to collect every regular file's relative path,
/// then verifies every reference returned by `extract_asset_refs` is
/// present.
pub fn verify_asset_refs(
    config: &Value,
    assets_dir: &std::path::Path,
) -> Result<(), AssetCheckError> {
    let abs = detect_absolute_paths(config);
    if !abs.is_empty() {
        return Err(AssetCheckError::AbsolutePathRefused(abs));
    }
    let needed = extract_asset_refs(config);
    let present = walk_present_assets(assets_dir);
    let missing: Vec<String> = needed.difference(&present).cloned().collect();
    if !missing.is_empty() {
        return Err(AssetCheckError::MissingFromBundle {
            missing: missing.len(), paths: missing,
        });
    }
    Ok(())
}

fn walk_present_assets(root: &std::path::Path) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let read = match std::fs::read_dir(&dir) { Ok(r) => r, Err(_) => continue };
        for entry in read.flatten() {
            let p = entry.path();
            if let Ok(ft) = std::fs::symlink_metadata(&p).map(|m| m.file_type()) {
                if ft.is_dir() { stack.push(p); continue; }
                if ft.is_file() {
                    if let Ok(rel) = p.strip_prefix(root) {
                        if let Some(s) = rel.to_str() {
                            out.insert(s.replace('\\', "/"));
                        }
                    }
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn extract_finds_known_keys_in_nested_arrays() {
        let v = json!({
            "route": {
                "rule_set": [
                    {"tag": "geosite", "type": "local", "format": "binary", "path": "geosite.db"},
                    {"tag": "rules",   "type": "local", "format": "source", "path": "rules/r1.json"},
                ]
            },
            "outbounds": [{"path": "ignored-because-array-walk-still-finds-it"}],
        });
        let refs = extract_asset_refs(&v);
        assert!(refs.contains("geosite.db"));
        assert!(refs.contains("rules/r1.json"));
        assert!(refs.contains("ignored-because-array-walk-still-finds-it"));
    }

    #[test]
    fn extract_skips_absolute_and_urls() {
        let v = json!({"x": {"path": "/etc/passwd"}, "y": {"path": "https://h/x"}});
        assert!(extract_asset_refs(&v).is_empty());
    }

    #[test]
    fn detect_absolute_paths_returns_offenders() {
        let v = json!({
            "x": {"path": "/etc/passwd"},
            "y": {"path": "ok.db"},
            "z": {"path": "/home/user/secret"},
        });
        let abs = detect_absolute_paths(&v);
        assert!(abs.contains(&"/etc/passwd".into()));
        assert!(abs.contains(&"/home/user/secret".into()));
        assert!(!abs.iter().any(|p| p == "ok.db"));
    }

    #[test]
    fn verify_passes_when_all_refs_present() {
        let v = json!({"route": {"rule_set": [{"path": "geosite.db"}]}});
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("geosite.db"), b"x").unwrap();
        verify_asset_refs(&v, tmp.path()).unwrap();
    }

    #[test]
    fn verify_fails_when_ref_missing() {
        let v = json!({"route": {"rule_set": [{"path": "geosite.db"}]}});
        let tmp = tempfile::tempdir().unwrap();
        let err = verify_asset_refs(&v, tmp.path()).unwrap_err();
        assert!(matches!(err, AssetCheckError::MissingFromBundle { .. }));
    }

    #[test]
    fn verify_fails_when_absolute_path() {
        let v = json!({"route": {"rule_set": [{"path": "/etc/passwd"}]}});
        let tmp = tempfile::tempdir().unwrap();
        let err = verify_asset_refs(&v, tmp.path()).unwrap_err();
        assert!(matches!(err, AssetCheckError::AbsolutePathRefused(_)));
    }
}
