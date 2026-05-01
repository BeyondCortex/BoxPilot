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

/// Bundle manifest written as `diagnostics-manifest.json` inside the tar.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct BundleManifest {
    pub schema_version: u32,
    pub generated_at: String,
    pub boxpilot_version: String,
    pub host: super::sysinfo::SystemInfo,
    pub files: Vec<BundleManifestFile>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct BundleManifestFile {
    pub name: String,
    pub size: u64,
    pub redacted: bool,
}

/// Write the gzip-compressed tar to `out_path`. The tar's top-level
/// directory equals `bundle_dirname` so unpacking yields a single folder.
pub fn write_tarball(
    out_path: &Path,
    bundle_dirname: &str,
    manifest: &BundleManifest,
    entries: &[BundleEntry],
) -> std::io::Result<()> {
    use flate2::write::GzEncoder;
    use flate2::Compression;
    use std::fs::File;
    use tar::Header;

    let f = File::create(out_path)?;
    let gz = GzEncoder::new(f, Compression::default());
    let mut tar = tar::Builder::new(gz);

    let manifest_bytes = serde_json::to_vec_pretty(manifest).map_err(io_err)?;
    let mut all_entries: Vec<(&str, &[u8])> = Vec::with_capacity(entries.len() + 1);
    all_entries.push(("diagnostics-manifest.json", manifest_bytes.as_slice()));
    for e in entries {
        all_entries.push((e.name.as_str(), e.contents.as_slice()));
    }

    for (name, body) in all_entries {
        let mut header = Header::new_gnu();
        header.set_path(format!("{bundle_dirname}/{name}"))?;
        header.set_mode(0o600);
        header.set_uid(0);
        header.set_gid(0);
        header.set_size(body.len() as u64);
        header.set_cksum();
        tar.append(&header, body)?;
    }

    tar.into_inner()?.finish()?;
    Ok(())
}

fn io_err(e: serde_json::Error) -> std::io::Error {
    std::io::Error::other(e)
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

    use super::super::sysinfo::SystemInfo;
    use flate2::read::GzDecoder;
    use std::collections::HashMap;
    use std::io::Read;

    fn read_tar_entries(path: &Path) -> HashMap<String, Vec<u8>> {
        let f = std::fs::File::open(path).unwrap();
        let gz = GzDecoder::new(f);
        let mut tar = tar::Archive::new(gz);
        let mut out = HashMap::new();
        for entry in tar.entries().unwrap() {
            let mut entry = entry.unwrap();
            let p = entry.path().unwrap().to_string_lossy().into_owned();
            let mut buf = Vec::new();
            entry.read_to_end(&mut buf).unwrap();
            out.insert(p, buf);
        }
        out
    }

    #[test]
    fn tarball_contains_manifest_and_entries() {
        let tmp = tempdir().unwrap();
        let out = tmp.path().join("x.tar.gz");
        let entries = vec![
            BundleEntry {
                name: "a.json".into(),
                contents: b"{\"x\":1}".to_vec(),
                redacted: true,
            },
            BundleEntry {
                name: "b.txt".into(),
                contents: b"hi".to_vec(),
                redacted: false,
            },
        ];
        let manifest = BundleManifest {
            schema_version: 1,
            generated_at: "2026-04-30T22-00-00Z".into(),
            boxpilot_version: "0.1.0".into(),
            host: SystemInfo {
                kernel: "test".into(),
                os_id: "test".into(),
                os_version_id: "1".into(),
                os_pretty_name: "Test".into(),
                boxpilot_version: "0.1.0".into(),
            },
            files: vec![
                BundleManifestFile {
                    name: "a.json".into(),
                    size: 7,
                    redacted: true,
                },
                BundleManifestFile {
                    name: "b.txt".into(),
                    size: 2,
                    redacted: false,
                },
            ],
        };
        write_tarball(&out, "boxpilot-diagnostics-test", &manifest, &entries).unwrap();
        assert!(out.exists());
        let read_back = read_tar_entries(&out);
        assert!(read_back.contains_key("boxpilot-diagnostics-test/diagnostics-manifest.json"));
        assert_eq!(read_back["boxpilot-diagnostics-test/a.json"], b"{\"x\":1}");
        assert_eq!(read_back["boxpilot-diagnostics-test/b.txt"], b"hi");
    }
}
