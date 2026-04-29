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
}

#[proxy(
    interface = "org.freedesktop.systemd1.Manager",
    default_service = "org.freedesktop.systemd1",
    default_path = "/org/freedesktop/systemd1"
)]
trait SystemdManager {
    fn get_unit(&self, name: &str) -> zbus::Result<zbus::zvariant::OwnedObjectPath>;
    // Reserved for plan #3's `service.install_managed` flow, which calls
    // LoadUnit to ensure systemd has parsed the freshly-written .service file
    // before issuing StartUnit.
    #[allow(dead_code)]
    fn load_unit(&self, name: &str) -> zbus::Result<zbus::zvariant::OwnedObjectPath>;

    /// `mode` is one of "replace" / "fail" / "isolate" / "ignore-dependencies"
    /// / "ignore-requirements". We always pass "replace".
    fn start_unit(&self, name: &str, mode: &str) -> zbus::Result<zbus::zvariant::OwnedObjectPath>;
    fn stop_unit(&self, name: &str, mode: &str) -> zbus::Result<zbus::zvariant::OwnedObjectPath>;
    fn restart_unit(&self, name: &str, mode: &str) -> zbus::Result<zbus::zvariant::OwnedObjectPath>;

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
}

fn systemd_err(e: zbus::Error) -> HelperError {
    HelperError::Systemd {
        message: e.to_string(),
    }
}

#[cfg(test)]
pub mod testing {
    use super::*;
    use std::sync::Mutex;

    pub struct FixedSystemd {
        pub answer: UnitState,
    }

    #[async_trait]
    impl Systemd for FixedSystemd {
        async fn unit_state(&self, _: &str) -> Result<UnitState, HelperError> {
            Ok(self.answer.clone())
        }
        async fn start_unit(&self, _: &str) -> Result<(), HelperError> { Ok(()) }
        async fn stop_unit(&self, _: &str) -> Result<(), HelperError> { Ok(()) }
        async fn restart_unit(&self, _: &str) -> Result<(), HelperError> { Ok(()) }
        async fn enable_unit_files(&self, _: &[String]) -> Result<(), HelperError> { Ok(()) }
        async fn disable_unit_files(&self, _: &[String]) -> Result<(), HelperError> { Ok(()) }
        async fn reload(&self) -> Result<(), HelperError> { Ok(()) }
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
        pub calls: Mutex<Vec<RecordedCall>>,
    }

    impl RecordingSystemd {
        pub fn new(answer: UnitState) -> Self {
            Self {
                answer,
                calls: Mutex::new(Vec::new()),
            }
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
            self.calls.lock().unwrap().push(RecordedCall::StartUnit(name.into()));
            Ok(())
        }
        async fn stop_unit(&self, name: &str) -> Result<(), HelperError> {
            self.calls.lock().unwrap().push(RecordedCall::StopUnit(name.into()));
            Ok(())
        }
        async fn restart_unit(&self, name: &str) -> Result<(), HelperError> {
            self.calls.lock().unwrap().push(RecordedCall::RestartUnit(name.into()));
            Ok(())
        }
        async fn enable_unit_files(&self, names: &[String]) -> Result<(), HelperError> {
            self.calls.lock().unwrap().push(RecordedCall::EnableUnitFiles(names.to_vec()));
            Ok(())
        }
        async fn disable_unit_files(&self, names: &[String]) -> Result<(), HelperError> {
            self.calls.lock().unwrap().push(RecordedCall::DisableUnitFiles(names.to_vec()));
            Ok(())
        }
        async fn reload(&self) -> Result<(), HelperError> {
            self.calls.lock().unwrap().push(RecordedCall::Reload);
            Ok(())
        }
    }

    #[tokio::test]
    async fn fixed_returns_canned_state() {
        let q = FixedSystemd {
            answer: UnitState::NotFound,
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
}
