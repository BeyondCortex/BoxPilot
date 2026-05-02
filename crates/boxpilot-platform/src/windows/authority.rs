//! Windows Authority: AlwaysAllow with a startup warn (per spec COQ3).
//! Real SID-based authorization arrives in Sub-project #2 alongside the
//! `controller_principal` schema bump.

use crate::traits::authority::{Authority, CallerPrincipal};
use async_trait::async_trait;
use boxpilot_ipc::HelperError;

pub struct AlwaysAllowAuthority;

impl AlwaysAllowAuthority {
    /// Constructor that emits a single tracing warn line. Caller should
    /// invoke this once at boxpilotd startup so the warn appears at the
    /// top of the `tracing-appender` log file (per spec §6, COQ5).
    pub fn new_with_warn() -> Self {
        tracing::warn!(
            "windows authority is in pass-through mode pending sub-project #2 — \
             do not run on a multi-user machine"
        );
        Self
    }
}

#[async_trait]
impl Authority for AlwaysAllowAuthority {
    async fn check(
        &self,
        _action_id: &str,
        _principal: &CallerPrincipal,
    ) -> Result<bool, HelperError> {
        Ok(true)
    }
}
