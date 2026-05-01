//! Bundle composition: file collection, redaction, tarball writer.

use std::path::{Path, PathBuf};

/// One file slot in the bundle. The composer iterates a fixed list so the
/// bundle layout is stable across runs even when sources are missing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BundleEntry {
    /// Name inside the tar (no leading directory; the composer prefixes
    /// the per-bundle directory).
    pub name: String,
    /// File contents as written into the tar.
    pub contents: Vec<u8>,
    /// Whether this file was redacted before inclusion.
    pub redacted: bool,
}

/// Source file → bundle entry. Returns the placeholder entry on missing /
/// unreadable source rather than failing the whole bundle.
pub fn collect_verbatim(name: &str, source: &Path) -> BundleEntry {
    match std::fs::read(source) {
        Ok(bytes) => BundleEntry {
            name: name.to_string(),
            contents: bytes,
            redacted: false,
        },
        Err(e) => unavailable(name, &format!("read {}: {e}", source.display())),
    }
}

/// Source JSON file → redacted bundle entry. The JSON is parsed, walked
/// through [`boxpilot_ipc::redact::redact_singbox_config`], and
/// re-serialized pretty-printed. Parse failure produces an unavailable entry.
pub fn collect_redacted_singbox_config(name: &str, source: &Path) -> BundleEntry {
    let bytes = match std::fs::read(source) {
        Ok(b) => b,
        Err(e) => return unavailable(name, &format!("read {}: {e}", source.display())),
    };
    let mut value: serde_json::Value = match serde_json::from_slice(&bytes) {
        Ok(v) => v,
        Err(e) => return unavailable(name, &format!("parse json: {e}")),
    };
    boxpilot_ipc::redact::redact_singbox_config(&mut value);
    match serde_json::to_vec_pretty(&value) {
        Ok(out) => BundleEntry {
            name: name.to_string(),
            contents: out,
            redacted: true,
        },
        Err(e) => unavailable(name, &format!("encode redacted: {e}")),
    }
}

/// Synthetic placeholder so the bundle layout stays consistent when a
/// source is absent. The replacement file's *name* gets the
/// `.unavailable.txt` suffix so a support engineer can see the original
/// slot was attempted but failed.
pub fn unavailable(name: &str, cause: &str) -> BundleEntry {
    BundleEntry {
        name: format!("{name}.unavailable.txt"),
        contents: format!("source unavailable: {cause}\n").into_bytes(),
        redacted: false,
    }
}

/// Build a path under `cache_diagnostics_dir` for the given bundle name.
pub fn bundle_path(cache_dir: &Path, generated_at: &str) -> PathBuf {
    cache_dir.join(format!("boxpilot-diagnostics-{generated_at}.tar.gz"))
}

/// Drop journal/stderr lines that contain markers correlated with secrets.
/// Text-stage redaction is fundamentally heuristic — we cannot parse a
/// freeform journal line into JSON. Schema-aware walking is reserved for
/// `*.json` artifacts inside the bundle.
///
/// Shared call site between [`super::compose`] and the activation
/// pipeline's `sing-box check` stderr scrub.
pub fn redact_journal_lines(s: &str) -> String {
    s.lines()
        .filter(|line| {
            let lower = line.to_ascii_lowercase();
            !(lower.contains("password")
                || lower.contains("uuid")
                || lower.contains("private_key")
                || lower.contains("token=")
                || lower.contains("secret"))
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn drops_password_lines() {
        let s = "ok 1\npassword=hunter2\nok 2";
        assert_eq!(redact_journal_lines(s), "ok 1\nok 2");
    }

    #[test]
    fn drops_uuid_and_private_key_and_token_and_secret() {
        let s = "a\nuuid=x\nb\nprivate_key=y\nc\ntoken=z\nd\nsecret=q\ne";
        assert_eq!(redact_journal_lines(s), "a\nb\nc\nd\ne");
    }

    #[test]
    fn passes_through_non_secret_lines() {
        let s = "starting up\nlistening on 127.0.0.1:9090";
        assert_eq!(
            redact_journal_lines(s),
            "starting up\nlistening on 127.0.0.1:9090"
        );
    }

    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn collect_verbatim_reads_bytes() {
        let tmp = tempdir().unwrap();
        let p = tmp.path().join("x");
        fs::write(&p, b"hello").unwrap();
        let e = collect_verbatim("x", &p);
        assert_eq!(e.name, "x");
        assert_eq!(e.contents, b"hello");
        assert!(!e.redacted);
    }

    #[test]
    fn collect_verbatim_missing_source_returns_unavailable() {
        let tmp = tempdir().unwrap();
        let e = collect_verbatim("x", &tmp.path().join("nope"));
        assert_eq!(e.name, "x.unavailable.txt");
        assert!(String::from_utf8_lossy(&e.contents).contains("source unavailable"));
        assert!(!e.redacted);
    }

    #[test]
    fn collect_redacted_replaces_password() {
        let tmp = tempdir().unwrap();
        let p = tmp.path().join("config.json");
        fs::write(
            &p,
            serde_json::to_vec(&serde_json::json!({
                "outbounds": [{"type":"vless","tag":"main","password":"hunter2"}]
            }))
            .unwrap(),
        )
        .unwrap();
        let e = collect_redacted_singbox_config("active-config.json", &p);
        assert_eq!(e.name, "active-config.json");
        assert!(e.redacted);
        let s = String::from_utf8_lossy(&e.contents);
        assert!(!s.contains("hunter2"), "password leaked: {s}");
        assert!(s.contains("***"));
    }

    #[test]
    fn collect_redacted_garbage_json_falls_back_to_unavailable() {
        let tmp = tempdir().unwrap();
        let p = tmp.path().join("config.json");
        fs::write(&p, b"not json").unwrap();
        let e = collect_redacted_singbox_config("active-config.json", &p);
        assert_eq!(e.name, "active-config.json.unavailable.txt");
        assert!(String::from_utf8_lossy(&e.contents).contains("parse json"));
    }

    #[test]
    fn bundle_path_uses_naming_convention() {
        let p = bundle_path(
            Path::new("/var/cache/boxpilot/diagnostics"),
            "2026-04-30T22-00-00Z",
        );
        assert_eq!(
            p.to_string_lossy(),
            "/var/cache/boxpilot/diagnostics/boxpilot-diagnostics-2026-04-30T22-00-00Z.tar.gz"
        );
    }
}
