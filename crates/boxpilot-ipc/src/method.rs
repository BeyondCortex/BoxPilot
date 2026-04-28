use serde::{Deserialize, Serialize};

/// Logical action identifiers from spec §6.3. Wire form uses underscores
/// (`service.install_managed`); polkit action IDs use dashes — see
/// [`polkit_action_id`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum HelperMethod {
    #[serde(rename = "service.status")]              ServiceStatus,
    #[serde(rename = "service.start")]               ServiceStart,
    #[serde(rename = "service.stop")]                ServiceStop,
    #[serde(rename = "service.restart")]             ServiceRestart,
    #[serde(rename = "service.enable")]              ServiceEnable,
    #[serde(rename = "service.disable")]             ServiceDisable,
    #[serde(rename = "service.install_managed")]     ServiceInstallManaged,
    #[serde(rename = "service.logs")]                ServiceLogs,
    #[serde(rename = "profile.activate_bundle")]     ProfileActivateBundle,
    #[serde(rename = "profile.rollback_release")]    ProfileRollbackRelease,
    #[serde(rename = "core.discover")]               CoreDiscover,
    #[serde(rename = "core.install_managed")]        CoreInstallManaged,
    #[serde(rename = "core.upgrade_managed")]        CoreUpgradeManaged,
    #[serde(rename = "core.rollback_managed")]       CoreRollbackManaged,
    #[serde(rename = "core.adopt")]                  CoreAdopt,
    #[serde(rename = "legacy.observe_service")]      LegacyObserveService,
    #[serde(rename = "legacy.migrate_service")]      LegacyMigrateService,
    #[serde(rename = "controller.transfer")]         ControllerTransfer,
    #[serde(rename = "diagnostics.export_redacted")] DiagnosticsExportRedacted,
}

impl HelperMethod {
    pub const ALL: [HelperMethod; 19] = [
        HelperMethod::ServiceStatus,
        HelperMethod::ServiceStart,
        HelperMethod::ServiceStop,
        HelperMethod::ServiceRestart,
        HelperMethod::ServiceEnable,
        HelperMethod::ServiceDisable,
        HelperMethod::ServiceInstallManaged,
        HelperMethod::ServiceLogs,
        HelperMethod::ProfileActivateBundle,
        HelperMethod::ProfileRollbackRelease,
        HelperMethod::CoreDiscover,
        HelperMethod::CoreInstallManaged,
        HelperMethod::CoreUpgradeManaged,
        HelperMethod::CoreRollbackManaged,
        HelperMethod::CoreAdopt,
        HelperMethod::LegacyObserveService,
        HelperMethod::LegacyMigrateService,
        HelperMethod::ControllerTransfer,
        HelperMethod::DiagnosticsExportRedacted,
    ];

    pub fn as_logical(&self) -> &'static str {
        // Round-trip via serde to keep the source of truth in one place.
        // SAFETY: enum values always serialize to a JSON string; unwrap is fine.
        let v = serde_json::to_value(self).unwrap();
        Box::leak(v.as_str().unwrap().to_owned().into_boxed_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn count_matches_spec() {
        // Spec §6.3 lists 18 mutating/observing actions plus controller.transfer
        // and diagnostics.export_redacted — 19 total when we count
        // legacy.observe_service as observe. Keep this number in sync if
        // §6.3 ever changes.
        assert_eq!(HelperMethod::ALL.len(), 19);
    }

    #[test]
    fn known_action_round_trips() {
        let m = HelperMethod::ServiceStatus;
        let s = serde_json::to_string(&m).unwrap();
        assert_eq!(s, "\"service.status\"");
        let back: HelperMethod = serde_json::from_str(&s).unwrap();
        assert_eq!(back, m);
    }

    #[test]
    fn underscore_variants_use_underscores_on_wire() {
        let s = serde_json::to_string(&HelperMethod::ProfileActivateBundle).unwrap();
        assert_eq!(s, "\"profile.activate_bundle\"");
    }

    #[test]
    fn unknown_action_fails_to_deserialize() {
        let r: Result<HelperMethod, _> = serde_json::from_str("\"service.nuke\"");
        assert!(r.is_err());
    }
}

/// Authorization class per spec §6.3.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthClass {
    /// Read-only status: `auth_self_keep` for controller, `yes` for non-controllers.
    ReadOnly,
    /// Mutating: `auth_admin_keep` for non-controllers, `auth_self_keep` for controller.
    Mutating,
    /// High-risk: always `auth_admin` (no caching).
    HighRisk,
}

impl HelperMethod {
    pub fn auth_class(&self) -> AuthClass {
        use HelperMethod::*;
        match self {
            ServiceStatus | ServiceLogs | CoreDiscover | LegacyObserveService => {
                AuthClass::ReadOnly
            }
            ControllerTransfer | LegacyMigrateService => AuthClass::HighRisk,
            _ => AuthClass::Mutating,
        }
    }

    pub fn is_mutating(&self) -> bool {
        !matches!(self.auth_class(), AuthClass::ReadOnly)
    }

    /// `app.boxpilot.helper.<dotted-with-dashes>`
    pub fn polkit_action_id(&self) -> String {
        let logical = self.as_logical(); // e.g. "profile.activate_bundle"
        let dashed = logical.replace('_', "-"); // "profile.activate-bundle"
        format!("app.boxpilot.helper.{dashed}")
    }
}

#[cfg(test)]
mod auth_tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn read_only_classifications() {
        assert_eq!(HelperMethod::ServiceStatus.auth_class(), AuthClass::ReadOnly);
        assert_eq!(HelperMethod::ServiceLogs.auth_class(), AuthClass::ReadOnly);
        assert_eq!(HelperMethod::CoreDiscover.auth_class(), AuthClass::ReadOnly);
        assert_eq!(HelperMethod::LegacyObserveService.auth_class(), AuthClass::ReadOnly);
    }

    #[test]
    fn high_risk_classifications() {
        assert_eq!(HelperMethod::ControllerTransfer.auth_class(), AuthClass::HighRisk);
        assert_eq!(HelperMethod::LegacyMigrateService.auth_class(), AuthClass::HighRisk);
    }

    #[test]
    fn mutating_default() {
        assert_eq!(HelperMethod::ServiceStart.auth_class(), AuthClass::Mutating);
        assert_eq!(HelperMethod::ProfileActivateBundle.auth_class(), AuthClass::Mutating);
        assert_eq!(HelperMethod::CoreInstallManaged.auth_class(), AuthClass::Mutating);
    }

    #[test]
    fn polkit_action_id_uses_dashes_not_underscores() {
        assert_eq!(
            HelperMethod::ProfileActivateBundle.polkit_action_id(),
            "app.boxpilot.helper.profile.activate-bundle"
        );
        assert_eq!(
            HelperMethod::ServiceStatus.polkit_action_id(),
            "app.boxpilot.helper.service.status"
        );
        assert_eq!(
            HelperMethod::CoreInstallManaged.polkit_action_id(),
            "app.boxpilot.helper.core.install-managed"
        );
    }

    #[test]
    fn every_action_has_a_polkit_id() {
        for m in HelperMethod::ALL {
            let id = m.polkit_action_id();
            assert!(id.starts_with("app.boxpilot.helper."));
            assert!(!id.contains('_'), "polkit IDs use dashes, got {id}");
        }
    }
}
