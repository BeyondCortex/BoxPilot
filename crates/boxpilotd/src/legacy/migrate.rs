//! `legacy.migrate_service` (§6.3, HighRisk): two phases.
//!
//! - `Prepare` (read-only with respect to system state, but privileged so we
//!   can read root-owned configs): read fragment + ExecStart + config bytes
//!   + sibling assets, return them to the user-side.
//! - `Cutover` (mutating): stop + disable the legacy unit, back up its
//!   fragment under `/var/lib/boxpilot/backups/units/`. Atomically replaces
//!   the "two services running concurrently" risk with "neither running";
//!   the standard activation pipeline (plan #5) then enables + starts
//!   `boxpilot-sing-box.service` from the imported profile.

use crate::legacy::observe::FragmentReader;
use crate::legacy::path_safety::classify_config_path;
use crate::legacy::unit_parser::parse_exec_start;
use boxpilot_ipc::{
    BoxpilotConfig, ConfigPathKind, HelperError, HelperResult, LegacyMigratePrepareResponse,
    MigratedAsset, BUNDLE_MAX_FILE_BYTES, BUNDLE_MAX_FILE_COUNT, LEGACY_UNIT_NAME,
};
use std::path::{Path, PathBuf};

pub struct PrepareDeps<'a> {
    pub systemd: &'a dyn crate::systemd::Systemd,
    pub fs_read: &'a dyn FragmentReader,
    pub config_reader: &'a dyn ConfigReader,
}

/// Read root-owned config + sibling files. Trait so the migrate logic can be
/// tested without running as root.
pub trait ConfigReader: Send + Sync {
    fn read_file(&self, path: &Path) -> std::io::Result<Vec<u8>>;
    fn read_dir(&self, path: &Path) -> std::io::Result<Vec<DirEntry>>;
    fn metadata_len(&self, path: &Path) -> std::io::Result<u64>;
}

#[derive(Debug, Clone)]
pub struct DirEntry {
    pub path: PathBuf,
    pub is_file: bool,
    pub is_symlink: bool,
}

pub struct StdConfigReader;
impl ConfigReader for StdConfigReader {
    fn read_file(&self, path: &Path) -> std::io::Result<Vec<u8>> {
        std::fs::read(path)
    }
    fn read_dir(&self, path: &Path) -> std::io::Result<Vec<DirEntry>> {
        let mut out = Vec::new();
        for e in std::fs::read_dir(path)? {
            let e = e?;
            let ft = std::fs::symlink_metadata(e.path())?.file_type();
            out.push(DirEntry {
                path: e.path(),
                is_file: ft.is_file(),
                is_symlink: ft.is_symlink(),
            });
        }
        Ok(out)
    }
    fn metadata_len(&self, path: &Path) -> std::io::Result<u64> {
        Ok(std::fs::symlink_metadata(path)?.len())
    }
}

pub async fn prepare(
    cfg: &BoxpilotConfig,
    deps: &PrepareDeps<'_>,
) -> HelperResult<LegacyMigratePrepareResponse> {
    if cfg.target_service == LEGACY_UNIT_NAME {
        return Err(HelperError::LegacyConflictsWithManaged {
            unit: LEGACY_UNIT_NAME.to_string(),
        });
    }
    let unit_state = deps.systemd.unit_state(LEGACY_UNIT_NAME).await?;
    if matches!(unit_state, boxpilot_ipc::UnitState::NotFound) {
        return Err(HelperError::LegacyUnitNotFound {
            unit: LEGACY_UNIT_NAME.to_string(),
        });
    }
    let fragment_path = deps
        .systemd
        .fragment_path(LEGACY_UNIT_NAME)
        .await?
        .ok_or_else(|| HelperError::LegacyExecStartUnparseable {
            reason: "unit has no FragmentPath (transient unit?)".into(),
        })?;
    let unit_text = deps
        .fs_read
        .read_to_string(Path::new(&fragment_path))
        .map_err(|e| HelperError::Ipc {
            message: format!("read fragment {fragment_path}: {e}"),
        })?;
    let exec =
        parse_exec_start(&unit_text).map_err(|e| HelperError::LegacyExecStartUnparseable {
            reason: e.to_string(),
        })?;
    let config_path = exec
        .config_path
        .ok_or_else(|| HelperError::LegacyExecStartUnparseable {
            reason: "ExecStart had no -c/--config argument".into(),
        })?;
    let kind = classify_config_path(&config_path);
    if matches!(kind, ConfigPathKind::UserOrEphemeral) {
        return Err(HelperError::LegacyConfigPathUnsafe {
            path: config_path.to_string_lossy().into_owned(),
        });
    }
    let cfg_bytes = deps
        .config_reader
        .read_file(&config_path)
        .map_err(|e| HelperError::Ipc {
            message: format!("read legacy config {}: {e}", config_path.display()),
        })?;
    if cfg_bytes.len() as u64 > BUNDLE_MAX_FILE_BYTES {
        return Err(HelperError::LegacyAssetTooLarge {
            path: config_path.to_string_lossy().into_owned(),
            size: cfg_bytes.len() as u64,
            limit: BUNDLE_MAX_FILE_BYTES,
        });
    }

    // Enumerate siblings.
    let parent = config_path.parent().ok_or_else(|| HelperError::Ipc {
        message: "legacy config path has no parent".into(),
    })?;
    let mut assets = Vec::new();
    let entries = deps
        .config_reader
        .read_dir(parent)
        .map_err(|e| HelperError::Ipc {
            message: format!("read_dir {}: {e}", parent.display()),
        })?;
    let mut count = 0u32;
    let mut total: u64 = cfg_bytes.len() as u64;
    for e in entries {
        if e.is_symlink || !e.is_file {
            continue;
        }
        if e.path == config_path {
            continue;
        }
        if count >= BUNDLE_MAX_FILE_COUNT - 1 {
            return Err(HelperError::LegacyTooManyAssets {
                count: count + 1,
                limit: BUNDLE_MAX_FILE_COUNT - 1,
            });
        }
        let size = deps
            .config_reader
            .metadata_len(&e.path)
            .map_err(|err| HelperError::Ipc {
                message: format!("stat {}: {err}", e.path.display()),
            })?;
        if size > BUNDLE_MAX_FILE_BYTES {
            return Err(HelperError::LegacyAssetTooLarge {
                path: e.path.to_string_lossy().into_owned(),
                size,
                limit: BUNDLE_MAX_FILE_BYTES,
            });
        }
        total = total.saturating_add(size);
        if total > boxpilot_ipc::BUNDLE_MAX_TOTAL_BYTES {
            return Err(HelperError::LegacyAssetTooLarge {
                path: e.path.to_string_lossy().into_owned(),
                size: total,
                limit: boxpilot_ipc::BUNDLE_MAX_TOTAL_BYTES,
            });
        }
        let bytes = deps
            .config_reader
            .read_file(&e.path)
            .map_err(|err| HelperError::Ipc {
                message: format!("read {}: {err}", e.path.display()),
            })?;
        let filename = e
            .path
            .file_name()
            .and_then(|s| s.to_str())
            .ok_or_else(|| HelperError::Ipc {
                message: format!("non-utf8 filename under {}", parent.display()),
            })?
            .to_string();
        assets.push(MigratedAsset { filename, bytes });
        count += 1;
    }

    let config_filename = config_path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("config.json")
        .to_string();

    Ok(LegacyMigratePrepareResponse {
        unit_name: LEGACY_UNIT_NAME.to_string(),
        config_path_was: config_path.to_string_lossy().into_owned(),
        config_filename,
        config_bytes: cfg_bytes,
        assets,
    })
}

pub struct CutoverDeps<'a> {
    pub systemd: &'a dyn crate::systemd::Systemd,
    pub backups_units_dir: &'a Path,
    pub now_iso: fn() -> String,
}

pub async fn cutover(
    deps: &CutoverDeps<'_>,
    unit_name: &str,
) -> HelperResult<boxpilot_ipc::LegacyMigrateCutoverResponse> {
    // Read FragmentPath BEFORE any mutation. Disable can remove a symlink
    // fragment and make the post-disable lookup return None, silently
    // skipping the backup. Capture it now so the backup is always written
    // when one exists on disk.
    let fragment_path = deps.systemd.fragment_path(unit_name).await.ok().flatten();

    let backup_path = match fragment_path {
        Some(p) => crate::legacy::backup::backup_unit_file(
            Path::new(&p),
            deps.backups_units_dir,
            unit_name,
            &(deps.now_iso)(),
        )
        .await
        .map(|pb| pb.to_string_lossy().into_owned())?,
        None => String::new(), // unit had no on-disk fragment; backup is a no-op
    };

    deps.systemd
        .stop_unit(unit_name)
        .await
        .map_err(|e| HelperError::LegacyStopFailed {
            unit: unit_name.to_string(),
            message: match e {
                HelperError::Systemd { message } => message,
                other => format!("{other}"),
            },
        })?;

    deps.systemd
        .disable_unit_files(&[unit_name.to_string()])
        .await
        .map_err(|e| HelperError::LegacyDisableFailed {
            unit: unit_name.to_string(),
            message: match e {
                HelperError::Systemd { message } => message,
                other => format!("{other}"),
            },
        })?;

    let final_unit_state = deps.systemd.unit_state(unit_name).await?;

    Ok(boxpilot_ipc::LegacyMigrateCutoverResponse {
        unit_name: unit_name.to_string(),
        backup_unit_path: backup_path,
        final_unit_state,
    })
}

/// Single entry point that dispatches `LegacyMigrateRequest::Prepare` /
/// `Cutover` to the right helper. Used by `iface::do_legacy_migrate_service`.
pub async fn run(
    cfg: &BoxpilotConfig,
    req: boxpilot_ipc::LegacyMigrateRequest,
    prep_deps: &PrepareDeps<'_>,
    cut_deps: &CutoverDeps<'_>,
) -> HelperResult<boxpilot_ipc::LegacyMigrateResponse> {
    match req {
        boxpilot_ipc::LegacyMigrateRequest::Prepare => {
            let r = prepare(cfg, prep_deps).await?;
            Ok(boxpilot_ipc::LegacyMigrateResponse::Prepare(r))
        }
        boxpilot_ipc::LegacyMigrateRequest::Cutover => {
            if cfg.target_service == LEGACY_UNIT_NAME {
                return Err(HelperError::LegacyConflictsWithManaged {
                    unit: LEGACY_UNIT_NAME.to_string(),
                });
            }
            let r = cutover(cut_deps, LEGACY_UNIT_NAME).await?;
            Ok(boxpilot_ipc::LegacyMigrateResponse::Cutover(r))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::systemd::testing::{FixedSystemd, RecordedCall, RecordingSystemd};
    use std::collections::HashMap;
    use std::sync::Mutex;
    use tempfile::tempdir;

    /// Composite test fake: serves both as a `FragmentReader` (for fragment
    /// text) and as a `ConfigReader` (for config + sibling enumeration).
    /// Uses `Mutex<HashMap>` because both traits require `Send + Sync`.
    struct MapFs {
        files: Mutex<HashMap<String, Vec<u8>>>,
        dirs: Mutex<HashMap<String, Vec<DirEntry>>>,
    }
    impl MapFs {
        fn new() -> Self {
            Self {
                files: Mutex::new(HashMap::new()),
                dirs: Mutex::new(HashMap::new()),
            }
        }
        fn add_file(&self, p: &str, bytes: &[u8]) {
            self.files
                .lock()
                .unwrap()
                .insert(p.to_string(), bytes.to_vec());
        }
        fn set_dir(&self, p: &str, entries: Vec<DirEntry>) {
            self.dirs.lock().unwrap().insert(p.to_string(), entries);
        }
    }
    impl crate::legacy::observe::FragmentReader for MapFs {
        fn read_to_string(&self, path: &Path) -> std::io::Result<String> {
            self.files
                .lock()
                .unwrap()
                .get(path.to_string_lossy().as_ref())
                .map(|v| String::from_utf8_lossy(v).into_owned())
                .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "no fragment"))
        }
    }
    impl ConfigReader for MapFs {
        fn read_file(&self, path: &Path) -> std::io::Result<Vec<u8>> {
            self.files
                .lock()
                .unwrap()
                .get(path.to_string_lossy().as_ref())
                .cloned()
                .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "no file"))
        }
        fn read_dir(&self, path: &Path) -> std::io::Result<Vec<DirEntry>> {
            self.dirs
                .lock()
                .unwrap()
                .get(path.to_string_lossy().as_ref())
                .cloned()
                .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "no dir"))
        }
        fn metadata_len(&self, path: &Path) -> std::io::Result<u64> {
            self.files
                .lock()
                .unwrap()
                .get(path.to_string_lossy().as_ref())
                .map(|v| v.len() as u64)
                .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "no file"))
        }
    }

    fn cfg() -> BoxpilotConfig {
        BoxpilotConfig {
            schema_version: 1,
            target_service: "boxpilot-sing-box.service".into(),
            core_path: None,
            core_state: None,
            controller_uid: None,
            active_profile_id: None,
            active_profile_name: None,
            active_profile_sha256: None,
            active_release_id: None,
            activated_at: None,
            previous_release_id: None,
            previous_profile_id: None,
            previous_profile_sha256: None,
            previous_activated_at: None,
        }
    }

    fn systemd_with_fragment(state: boxpilot_ipc::UnitState) -> FixedSystemd {
        FixedSystemd::new_with_fragment(
            state,
            Some("/etc/systemd/system/sing-box.service".into()),
            Some("enabled".into()),
        )
    }

    #[tokio::test]
    async fn prepare_returns_config_and_assets_for_safe_path() {
        let fs = MapFs::new();
        fs.add_file(
            "/etc/systemd/system/sing-box.service",
            b"[Service]\nExecStart=/usr/bin/sing-box run -c /etc/sing-box/config.json\n",
        );
        fs.add_file("/etc/sing-box/config.json", br#"{"log":{}}"#);
        fs.add_file("/etc/sing-box/geosite.db", b"asset-bytes");
        fs.set_dir(
            "/etc/sing-box",
            vec![
                DirEntry {
                    path: PathBuf::from("/etc/sing-box/config.json"),
                    is_file: true,
                    is_symlink: false,
                },
                DirEntry {
                    path: PathBuf::from("/etc/sing-box/geosite.db"),
                    is_file: true,
                    is_symlink: false,
                },
            ],
        );
        let sd = systemd_with_fragment(boxpilot_ipc::UnitState::Known {
            active_state: "active".into(),
            sub_state: "running".into(),
            load_state: "loaded".into(),
            n_restarts: 0,
            exec_main_status: 0,
        });
        let deps = PrepareDeps {
            systemd: &sd,
            fs_read: &fs,
            config_reader: &fs,
        };
        let r = prepare(&cfg(), &deps).await.unwrap();
        assert_eq!(r.config_filename, "config.json");
        assert_eq!(r.config_bytes, br#"{"log":{}}"#);
        assert_eq!(r.assets.len(), 1);
        assert_eq!(r.assets[0].filename, "geosite.db");
        assert_eq!(r.assets[0].bytes, b"asset-bytes");
    }

    #[tokio::test]
    async fn prepare_refuses_user_path() {
        let fs = MapFs::new();
        fs.add_file(
            "/etc/systemd/system/sing-box.service",
            b"[Service]\nExecStart=/usr/bin/sing-box run -c /home/alice/sb/c.json\n",
        );
        let sd = systemd_with_fragment(boxpilot_ipc::UnitState::Known {
            active_state: "active".into(),
            sub_state: "running".into(),
            load_state: "loaded".into(),
            n_restarts: 0,
            exec_main_status: 0,
        });
        let deps = PrepareDeps {
            systemd: &sd,
            fs_read: &fs,
            config_reader: &fs,
        };
        let r = prepare(&cfg(), &deps).await;
        assert!(matches!(r, Err(HelperError::LegacyConfigPathUnsafe { .. })));
    }

    #[tokio::test]
    async fn prepare_refuses_when_unit_not_found() {
        let fs = MapFs::new();
        let sd = systemd_with_fragment(boxpilot_ipc::UnitState::NotFound);
        let deps = PrepareDeps {
            systemd: &sd,
            fs_read: &fs,
            config_reader: &fs,
        };
        let r = prepare(&cfg(), &deps).await;
        assert!(matches!(r, Err(HelperError::LegacyUnitNotFound { .. })));
    }

    #[tokio::test]
    async fn prepare_refuses_when_target_service_collides_with_legacy_name() {
        let fs = MapFs::new();
        let sd = systemd_with_fragment(boxpilot_ipc::UnitState::Known {
            active_state: "active".into(),
            sub_state: "running".into(),
            load_state: "loaded".into(),
            n_restarts: 0,
            exec_main_status: 0,
        });
        let mut c = cfg();
        c.target_service = "sing-box.service".into();
        let deps = PrepareDeps {
            systemd: &sd,
            fs_read: &fs,
            config_reader: &fs,
        };
        let r = prepare(&c, &deps).await;
        assert!(matches!(
            r,
            Err(HelperError::LegacyConflictsWithManaged { .. })
        ));
    }

    #[tokio::test]
    async fn prepare_skips_symlinks_and_subdirs() {
        let fs = MapFs::new();
        fs.add_file(
            "/etc/systemd/system/sing-box.service",
            b"[Service]\nExecStart=/usr/bin/sing-box run -c /etc/sing-box/config.json\n",
        );
        fs.add_file("/etc/sing-box/config.json", b"{}");
        fs.set_dir(
            "/etc/sing-box",
            vec![
                DirEntry {
                    path: PathBuf::from("/etc/sing-box/config.json"),
                    is_file: true,
                    is_symlink: false,
                },
                DirEntry {
                    path: PathBuf::from("/etc/sing-box/symlink-to-secret"),
                    is_file: false,
                    is_symlink: true,
                },
                DirEntry {
                    path: PathBuf::from("/etc/sing-box/subdir"),
                    is_file: false,
                    is_symlink: false,
                },
            ],
        );
        let sd = systemd_with_fragment(boxpilot_ipc::UnitState::Known {
            active_state: "active".into(),
            sub_state: "running".into(),
            load_state: "loaded".into(),
            n_restarts: 0,
            exec_main_status: 0,
        });
        let deps = PrepareDeps {
            systemd: &sd,
            fs_read: &fs,
            config_reader: &fs,
        };
        let r = prepare(&cfg(), &deps).await.unwrap();
        assert!(r.assets.is_empty());
    }

    #[tokio::test]
    async fn cutover_stops_then_disables_then_backs_up() {
        let tmp = tempdir().unwrap();
        // Stage a fragment file so backup_unit_file has something to copy.
        let fragments = tmp.path().join("etc/systemd/system");
        tokio::fs::create_dir_all(&fragments).await.unwrap();
        let fragment = fragments.join("sing-box.service");
        tokio::fs::write(&fragment, b"[Service]\nExecStart=foo\n")
            .await
            .unwrap();

        let recording = RecordingSystemd::new(boxpilot_ipc::UnitState::NotFound);
        recording.set_fragment_path(Some(fragment.to_string_lossy().into_owned()));

        let backups = tmp.path().join("var/lib/boxpilot/backups/units");
        let resp = cutover(
            &CutoverDeps {
                systemd: &recording,
                backups_units_dir: &backups,
                now_iso: || "2026-04-29T00-00-00Z".into(),
            },
            "sing-box.service",
        )
        .await
        .unwrap();

        // Order: StopUnit before DisableUnitFiles. backup arrives after.
        let calls = recording.calls();
        let stop_idx = calls
            .iter()
            .position(|c| matches!(c, RecordedCall::StopUnit(u) if u == "sing-box.service"))
            .expect("stop call");
        let disable_idx = calls
            .iter()
            .position(|c| matches!(c, RecordedCall::DisableUnitFiles(v) if v == &vec!["sing-box.service".to_string()]))
            .expect("disable call");
        assert!(stop_idx < disable_idx, "stop must precede disable");

        assert!(resp
            .backup_unit_path
            .starts_with(&backups.to_string_lossy().into_owned()));
        assert!(tokio::fs::metadata(&resp.backup_unit_path).await.is_ok());
    }

    #[tokio::test]
    async fn cutover_aborts_when_stop_returns_systemd_error() {
        // FixedSystemd returns Ok(()) for stop, so use a small wrapper
        // that fails StopUnit; reuse the test struct.
        struct StopFails;
        #[async_trait::async_trait]
        impl crate::systemd::Systemd for StopFails {
            async fn unit_state(&self, _: &str) -> Result<boxpilot_ipc::UnitState, HelperError> {
                Ok(boxpilot_ipc::UnitState::NotFound)
            }
            async fn start_unit(&self, _: &str) -> Result<(), HelperError> {
                Ok(())
            }
            async fn stop_unit(&self, _: &str) -> Result<(), HelperError> {
                Err(HelperError::Systemd {
                    message: "EBUSY".into(),
                })
            }
            async fn restart_unit(&self, _: &str) -> Result<(), HelperError> {
                Ok(())
            }
            async fn enable_unit_files(&self, _: &[String]) -> Result<(), HelperError> {
                Ok(())
            }
            async fn disable_unit_files(&self, _: &[String]) -> Result<(), HelperError> {
                Ok(())
            }
            async fn reload(&self) -> Result<(), HelperError> {
                Ok(())
            }
            async fn fragment_path(&self, _: &str) -> Result<Option<String>, HelperError> {
                Ok(None)
            }
            async fn unit_file_state(&self, _: &str) -> Result<Option<String>, HelperError> {
                Ok(None)
            }
        }
        let tmp = tempdir().unwrap();
        let r = cutover(
            &CutoverDeps {
                systemd: &StopFails,
                backups_units_dir: &tmp.path().join("b/u"),
                now_iso: || "ts".into(),
            },
            "sing-box.service",
        )
        .await;
        assert!(matches!(r, Err(HelperError::LegacyStopFailed { .. })));
    }

    #[tokio::test]
    async fn prepare_rejects_when_cumulative_assets_exceed_total_cap() {
        // Two huge assets that individually fit (each = MAX_FILE / 2 + 1 byte)
        // but together exceed MAX_TOTAL.
        let half_plus_one = (boxpilot_ipc::BUNDLE_MAX_FILE_BYTES / 2 + 1) as usize;
        // Need to push past MAX_TOTAL with a few large files. With 16 MiB
        // BUNDLE_MAX_FILE_BYTES and 64 MiB BUNDLE_MAX_TOTAL_BYTES, four
        // ~16 MiB files = 64 MiB which is exactly the cap; five push past.
        let big = vec![0u8; boxpilot_ipc::BUNDLE_MAX_FILE_BYTES as usize];
        let _ = half_plus_one; // silence the warning
        let fs = MapFs::new();
        fs.add_file(
            "/etc/systemd/system/sing-box.service",
            b"[Service]\nExecStart=/usr/bin/sing-box run -c /etc/sb/c.json\n",
        );
        fs.add_file("/etc/sb/c.json", b"{}");
        for i in 0..5 {
            let p = format!("/etc/sb/asset{i}");
            fs.add_file(&p, &big);
        }
        let entries: Vec<DirEntry> = (0..5)
            .map(|i| DirEntry {
                path: PathBuf::from(format!("/etc/sb/asset{i}")),
                is_file: true,
                is_symlink: false,
            })
            .chain(std::iter::once(DirEntry {
                path: PathBuf::from("/etc/sb/c.json"),
                is_file: true,
                is_symlink: false,
            }))
            .collect();
        fs.set_dir("/etc/sb", entries);
        let sd = systemd_with_fragment(boxpilot_ipc::UnitState::Known {
            active_state: "active".into(),
            sub_state: "running".into(),
            load_state: "loaded".into(),
            n_restarts: 0,
            exec_main_status: 0,
        });
        let deps = PrepareDeps {
            systemd: &sd,
            fs_read: &fs,
            config_reader: &fs,
        };
        let r = prepare(&cfg(), &deps).await;
        assert!(matches!(r, Err(HelperError::LegacyAssetTooLarge { .. })));
    }
}
