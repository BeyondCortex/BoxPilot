//! `service.{start,stop,restart,enable,disable}` (§6.3). One D-Bus call
//! to systemd, then a `unit_state` query so the response carries the
//! post-op state. The lock and authorization were already taken in
//! `dispatch::authorize`; this module is a thin wrapper.

use crate::systemd::Systemd;
use boxpilot_ipc::{HelperResult, ServiceControlResponse};

#[derive(Debug, Clone, Copy)]
pub enum Verb {
    Start,
    Stop,
    Restart,
    Enable,
    Disable,
}

pub async fn run(
    verb: Verb,
    unit_name: &str,
    systemd: &dyn Systemd,
) -> HelperResult<ServiceControlResponse> {
    match verb {
        Verb::Start => systemd.start_unit(unit_name).await?,
        Verb::Stop => systemd.stop_unit(unit_name).await?,
        Verb::Restart => systemd.restart_unit(unit_name).await?,
        Verb::Enable => systemd.enable_unit_files(&[unit_name.to_string()]).await?,
        Verb::Disable => systemd.disable_unit_files(&[unit_name.to_string()]).await?,
    }
    let unit_state = systemd.unit_state(unit_name).await?;
    Ok(ServiceControlResponse { unit_state })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::systemd::testing::{RecordedCall, RecordingSystemd};
    use boxpilot_ipc::UnitState;

    #[tokio::test]
    async fn start_invokes_start_unit_and_returns_state() {
        let s = RecordingSystemd::new(UnitState::Known {
            active_state: "active".into(),
            sub_state: "running".into(),
            load_state: "loaded".into(),
            n_restarts: 0,
            exec_main_status: 0,
        });
        let resp = run(Verb::Start, "boxpilot-sing-box.service", &s)
            .await
            .unwrap();
        assert!(matches!(resp.unit_state, UnitState::Known { .. }));
        assert_eq!(
            s.calls(),
            vec![RecordedCall::StartUnit("boxpilot-sing-box.service".into())]
        );
    }

    #[tokio::test]
    async fn enable_invokes_enable_unit_files() {
        let s = RecordingSystemd::new(UnitState::NotFound);
        run(Verb::Enable, "boxpilot-sing-box.service", &s)
            .await
            .unwrap();
        assert_eq!(
            s.calls(),
            vec![RecordedCall::EnableUnitFiles(vec![
                "boxpilot-sing-box.service".into()
            ])]
        );
    }

    #[tokio::test]
    async fn disable_invokes_disable_unit_files() {
        let s = RecordingSystemd::new(UnitState::NotFound);
        run(Verb::Disable, "boxpilot-sing-box.service", &s)
            .await
            .unwrap();
        assert_eq!(
            s.calls(),
            vec![RecordedCall::DisableUnitFiles(vec![
                "boxpilot-sing-box.service".into()
            ])]
        );
    }

    #[tokio::test]
    async fn restart_invokes_restart_unit() {
        let s = RecordingSystemd::new(UnitState::NotFound);
        run(Verb::Restart, "boxpilot-sing-box.service", &s)
            .await
            .unwrap();
        assert_eq!(
            s.calls(),
            vec![RecordedCall::RestartUnit(
                "boxpilot-sing-box.service".into()
            )]
        );
    }

    #[tokio::test]
    async fn stop_invokes_stop_unit() {
        let s = RecordingSystemd::new(UnitState::NotFound);
        run(Verb::Stop, "boxpilot-sing-box.service", &s)
            .await
            .unwrap();
        assert_eq!(
            s.calls(),
            vec![RecordedCall::StopUnit("boxpilot-sing-box.service".into())]
        );
    }
}
