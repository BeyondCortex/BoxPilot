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
    let exec = parse_exec_start(&unit_text).map_err(|e| HelperError::LegacyExecStartUnparseable {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::systemd::testing::FixedSystemd;
    use std::collections::HashMap;
    use std::sync::Mutex;

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
        assert!(matches!(r, Err(HelperError::LegacyConflictsWithManaged { .. })));
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
}
