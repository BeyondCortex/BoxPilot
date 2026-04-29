//! Bundle of trait objects used by every method handler. Keeps the
//! [`crate::iface::Helper`] D-Bus interface struct small and lets unit tests
//! swap any dependency.

use crate::authority::Authority;
use crate::controller::{ControllerState, UserLookup};
use crate::core::download::Downloader;
use crate::core::github::GithubClient;
use crate::core::trust::{FsMetadataProvider, VersionChecker};
use crate::credentials::CallerResolver;
use crate::paths::Paths;
use crate::profile::checker::SingboxChecker;
use crate::profile::verifier::ServiceVerifier;
use crate::systemd::{JournalReader, Systemd};
use boxpilot_ipc::{BoxpilotConfig, HelperError, HelperResult};
use std::sync::Arc;

pub struct HelperContext {
    pub paths: Paths,
    pub callers: Arc<dyn CallerResolver>,
    pub authority: Arc<dyn Authority>,
    pub systemd: Arc<dyn Systemd>,
    pub journal: Arc<dyn JournalReader>,
    pub user_lookup: Arc<dyn UserLookup>,
    pub github: Arc<dyn GithubClient>,
    pub downloader: Arc<dyn Downloader>,
    pub fs_meta: Arc<dyn FsMetadataProvider>,
    pub version_checker: Arc<dyn VersionChecker>,
    pub checker: Arc<dyn SingboxChecker>,
    pub verifier: Arc<dyn ServiceVerifier>,
    // Cache is intentionally absent. `load_config` reads the file each call;
    // call sites are infrequent (one disk read per `service.status` poll, or
    // per privileged action). When SIGHUP-style reload lands in a later
    // plan, reintroduce a cache here alongside the signal-handling path
    // that invalidates it. A dead `RwLock<Option<BoxpilotConfig>>` field
    // would otherwise mislead readers about caching that never happens.
}

impl HelperContext {
    #[allow(clippy::too_many_arguments)] // all 12 args are distinct trait deps; a builder would be overkill
    pub fn new(
        paths: Paths,
        callers: Arc<dyn CallerResolver>,
        authority: Arc<dyn Authority>,
        systemd: Arc<dyn Systemd>,
        journal: Arc<dyn JournalReader>,
        user_lookup: Arc<dyn UserLookup>,
        github: Arc<dyn GithubClient>,
        downloader: Arc<dyn Downloader>,
        fs_meta: Arc<dyn FsMetadataProvider>,
        version_checker: Arc<dyn VersionChecker>,
        checker: Arc<dyn SingboxChecker>,
        verifier: Arc<dyn ServiceVerifier>,
    ) -> Self {
        Self {
            paths,
            callers,
            authority,
            systemd,
            journal,
            user_lookup,
            github,
            downloader,
            fs_meta,
            version_checker,
            checker,
            verifier,
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
                previous_release_id: None,
                previous_profile_id: None,
                previous_profile_sha256: None,
                previous_activated_at: None,
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
        let github = Arc::new(crate::core::github::testing::CannedGithubClient {
            latest: Ok("1.10.0".into()),
            sha256sums: Ok(None),
        });
        let downloader = Arc::new(crate::core::download::testing::FixedDownloader::new(
            Vec::new(),
        ));
        let fs_meta = Arc::new(PermissiveTestFs);
        let version_checker = Arc::new(
            crate::core::trust::version_testing::FixedVersionChecker::ok("sing-box version 1.10.0"),
        );
        let journal = Arc::new(crate::systemd::testing::FixedJournal { lines: Vec::new() });
        HelperContext::new(
            paths,
            Arc::new(FixedResolver::with(callers)),
            Arc::new(authority),
            Arc::new(FixedSystemd {
                answer: systemd_answer,
            }),
            journal,
            Arc::new(PasswdLookup),
            github,
            downloader,
            fs_meta,
            version_checker,
            Arc::new(crate::profile::checker::testing::FakeChecker::ok()),
            Arc::new(crate::profile::verifier::testing::ScriptedVerifier::new(
                vec![],
            )),
        )
    }

    /// Build a context wired to a caller-supplied `Arc<RecordingSystemd>`
    /// so the test can assert on which verb fired after a method runs.
    /// Returns the ctx; the caller already has the Arc.
    pub fn ctx_with_recording(
        tmp: &TempDir,
        config: Option<&str>,
        authority: CannedAuthority,
        rec: Arc<crate::systemd::testing::RecordingSystemd>,
        callers: &[(&str, u32)],
    ) -> HelperContext {
        let paths = Paths::with_root(tmp.path());
        std::fs::create_dir_all(paths.etc_dir()).unwrap();
        if let Some(text) = config {
            std::fs::write(paths.boxpilot_toml(), text).unwrap();
        }
        let github = Arc::new(crate::core::github::testing::CannedGithubClient {
            latest: Ok("1.10.0".into()),
            sha256sums: Ok(None),
        });
        let downloader = Arc::new(crate::core::download::testing::FixedDownloader::new(Vec::new()));
        let journal = Arc::new(crate::systemd::testing::FixedJournal { lines: Vec::new() });
        HelperContext::new(
            paths,
            Arc::new(FixedResolver::with(callers)),
            Arc::new(authority),
            rec,
            journal,
            Arc::new(PasswdLookup),
            github,
            downloader,
            Arc::new(PermissiveTestFs),
            Arc::new(crate::core::trust::version_testing::FixedVersionChecker::ok(
                "sing-box version 1.10.0",
            )),
            Arc::new(crate::profile::checker::testing::FakeChecker::ok()),
            Arc::new(crate::profile::verifier::testing::ScriptedVerifier::new(
                vec![],
            )),
        )
    }

    /// Like `ctx_with` but lets the caller seed the journal tail with
    /// canned lines for `service.logs` tests.
    pub fn ctx_with_journal_lines(
        tmp: &TempDir,
        config: Option<&str>,
        authority: CannedAuthority,
        systemd_answer: UnitState,
        callers: &[(&str, u32)],
        lines: Vec<String>,
    ) -> HelperContext {
        let paths = Paths::with_root(tmp.path());
        std::fs::create_dir_all(paths.etc_dir()).unwrap();
        if let Some(text) = config {
            std::fs::write(paths.boxpilot_toml(), text).unwrap();
        }
        let github = Arc::new(crate::core::github::testing::CannedGithubClient {
            latest: Ok("1.10.0".into()),
            sha256sums: Ok(None),
        });
        let downloader = Arc::new(crate::core::download::testing::FixedDownloader::new(Vec::new()));
        let journal = Arc::new(crate::systemd::testing::FixedJournal { lines });
        HelperContext::new(
            paths,
            Arc::new(FixedResolver::with(callers)),
            Arc::new(authority),
            Arc::new(crate::systemd::testing::FixedSystemd { answer: systemd_answer }),
            journal,
            Arc::new(PasswdLookup),
            github,
            downloader,
            Arc::new(PermissiveTestFs),
            Arc::new(crate::core::trust::version_testing::FixedVersionChecker::ok(
                "sing-box version 1.10.0",
            )),
            Arc::new(crate::profile::checker::testing::FakeChecker::ok()),
            Arc::new(crate::profile::verifier::testing::ScriptedVerifier::new(
                vec![],
            )),
        )
    }

    /// A permissive test `FsMetadataProvider` that reports every path as a
    /// root-owned 0o755 regular file (for leaf paths ending with a known
    /// binary name) or directory (for all other paths). This lets tests that
    /// probe trust checks against `/usr/bin/sing-box` or similar pass without
    /// requiring a real filesystem.
    pub struct PermissiveTestFs;

    impl crate::core::trust::FsMetadataProvider for PermissiveTestFs {
        fn stat(&self, path: &std::path::Path) -> std::io::Result<crate::core::trust::FileStat> {
            use crate::core::trust::{FileKind, FileStat};
            let is_binary = path
                .file_name()
                .and_then(|n| n.to_str())
                .map(|n| n == "sing-box")
                .unwrap_or(false);
            Ok(FileStat {
                uid: 0,
                gid: 0,
                mode: 0o755,
                kind: if is_binary {
                    FileKind::Regular
                } else {
                    FileKind::Directory
                },
            })
        }
        fn read_link(&self, _path: &std::path::Path) -> std::io::Result<std::path::PathBuf> {
            Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "test",
            ))
        }
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
