//! Diagnostics export pipeline (spec §5.5 / §14, plan #8). The public entry
//! point is [`compose`], called from `iface::diagnostics_export_redacted`.

pub mod bundle;
pub mod gc;
pub mod sysinfo;

use boxpilot_platform::Paths;
use boxpilot_ipc::{
    DiagnosticsExportResponse, HelperError, HelperResult, DIAGNOSTICS_BUNDLE_CAP_BYTES,
    DIAGNOSTICS_JOURNAL_TAIL_LINES, DIAGNOSTICS_SCHEMA_VERSION,
};
use bundle::{
    bundle_path, collect_redacted_singbox_config, collect_verbatim, redact_journal_lines,
    write_tarball, BundleEntry, BundleManifest, BundleManifestFile,
};
use std::path::Path;

/// Inputs the daemon supplies to the composer. Implemented as a struct so
/// tests can inject a fake journal without spinning a real systemd.
pub struct ComposeInputs<'a> {
    pub paths: &'a Paths,
    pub unit_name: &'a str,
    pub journal: &'a dyn crate::systemd::JournalReader,
    pub os_release_path: &'a Path,
    pub now_iso: fn() -> String,
}

pub async fn compose(inputs: ComposeInputs<'_>) -> HelperResult<DiagnosticsExportResponse> {
    let dir = inputs.paths.cache_diagnostics_dir();
    create_dir_secure(&dir)?;
    gc::evict_to_cap(&dir, DIAGNOSTICS_BUNDLE_CAP_BYTES).map_err(|e| {
        HelperError::DiagnosticsIoFailed {
            step: "gc evict".into(),
            cause: e.to_string(),
        }
    })?;

    let generated_at = (inputs.now_iso)();
    let bundle_dirname = format!("boxpilot-diagnostics-{generated_at}");
    let out_path = bundle_path(&dir, &generated_at);

    // 1. Active config — schema-aware redact.
    let active_config_src = inputs.paths.active_symlink().join("config.json");
    let active_config_entry =
        collect_redacted_singbox_config("active-config.json", &active_config_src);

    // 2-5. Verbatim files.
    let toml_entry = collect_verbatim("boxpilot.toml", &inputs.paths.boxpilot_toml());
    let install_state_entry =
        collect_verbatim("install-state.json", &inputs.paths.install_state_json());
    let unit_entry = collect_verbatim(
        "service-unit.txt",
        &inputs.paths.systemd_unit_path(inputs.unit_name),
    );
    let manifest_entry = collect_verbatim(
        "manifest.json",
        &inputs.paths.active_symlink().join("manifest.json"),
    );

    // 6. Live service status snapshot — placeholder for now; the full
    //    snapshot is captured at the iface call site (Task 17).
    let service_status_entry = BundleEntry {
        name: "service-status.json".into(),
        contents: b"{}".to_vec(),
        redacted: false,
    };

    // 7. Journal tail — line-drop redact.
    let journal_lines = inputs
        .journal
        .tail(inputs.unit_name, DIAGNOSTICS_JOURNAL_TAIL_LINES)
        .await
        .unwrap_or_default();
    let journal_text = journal_lines.join("\n");
    let journal_entry = BundleEntry {
        name: "journal-tail.txt".into(),
        contents: redact_journal_lines(&journal_text).into_bytes(),
        redacted: true,
    };

    // 8. system-info.json
    let info = sysinfo::collect(inputs.os_release_path);
    let sysinfo_entry = BundleEntry {
        name: "system-info.json".into(),
        contents: serde_json::to_vec_pretty(&info).map_err(|e| {
            HelperError::DiagnosticsEncodeFailed {
                cause: e.to_string(),
            }
        })?,
        redacted: false,
    };

    let entries = vec![
        active_config_entry,
        toml_entry,
        install_state_entry,
        unit_entry,
        manifest_entry,
        service_status_entry,
        journal_entry,
        sysinfo_entry,
    ];

    let manifest = BundleManifest {
        schema_version: DIAGNOSTICS_SCHEMA_VERSION,
        generated_at: generated_at.clone(),
        boxpilot_version: env!("CARGO_PKG_VERSION").to_string(),
        host: info,
        files: entries
            .iter()
            .map(|e| BundleManifestFile {
                name: e.name.clone(),
                size: e.contents.len() as u64,
                redacted: e.redacted,
            })
            .collect(),
    };

    // Stream into a NamedTempFile co-located with the target so the final
    // rename stays on the same filesystem.
    let tmp = tempfile::Builder::new()
        .prefix(".boxpilot-diag-")
        .suffix(".tmp")
        .tempfile_in(&dir)
        .map_err(|e| HelperError::DiagnosticsIoFailed {
            step: "tempfile".into(),
            cause: e.to_string(),
        })?;
    let tmp_path = tmp.path().to_path_buf();
    drop(tmp); // close the fd; write_tarball reopens via File::create
    write_tarball(&tmp_path, &bundle_dirname, &manifest, &entries).map_err(|e| {
        HelperError::DiagnosticsIoFailed {
            step: "write tarball".into(),
            cause: e.to_string(),
        }
    })?;
    std::fs::rename(&tmp_path, &out_path).map_err(|e| HelperError::DiagnosticsIoFailed {
        step: "rename to final".into(),
        cause: e.to_string(),
    })?;

    let bundle_size_bytes = std::fs::metadata(&out_path)
        .map_err(|e| HelperError::DiagnosticsIoFailed {
            step: "stat final".into(),
            cause: e.to_string(),
        })?
        .len();

    Ok(DiagnosticsExportResponse {
        schema_version: DIAGNOSTICS_SCHEMA_VERSION,
        bundle_path: out_path.to_string_lossy().into_owned(),
        bundle_size_bytes,
        generated_at,
    })
}

fn create_dir_secure(dir: &Path) -> HelperResult<()> {
    if !dir.exists() {
        std::fs::create_dir_all(dir).map_err(|e| HelperError::DiagnosticsIoFailed {
            step: "mkdir".into(),
            cause: e.to_string(),
        })?;
    }
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::fs::PermissionsExt;
        let perms = std::fs::Permissions::from_mode(0o750);
        std::fs::set_permissions(dir, perms).map_err(|e| HelperError::DiagnosticsIoFailed {
            step: "chmod".into(),
            cause: e.to_string(),
        })?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::systemd::testing::FixedJournal;
    use std::fs;
    use tempfile::tempdir;

    fn iso() -> String {
        "2026-04-30T22-00-00Z".to_string()
    }

    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn happy_path_writes_tarball_with_redacted_active_config() {
        let tmp = tempdir().unwrap();
        let paths = Paths::with_root(tmp.path());

        // Set up minimal fake filesystem:
        let active = paths.releases_dir().join("rel-1");
        fs::create_dir_all(&active).unwrap();
        fs::write(
            active.join("config.json"),
            serde_json::to_vec(&serde_json::json!({
                "outbounds":[{"type":"vless","tag":"main","password":"hunter2"}]
            }))
            .unwrap(),
        )
        .unwrap();
        fs::write(active.join("manifest.json"), b"{\"schema_version\":1}").unwrap();
        fs::create_dir_all(paths.etc_dir()).unwrap();
        std::os::unix::fs::symlink(&active, paths.active_symlink()).unwrap();
        fs::write(paths.boxpilot_toml(), b"schema_version = 1\n").unwrap();
        fs::create_dir_all(paths.cores_dir().parent().unwrap()).unwrap();
        fs::write(
            paths.install_state_json(),
            b"{\"schema_version\":1,\"managed_cores\":[]}",
        )
        .unwrap();
        fs::create_dir_all(tmp.path().join("etc/systemd/system")).unwrap();
        fs::write(
            paths.systemd_unit_path("boxpilot-sing-box.service"),
            b"[Service]\nExecStart=/usr/bin/sing-box\n",
        )
        .unwrap();
        let os_release = tmp.path().join("os-release");
        fs::write(
            &os_release,
            b"ID=test\nVERSION_ID=1\nPRETTY_NAME=\"Test\"\n",
        )
        .unwrap();

        let journal = FixedJournal {
            lines: vec!["starting".into(), "password=leak".into(), "running".into()],
        };

        let resp = compose(ComposeInputs {
            paths: &paths,
            unit_name: "boxpilot-sing-box.service",
            journal: &journal,
            os_release_path: &os_release,
            now_iso: iso,
        })
        .await
        .unwrap();

        assert!(resp.bundle_size_bytes > 0);
        assert!(resp.bundle_path.ends_with(".tar.gz"));
        assert_eq!(resp.generated_at, iso());
        assert_eq!(resp.schema_version, 1);

        // Open the tarball and verify the secret is gone.
        let f = std::fs::File::open(&resp.bundle_path).unwrap();
        let gz = flate2::read::GzDecoder::new(f);
        let mut tar = tar::Archive::new(gz);
        let mut found_redacted = false;
        let mut found_journal_redacted = false;
        for entry in tar.entries().unwrap() {
            let mut entry = entry.unwrap();
            let path = entry.path().unwrap().to_string_lossy().into_owned();
            let mut buf = Vec::new();
            std::io::Read::read_to_end(&mut entry, &mut buf).unwrap();
            if path.ends_with("active-config.json") {
                let s = String::from_utf8_lossy(&buf);
                assert!(!s.contains("hunter2"), "active config leaked: {s}");
                assert!(s.contains("***"));
                found_redacted = true;
            }
            if path.ends_with("journal-tail.txt") {
                let s = String::from_utf8_lossy(&buf);
                assert!(!s.contains("leak"), "journal leaked: {s}");
                found_journal_redacted = true;
            }
        }
        assert!(found_redacted);
        assert!(found_journal_redacted);
    }
}
