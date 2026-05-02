//! Test doubles for `ServiceManager`. Moved verbatim from
//! `boxpilotd::systemd::testing` so call sites can keep importing them by
//! name through the re-export shell.

use crate::traits::service::ServiceManager;
use async_trait::async_trait;
use boxpilot_ipc::{HelperError, UnitState};
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
impl ServiceManager for FixedSystemd {
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
impl ServiceManager for RecordingSystemd {
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
