//! Bundle of trait objects used by every method handler. Keeps the
//! [`crate::iface::Helper`] D-Bus interface struct small and lets unit tests
//! swap any dependency.

use crate::authority::Authority;
use crate::controller::{ControllerState, UserLookup};
use crate::credentials::CallerResolver;
use crate::paths::Paths;
use crate::systemd::SystemdQuery;
use boxpilot_ipc::{BoxpilotConfig, HelperError, HelperResult};
use std::sync::Arc;

pub struct HelperContext {
    pub paths: Paths,
    pub callers: Arc<dyn CallerResolver>,
    pub authority: Arc<dyn Authority>,
    pub systemd: Arc<dyn SystemdQuery>,
    pub user_lookup: Arc<dyn UserLookup>,
    // Cache is intentionally absent. `load_config` reads the file each call;
    // call sites are infrequent (one disk read per `service.status` poll, or
    // per privileged action). When SIGHUP-style reload lands in a later
    // plan, reintroduce a cache here alongside the signal-handling path
    // that invalidates it. A dead `RwLock<Option<BoxpilotConfig>>` field
    // would otherwise mislead readers about caching that never happens.
}

impl HelperContext {
    pub fn new(
        paths: Paths,
        callers: Arc<dyn CallerResolver>,
        authority: Arc<dyn Authority>,
        systemd: Arc<dyn SystemdQuery>,
        user_lookup: Arc<dyn UserLookup>,
    ) -> Self {
        Self {
            paths,
            callers,
            authority,
            systemd,
            user_lookup,
        }
    }

    /// Read `boxpilot.toml`. Missing file → returns a freshly minted v1
    /// config with no fields populated, so the helper still answers
    /// `service.status` on a fresh box (controller is `Unset`).
    pub async fn load_config(&self) -> HelperResult<BoxpilotConfig> {
        let path = self.paths.boxpilot_toml();
        match tokio::fs::read_to_string(&path).await {
            Ok(text) => BoxpilotConfig::parse(&text),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(BoxpilotConfig {
                schema_version: boxpilot_ipc::CURRENT_SCHEMA_VERSION,
                target_service: "boxpilot-sing-box.service".into(),
                core_path: None,
                core_state: None,
                controller_uid: None,
                active_profile_id: None,
                active_profile_name: None,
                active_profile_sha256: None,
                active_release_id: None,
                activated_at: None,
            }),
            Err(e) => Err(HelperError::Ipc {
                message: format!("read {path:?}: {e}"),
            }),
        }
    }

    pub async fn controller_state(&self) -> HelperResult<ControllerState> {
        let cfg = self.load_config().await?;
        Ok(ControllerState::from_uid(
            cfg.controller_uid,
            &*self.user_lookup,
        ))
    }
}

#[cfg(test)]
pub mod testing {
    use super::*;
    use crate::authority::testing::CannedAuthority;
    use crate::controller::PasswdLookup;
    use crate::credentials::testing::FixedResolver;
    use crate::systemd::testing::FixedSystemd;
    use boxpilot_ipc::UnitState;
    use tempfile::TempDir;

    pub fn ctx_with(
        tmp: &TempDir,
        config: Option<&str>,
        authority: CannedAuthority,
        systemd_answer: UnitState,
        callers: &[(&str, u32)],
    ) -> HelperContext {
        let paths = Paths::with_root(tmp.path());
        std::fs::create_dir_all(paths.etc_dir()).unwrap();
        if let Some(text) = config {
            std::fs::write(paths.boxpilot_toml(), text).unwrap();
        }
        HelperContext::new(
            paths,
            Arc::new(FixedResolver::with(callers)),
            Arc::new(authority),
            Arc::new(FixedSystemd {
                answer: systemd_answer,
            }),
            Arc::new(PasswdLookup),
        )
    }

    #[tokio::test]
    async fn load_config_returns_default_on_missing_file() {
        let tmp = tempfile::tempdir().unwrap();
        let ctx = ctx_with(
            &tmp,
            None,
            CannedAuthority::allowing(&[]),
            UnitState::NotFound,
            &[],
        );
        let cfg = ctx.load_config().await.unwrap();
        assert_eq!(cfg.schema_version, boxpilot_ipc::CURRENT_SCHEMA_VERSION);
        assert_eq!(cfg.controller_uid, None);
    }

    #[tokio::test]
    async fn load_config_parses_file_when_present() {
        let tmp = tempfile::tempdir().unwrap();
        let ctx = ctx_with(
            &tmp,
            Some("schema_version = 1\ncontroller_uid = 1000\n"),
            CannedAuthority::allowing(&[]),
            UnitState::NotFound,
            &[],
        );
        let cfg = ctx.load_config().await.unwrap();
        assert_eq!(cfg.controller_uid, Some(1000));
    }

    #[tokio::test]
    async fn load_config_rejects_unknown_schema() {
        let tmp = tempfile::tempdir().unwrap();
        let ctx = ctx_with(
            &tmp,
            Some("schema_version = 2\n"),
            CannedAuthority::allowing(&[]),
            UnitState::NotFound,
            &[],
        );
        let r = ctx.load_config().await;
        assert!(matches!(
            r,
            Err(HelperError::UnsupportedSchemaVersion { got: 2 })
        ));
    }
}
