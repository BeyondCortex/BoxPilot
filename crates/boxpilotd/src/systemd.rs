//! systemd query layer. We only need read access in this plan
//! (`Manager.GetUnit` + `Properties.Get`), which is unauthenticated on the
//! system bus when the daemon runs as root. Service-control verbs come in
//! plan #3.

use async_trait::async_trait;
use boxpilot_ipc::{HelperError, UnitState};
use zbus::{proxy, Connection};

#[async_trait]
pub trait SystemdQuery: Send + Sync {
    async fn unit_state(&self, unit_name: &str) -> Result<UnitState, HelperError>;
}

#[proxy(
    interface = "org.freedesktop.systemd1.Manager",
    default_service = "org.freedesktop.systemd1",
    default_path = "/org/freedesktop/systemd1"
)]
trait SystemdManager {
    fn get_unit(&self, name: &str) -> zbus::Result<zbus::zvariant::OwnedObjectPath>;
    fn load_unit(&self, name: &str) -> zbus::Result<zbus::zvariant::OwnedObjectPath>;
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
impl SystemdQuery for DBusSystemd {
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
}

fn systemd_err(e: zbus::Error) -> HelperError {
    HelperError::Systemd {
        message: e.to_string(),
    }
}

#[cfg(test)]
pub mod testing {
    use super::*;

    pub struct FixedSystemd {
        pub answer: UnitState,
    }

    #[async_trait]
    impl SystemdQuery for FixedSystemd {
        async fn unit_state(&self, _unit_name: &str) -> Result<UnitState, HelperError> {
            Ok(self.answer.clone())
        }
    }

    #[tokio::test]
    async fn fixed_returns_canned_state() {
        let q = FixedSystemd {
            answer: UnitState::NotFound,
        };
        assert_eq!(q.unit_state("anything").await.unwrap(), UnitState::NotFound);
    }
}
