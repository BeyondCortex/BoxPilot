//! Linux `ServiceManager` impl backed by systemd's D-Bus interface.
//! Verbatim port from `boxpilotd::systemd` — zbus proxy macros and method
//! bodies are unchanged.
//!
//! systemd query and control layer.  Read access (`GetUnit` + `Properties.Get`)
//! is unauthenticated on the system bus when the daemon runs as root.

use crate::traits::service::ServiceManager;
use async_trait::async_trait;
use boxpilot_ipc::{HelperError, UnitState};
use zbus::{proxy, Connection};

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
impl ServiceManager for DBusSystemd {
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
            platform_extra: boxpilot_ipc::PlatformUnitExtra::Linux,
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
