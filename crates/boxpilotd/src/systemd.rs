//! systemd query and control layer.  Read access (`GetUnit` + `Properties.Get`)
//! is unauthenticated on the system bus when the daemon runs as root.
//! Service-control verbs (`StartUnit`, `StopUnit`, …) are added in plan #3.

use async_trait::async_trait;
use boxpilot_ipc::{HelperError, UnitState};
use zbus::{proxy, Connection};

#[async_trait]
pub trait Systemd: Send + Sync {
    async fn unit_state(&self, unit_name: &str) -> Result<UnitState, HelperError>;
    async fn start_unit(&self, unit_name: &str) -> Result<(), HelperError>;
    async fn stop_unit(&self, unit_name: &str) -> Result<(), HelperError>;
    async fn restart_unit(&self, unit_name: &str) -> Result<(), HelperError>;
    async fn enable_unit_files(&self, unit_names: &[String]) -> Result<(), HelperError>;
    async fn disable_unit_files(&self, unit_names: &[String]) -> Result<(), HelperError>;
    /// Equivalent to `systemctl daemon-reload`. Required after writing a
    /// new unit file so systemd parses it before the next StartUnit.
    async fn reload(&self) -> Result<(), HelperError>;

    /// `org.freedesktop.systemd1.Unit::FragmentPath` — the on-disk unit file
    /// for `unit_name`. `None` for transient units or when the fragment has
    /// been deleted from disk.
    async fn fragment_path(&self, unit_name: &str) -> Result<Option<String>, HelperError>;

    /// `systemctl is-enabled` view: `enabled` / `disabled` / `static` /
    /// `masked` / `not-found`. Surfaced as a string because the systemd
    /// vocabulary is itself open-ended; consumers branch on the canonical
    /// values they care about.
    async fn unit_file_state(&self, unit_name: &str) -> Result<Option<String>, HelperError>;
}

#[proxy(
    interface = "org.freedesktop.systemd1.Manager",
    default_service = "org.freedesktop.systemd1",
    default_path = "/org/freedesktop/systemd1"
)]
trait SystemdManager {
    fn get_unit(&self, name: &str) -> zbus::Result<zbus::zvariant::OwnedObjectPath>;
    // Plan #3 ended up using Reload (daemon-reload) instead of LoadUnit for
    // the install path; LoadUnit stays here pre-declared in case a future
    // plan needs single-unit re-parse without a full daemon-reload.
    #[allow(dead_code)]
    fn load_unit(&self, name: &str) -> zbus::Result<zbus::zvariant::OwnedObjectPath>;

    /// `mode` is one of "replace" / "fail" / "isolate" / "ignore-dependencies"
    /// / "ignore-requirements". We always pass "replace".
    fn start_unit(&self, name: &str, mode: &str) -> zbus::Result<zbus::zvariant::OwnedObjectPath>;
    fn stop_unit(&self, name: &str, mode: &str) -> zbus::Result<zbus::zvariant::OwnedObjectPath>;
    fn restart_unit(&self, name: &str, mode: &str)
        -> zbus::Result<zbus::zvariant::OwnedObjectPath>;

    /// Returns `(carries_install_info, changes)`. We ignore the changes vec
    /// — systemd has already applied them; we just want the success/error
    /// surface. `runtime=false` writes to `/etc/systemd/system/`; `force=true`
    /// overwrites pre-existing symlinks.
    #[zbus(name = "EnableUnitFiles")]
    fn enable_unit_files(
        &self,
        files: &[&str],
        runtime: bool,
        force: bool,
    ) -> zbus::Result<(bool, Vec<(String, String, String)>)>;

    #[zbus(name = "DisableUnitFiles")]
    fn disable_unit_files(
        &self,
        files: &[&str],
        runtime: bool,
    ) -> zbus::Result<Vec<(String, String, String)>>;

    fn reload(&self) -> zbus::Result<()>;

    fn get_unit_file_state(&self, name: &str) -> zbus::Result<String>;
}

#[proxy(interface = "org.freedesktop.systemd1.Unit")]
trait SystemdUnit {
    #[zbus(property)]
    fn active_state(&self) -> zbus::Result<String>;
    #[zbus(property)]
    fn sub_state(&self) -> zbus::Result<String>;
    #[zbus(property)]
    fn load_state(&self) -> zbus::Result<String>;
}

#[proxy(interface = "org.freedesktop.systemd1.Service")]
trait SystemdService {
    #[zbus(property)]
    fn n_restarts(&self) -> zbus::Result<u32>;
    #[zbus(property)]
    fn exec_main_status(&self) -> zbus::Result<i32>;
}

pub struct DBusSystemd {
    conn: Connection,
}

impl DBusSystemd {
    pub fn new(conn: Connection) -> Self {
        Self { conn }
    }
}

#[async_trait]
impl Systemd for DBusSystemd {
    async fn unit_state(&self, unit_name: &str) -> Result<UnitState, HelperError> {
        let mgr = SystemdManagerProxy::new(&self.conn)
            .await
            .map_err(|e| HelperError::Systemd {
                message: format!("manager proxy: {e}"),
            })?;

        // GetUnit returns NoSuchUnit for unloaded units. We translate that
        // into UnitState::NotFound rather than bubbling up an error so the
        // GUI can render "service not installed yet" cleanly.
        let unit_path = match mgr.get_unit(unit_name).await {
            Ok(p) => p,
            Err(zbus::Error::MethodError(name, _, _))
                if name.as_str() == "org.freedesktop.systemd1.NoSuchUnit" =>
            {
                return Ok(UnitState::NotFound);
            }
            Err(e) => {
                return Err(HelperError::Systemd {
                    message: format!("GetUnit: {e}"),
                })
            }
        };

        let unit = SystemdUnitProxy::builder(&self.conn)
            .destination("org.freedesktop.systemd1")
            .map_err(|e| HelperError::Systemd {
                message: e.to_string(),
            })?
            .path(unit_path.clone())
            .map_err(|e| HelperError::Systemd {
                message: e.to_string(),
            })?
            .build()
            .await
            .map_err(|e| HelperError::Systemd {
                message: e.to_string(),
            })?;
        let active_state = unit.active_state().await.map_err(systemd_err)?;
        let sub_state = unit.sub_state().await.map_err(systemd_err)?;
        let load_state = unit.load_state().await.map_err(systemd_err)?;

        let svc = SystemdServiceProxy::builder(&self.conn)
            .destination("org.freedesktop.systemd1")
            .map_err(|e| HelperError::Systemd {
                message: e.to_string(),
            })?
            .path(unit_path)
            .map_err(|e| HelperError::Systemd {
                message: e.to_string(),
            })?
            .build()
            .await
            .map_err(|e| HelperError::Systemd {
                message: e.to_string(),
            })?;
        // For non-Service units these properties may be absent — surface 0
        // rather than failing the whole query.
        let n_restarts = svc.n_restarts().await.unwrap_or(0);
        let exec_main_status = svc.exec_main_status().await.unwrap_or(0);

        Ok(UnitState::Known {
            active_state,
            sub_state,
            load_state,
            n_restarts,
            exec_main_status,
        })
    }

    async fn start_unit(&self, unit_name: &str) -> Result<(), HelperError> {
        let mgr = SystemdManagerProxy::new(&self.conn)
            .await
            .map_err(systemd_err)?;
        mgr.start_unit(unit_name, "replace")
            .await
            .map(|_| ())
            .map_err(systemd_err)
    }

    async fn stop_unit(&self, unit_name: &str) -> Result<(), HelperError> {
        let mgr = SystemdManagerProxy::new(&self.conn)
            .await
            .map_err(systemd_err)?;
        mgr.stop_unit(unit_name, "replace")
            .await
            .map(|_| ())
            .map_err(systemd_err)
    }

    async fn restart_unit(&self, unit_name: &str) -> Result<(), HelperError> {
        let mgr = SystemdManagerProxy::new(&self.conn)
            .await
            .map_err(systemd_err)?;
        mgr.restart_unit(unit_name, "replace")
            .await
            .map(|_| ())
            .map_err(systemd_err)
    }

    async fn enable_unit_files(&self, unit_names: &[String]) -> Result<(), HelperError> {
        let mgr = SystemdManagerProxy::new(&self.conn)
            .await
            .map_err(systemd_err)?;
        let refs: Vec<&str> = unit_names.iter().map(|s| s.as_str()).collect();
        mgr.enable_unit_files(&refs, false, true)
            .await
            .map(|_| ())
            .map_err(systemd_err)
    }

    async fn disable_unit_files(&self, unit_names: &[String]) -> Result<(), HelperError> {
        let mgr = SystemdManagerProxy::new(&self.conn)
            .await
            .map_err(systemd_err)?;
        let refs: Vec<&str> = unit_names.iter().map(|s| s.as_str()).collect();
        mgr.disable_unit_files(&refs, false)
            .await
            .map(|_| ())
            .map_err(systemd_err)
    }

    async fn reload(&self) -> Result<(), HelperError> {
        let mgr = SystemdManagerProxy::new(&self.conn)
            .await
            .map_err(systemd_err)?;
        mgr.reload().await.map_err(systemd_err)
    }

    async fn fragment_path(&self, unit_name: &str) -> Result<Option<String>, HelperError> {
        let mgr = SystemdManagerProxy::new(&self.conn)
            .await
            .map_err(systemd_err)?;
        let unit_path = match mgr.get_unit(unit_name).await {
            Ok(p) => p,
            Err(zbus::Error::MethodError(name, _, _))
                if name.as_str() == "org.freedesktop.systemd1.NoSuchUnit" =>
            {
                return Ok(None);
            }
            Err(e) => return Err(systemd_err(e)),
        };
        // FragmentPath lives on the Unit interface; we read it via a generic
        // Properties.Get so we don't have to add yet another typed property.
        let props = zbus::fdo::PropertiesProxy::builder(&self.conn)
            .destination("org.freedesktop.systemd1")
            .map_err(systemd_err)?
            .path(unit_path)
            .map_err(systemd_err)?
            .build()
            .await
            .map_err(systemd_err)?;
        let iface =
            zbus::names::InterfaceName::from_static_str_unchecked("org.freedesktop.systemd1.Unit");
        let v = props
            .get(iface, "FragmentPath")
            .await
            .map_err(|e| HelperError::Systemd {
                message: format!("FragmentPath: {e}"),
            })?;
        let s: String = v.try_into().map_err(|e| HelperError::Systemd {
            message: format!("FragmentPath decode: {e}"),
        })?;
        if s.is_empty() {
            Ok(None)
        } else {
            Ok(Some(s))
        }
    }

    async fn unit_file_state(&self, unit_name: &str) -> Result<Option<String>, HelperError> {
        let mgr = SystemdManagerProxy::new(&self.conn)
            .await
            .map_err(systemd_err)?;
        match mgr.get_unit_file_state(unit_name).await {
            Ok(s) => Ok(Some(s)),
            Err(zbus::Error::MethodError(_, _, _)) => Ok(None),
            Err(e) => Err(systemd_err(e)),
        }
    }
}

fn systemd_err(e: zbus::Error) -> HelperError {
    HelperError::Systemd {
        message: e.to_string(),
    }
}

#[async_trait]
pub trait JournalReader: Send + Sync {
    /// Return the last `lines` journal entries for `unit_name`. Caller is
    /// responsible for clamping `lines` to a sane upper bound; this trait
    /// passes through whatever it gets.
    async fn tail(&self, unit_name: &str, lines: u32) -> Result<Vec<String>, HelperError>;
}

pub struct JournalctlProcess;

#[async_trait]
impl JournalReader for JournalctlProcess {
    async fn tail(&self, unit_name: &str, lines: u32) -> Result<Vec<String>, HelperError> {
        let n_str = lines.to_string();
        let out = tokio::process::Command::new("journalctl")
            .arg("--no-pager")
            .arg("-u")
            .arg(unit_name)
            .arg("-n")
            .arg(&n_str)
            // --output=short keeps the format `Apr 28 12:34:56 host unit[pid]: msg`
            // which is what `journalctl` defaults to anyway, but pinning it makes
            // the format stable across distros that change defaults.
            .arg("--output=short")
            .output()
            .await
            .map_err(|e| HelperError::Ipc {
                message: format!("spawn journalctl: {e}"),
            })?;
        if !out.status.success() {
            return Err(HelperError::Ipc {
                message: format!(
                    "journalctl exit {:?}: {}",
                    out.status.code(),
                    String::from_utf8_lossy(&out.stderr).trim()
                ),
            });
        }
        let text = String::from_utf8_lossy(&out.stdout);
        Ok(text.lines().map(|l| l.to_string()).collect())
    }
}

#[cfg(test)]
pub mod testing {
    use super::*;
    use std::sync::Mutex;

    pub struct FixedSystemd {
        pub answer: UnitState,
        pub fragment_path: Option<String>,
        pub unit_file_state: Option<String>,
    }

    impl FixedSystemd {
        pub fn new_with_fragment(
            answer: UnitState,
            fragment_path: Option<String>,
            unit_file_state: Option<String>,
        ) -> Self {
            Self {
                answer,
                fragment_path,
                unit_file_state,
            }
        }
    }

    #[async_trait]
    impl Systemd for FixedSystemd {
        async fn unit_state(&self, _: &str) -> Result<UnitState, HelperError> {
            Ok(self.answer.clone())
        }
        async fn start_unit(&self, _: &str) -> Result<(), HelperError> {
            Ok(())
        }
        async fn stop_unit(&self, _: &str) -> Result<(), HelperError> {
            Ok(())
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
            Ok(self.fragment_path.clone())
        }
        async fn unit_file_state(&self, _: &str) -> Result<Option<String>, HelperError> {
            Ok(self.unit_file_state.clone())
        }
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    pub enum RecordedCall {
        StartUnit(String),
        StopUnit(String),
        RestartUnit(String),
        EnableUnitFiles(Vec<String>),
        DisableUnitFiles(Vec<String>),
        Reload,
    }

    pub struct RecordingSystemd {
        pub answer: UnitState,
        pub fragment_path: Mutex<Option<String>>,
        pub unit_file_state: Mutex<Option<String>>,
        pub calls: Mutex<Vec<RecordedCall>>,
    }

    impl RecordingSystemd {
        pub fn new(answer: UnitState) -> Self {
            Self {
                answer,
                fragment_path: Mutex::new(None),
                unit_file_state: Mutex::new(None),
                calls: Mutex::new(Vec::new()),
            }
        }
        pub fn set_fragment_path(&self, path: Option<String>) {
            *self.fragment_path.lock().unwrap() = path;
        }
        #[allow(dead_code)] // wired by future plans (test-only fixture setter)
        pub fn set_unit_file_state(&self, state: Option<String>) {
            *self.unit_file_state.lock().unwrap() = state;
        }
        pub fn calls(&self) -> Vec<RecordedCall> {
            self.calls.lock().unwrap().clone()
        }
    }

    #[async_trait]
    impl Systemd for RecordingSystemd {
        async fn unit_state(&self, _: &str) -> Result<UnitState, HelperError> {
            Ok(self.answer.clone())
        }
        async fn start_unit(&self, name: &str) -> Result<(), HelperError> {
            self.calls
                .lock()
                .unwrap()
                .push(RecordedCall::StartUnit(name.into()));
            Ok(())
        }
        async fn stop_unit(&self, name: &str) -> Result<(), HelperError> {
            self.calls
                .lock()
                .unwrap()
                .push(RecordedCall::StopUnit(name.into()));
            Ok(())
        }
        async fn restart_unit(&self, name: &str) -> Result<(), HelperError> {
            self.calls
                .lock()
                .unwrap()
                .push(RecordedCall::RestartUnit(name.into()));
            Ok(())
        }
        async fn enable_unit_files(&self, names: &[String]) -> Result<(), HelperError> {
            self.calls
                .lock()
                .unwrap()
                .push(RecordedCall::EnableUnitFiles(names.to_vec()));
            Ok(())
        }
        async fn disable_unit_files(&self, names: &[String]) -> Result<(), HelperError> {
            self.calls
                .lock()
                .unwrap()
                .push(RecordedCall::DisableUnitFiles(names.to_vec()));
            Ok(())
        }
        async fn reload(&self) -> Result<(), HelperError> {
            self.calls.lock().unwrap().push(RecordedCall::Reload);
            Ok(())
        }
        async fn fragment_path(&self, _: &str) -> Result<Option<String>, HelperError> {
            Ok(self.fragment_path.lock().unwrap().clone())
        }
        async fn unit_file_state(&self, _: &str) -> Result<Option<String>, HelperError> {
            Ok(self.unit_file_state.lock().unwrap().clone())
        }
    }

    #[tokio::test]
    async fn fixed_returns_canned_state() {
        let q = FixedSystemd {
            answer: UnitState::NotFound,
            fragment_path: None,
            unit_file_state: None,
        };
        assert_eq!(q.unit_state("anything").await.unwrap(), UnitState::NotFound);
    }

    #[tokio::test]
    async fn recording_systemd_records_start_unit() {
        let s = RecordingSystemd::new(UnitState::NotFound);
        s.start_unit("boxpilot-sing-box.service").await.unwrap();
        assert_eq!(
            s.calls(),
            vec![RecordedCall::StartUnit("boxpilot-sing-box.service".into())]
        );
    }

    #[tokio::test]
    async fn fixed_systemd_returns_canned_fragment_path() {
        let q = FixedSystemd::new_with_fragment(
            UnitState::NotFound,
            Some("/etc/systemd/system/sing-box.service".into()),
            Some("enabled".into()),
        );
        assert_eq!(
            q.fragment_path("sing-box.service").await.unwrap(),
            Some("/etc/systemd/system/sing-box.service".into())
        );
        assert_eq!(
            q.unit_file_state("sing-box.service").await.unwrap(),
            Some("enabled".into())
        );
    }

    #[tokio::test]
    async fn fixed_systemd_default_constructor_keeps_fragment_unset() {
        // Existing tests construct FixedSystemd { answer: ... } directly;
        // check that style still works and returns None for the new methods.
        let q = FixedSystemd {
            answer: UnitState::NotFound,
            fragment_path: None,
            unit_file_state: None,
        };
        assert!(q.fragment_path("u").await.unwrap().is_none());
        assert!(q.unit_file_state("u").await.unwrap().is_none());
    }

    pub struct FixedJournal {
        pub lines: Vec<String>,
    }

    #[async_trait]
    impl JournalReader for FixedJournal {
        async fn tail(&self, _: &str, _: u32) -> Result<Vec<String>, HelperError> {
            Ok(self.lines.clone())
        }
    }

    #[tokio::test]
    async fn fixed_journal_returns_canned_lines() {
        let j = FixedJournal {
            lines: vec!["a".into(), "b".into()],
        };
        assert_eq!(j.tail("u", 10).await.unwrap(), vec!["a", "b"]);
    }
}
