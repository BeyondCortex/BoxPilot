//! Caller principal + Authority decision. The principal is platform-tagged
//! so dispatch (in `boxpilotd::dispatch`) can stay platform-neutral.
//!
//! Linux principal: kernel uid resolved via `GetConnectionUnixUser` over
//! D-Bus.
//! Windows principal: SID resolved via `GetNamedPipeClientProcessId` +
//! `OpenProcessToken` + `GetTokenInformation(TokenUser)` (real impl in PR 12).
//!
//! `Authority::check` is invoked AFTER the IpcServer resolves the principal.
//! Polkit (Linux) takes a D-Bus sender bus name string as the subject; the
//! Linux Authority impl carries an internal `(uid, sender)` pair when
//! constructed for a specific call so it can pass `sender` to polkit while
//! presenting `principal` to dispatch.

use async_trait::async_trait;
use boxpilot_ipc::HelperError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CallerPrincipal {
    LinuxUid(u32),
    WindowsSid(String),
}

impl CallerPrincipal {
    pub fn linux_uid(&self) -> Option<u32> {
        if let CallerPrincipal::LinuxUid(u) = self {
            Some(*u)
        } else {
            None
        }
    }
}

#[async_trait]
pub trait Authority: Send + Sync {
    async fn check(
        &self,
        action_id: &str,
        principal: &CallerPrincipal,
    ) -> Result<bool, HelperError>;
}
