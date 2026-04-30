//! `legacy.observe_service` (§6.3, ReadOnly): probe `sing-box.service`,
//! return runtime state + on-disk fragment + extracted config path.

use crate::legacy::path_safety::classify_config_path;
use crate::legacy::unit_parser::parse_exec_start;
use crate::systemd::Systemd;
use boxpilot_ipc::{
    BoxpilotConfig, ConfigPathKind, HelperResult, LegacyObserveServiceResponse, UnitState,
    LEGACY_UNIT_NAME,
};

pub struct ObserveDeps<'a> {
    pub systemd: &'a dyn Systemd,
    pub fs_read: &'a dyn FragmentReader,
}

/// Read the contents of an on-disk unit fragment. Defined as a trait so the
/// observe orchestrator can be tested without touching the real filesystem.
pub trait FragmentReader: Send + Sync {
    fn read_to_string(&self, path: &std::path::Path) -> std::io::Result<String>;
}

pub struct StdFsFragmentReader;
impl FragmentReader for StdFsFragmentReader {
    fn read_to_string(&self, path: &std::path::Path) -> std::io::Result<String> {
        std::fs::read_to_string(path)
    }
}

pub async fn observe(
    cfg: &BoxpilotConfig,
    deps: &ObserveDeps<'_>,
) -> HelperResult<LegacyObserveServiceResponse> {
    let unit_state = deps.systemd.unit_state(LEGACY_UNIT_NAME).await?;
    let detected = !matches!(unit_state, UnitState::NotFound);

    if !detected {
        return Ok(LegacyObserveServiceResponse {
            detected: false,
            unit_name: None,
            fragment_path: None,
            unit_file_state: None,
            exec_start_raw: None,
            config_path: None,
            config_path_kind: ConfigPathKind::Unknown,
            unit_state,
            conflicts_with_managed: false,
        });
    }

    let fragment_path = deps.systemd.fragment_path(LEGACY_UNIT_NAME).await?;
    let unit_file_state = deps.systemd.unit_file_state(LEGACY_UNIT_NAME).await?;

    let (exec_start_raw, config_path) = match fragment_path.as_deref() {
        Some(p) => match deps.fs_read.read_to_string(std::path::Path::new(p)) {
            Ok(text) => match parse_exec_start(&text) {
                Ok(es) => (
                    Some(es.raw),
                    es.config_path.map(|p| p.to_string_lossy().into_owned()),
                ),
                Err(_) => (None, None),
            },
            Err(_) => (None, None),
        },
        None => (None, None),
    };

    let kind = match config_path.as_deref() {
        Some(p) => classify_config_path(std::path::Path::new(p)),
        None => ConfigPathKind::Unknown,
    };

    Ok(LegacyObserveServiceResponse {
        detected: true,
        unit_name: Some(LEGACY_UNIT_NAME.to_string()),
        fragment_path,
        unit_file_state,
        exec_start_raw,
        config_path,
        config_path_kind: kind,
        unit_state,
        conflicts_with_managed: cfg.target_service == LEGACY_UNIT_NAME,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::systemd::testing::FixedSystemd;
    use boxpilot_ipc::{BoxpilotConfig, CoreState};
    use std::collections::HashMap;
    use std::path::Path;
    use std::sync::Mutex;

    struct MapFsReader {
        files: Mutex<HashMap<String, String>>,
    }
    impl MapFsReader {
        fn new(files: &[(&str, &str)]) -> Self {
            let mut m = HashMap::new();
            for (k, v) in files {
                m.insert((*k).to_string(), (*v).to_string());
            }
            MapFsReader {
                files: Mutex::new(m),
            }
        }
    }
    impl FragmentReader for MapFsReader {
        fn read_to_string(&self, path: &Path) -> std::io::Result<String> {
            self.files
                .lock()
                .unwrap()
                .get(path.to_string_lossy().as_ref())
                .cloned()
                .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "no such file"))
        }
    }

    fn empty_cfg() -> BoxpilotConfig {
        BoxpilotConfig {
            schema_version: 1,
            target_service: "boxpilot-sing-box.service".into(),
            core_path: None,
            core_state: Some(CoreState::External),
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

    #[tokio::test]
    async fn detected_false_when_unit_not_loaded() {
        let sd = FixedSystemd::new_with_fragment(UnitState::NotFound, None, None);
        let fs = MapFsReader::new(&[]);
        let cfg = empty_cfg();
        let deps = ObserveDeps {
            systemd: &sd,
            fs_read: &fs,
        };
        let r = observe(&cfg, &deps).await.unwrap();
        assert!(!r.detected);
        assert_eq!(r.config_path_kind, ConfigPathKind::Unknown);
    }

    #[tokio::test]
    async fn detected_true_with_system_path_config() {
        let sd = FixedSystemd::new_with_fragment(
            UnitState::Known {
                active_state: "active".into(),
                sub_state: "running".into(),
                load_state: "loaded".into(),
                n_restarts: 0,
                exec_main_status: 0,
            },
            Some("/etc/systemd/system/sing-box.service".into()),
            Some("enabled".into()),
        );
        let fs = MapFsReader::new(&[(
            "/etc/systemd/system/sing-box.service",
            "[Service]\nExecStart=/usr/bin/sing-box run -c /etc/sing-box/config.json\n",
        )]);
        let cfg = empty_cfg();
        let deps = ObserveDeps {
            systemd: &sd,
            fs_read: &fs,
        };
        let r = observe(&cfg, &deps).await.unwrap();
        assert!(r.detected);
        assert_eq!(r.unit_name.as_deref(), Some("sing-box.service"));
        assert_eq!(
            r.config_path.as_deref(),
            Some("/etc/sing-box/config.json")
        );
        assert_eq!(r.config_path_kind, ConfigPathKind::SystemPath);
        assert_eq!(r.unit_file_state.as_deref(), Some("enabled"));
    }

    #[tokio::test]
    async fn user_path_config_is_flagged_unsafe() {
        let sd = FixedSystemd::new_with_fragment(
            UnitState::Known {
                active_state: "inactive".into(),
                sub_state: "dead".into(),
                load_state: "loaded".into(),
                n_restarts: 0,
                exec_main_status: 0,
            },
            Some("/etc/systemd/system/sing-box.service".into()),
            Some("disabled".into()),
        );
        let fs = MapFsReader::new(&[(
            "/etc/systemd/system/sing-box.service",
            "[Service]\nExecStart=/usr/bin/sing-box run -c /home/alice/sb/c.json\n",
        )]);
        let cfg = empty_cfg();
        let deps = ObserveDeps {
            systemd: &sd,
            fs_read: &fs,
        };
        let r = observe(&cfg, &deps).await.unwrap();
        assert_eq!(r.config_path_kind, ConfigPathKind::UserOrEphemeral);
    }

    #[tokio::test]
    async fn unparseable_exec_start_falls_back_to_unknown() {
        let sd = FixedSystemd::new_with_fragment(
            UnitState::Known {
                active_state: "active".into(),
                sub_state: "running".into(),
                load_state: "loaded".into(),
                n_restarts: 0,
                exec_main_status: 0,
            },
            Some("/etc/systemd/system/sing-box.service".into()),
            Some("enabled".into()),
        );
        let fs = MapFsReader::new(&[(
            "/etc/systemd/system/sing-box.service",
            "[Unit]\nDescription=broken\n",
        )]);
        let cfg = empty_cfg();
        let deps = ObserveDeps {
            systemd: &sd,
            fs_read: &fs,
        };
        let r = observe(&cfg, &deps).await.unwrap();
        assert!(r.detected);
        assert_eq!(r.config_path_kind, ConfigPathKind::Unknown);
        assert_eq!(r.exec_start_raw, None);
    }

    #[tokio::test]
    async fn detects_conflict_when_target_service_matches_legacy_name() {
        let sd = FixedSystemd::new_with_fragment(
            UnitState::Known {
                active_state: "active".into(),
                sub_state: "running".into(),
                load_state: "loaded".into(),
                n_restarts: 0,
                exec_main_status: 0,
            },
            Some("/etc/systemd/system/sing-box.service".into()),
            Some("enabled".into()),
        );
        let fs = MapFsReader::new(&[(
            "/etc/systemd/system/sing-box.service",
            "[Service]\nExecStart=/usr/bin/sing-box run -c /etc/sing-box/config.json\n",
        )]);
        let mut cfg = empty_cfg();
        cfg.target_service = "sing-box.service".into();
        let deps = ObserveDeps {
            systemd: &sd,
            fs_read: &fs,
        };
        let r = observe(&cfg, &deps).await.unwrap();
        assert!(r.conflicts_with_managed);
    }
}
