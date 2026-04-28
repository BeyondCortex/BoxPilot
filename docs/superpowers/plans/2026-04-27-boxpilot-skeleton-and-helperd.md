# BoxPilot Skeleton & `boxpilotd` Scaffolding Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the foundational Rust workspace, Tauri 2 + Vue 3 GUI shell, and root-owned `boxpilotd` D-Bus helper, so that the unprivileged GUI can perform a polkit-authorized round-trip to query systemd state. Controller-UID model, global advisory lock, and the §6.3 action whitelist must be enforced in this plan even though the only end-to-end action wired through is read-only (`service.status`).

**Architecture:** Cargo workspace with three crates: `boxpilot-ipc` (shared serde types, action classification, config schema), `boxpilotd` (root D-Bus daemon, system-bus activated, polkit-guarded), and `boxpilot-tauri` (Tauri 2 backend). Frontend is Vue 3 + TypeScript + Vite under `frontend/`. The daemon authenticates callers via D-Bus connection credentials (`org.freedesktop.DBus.GetConnectionUnixUser`) — never via request body — authorizes through polkit, and serializes mutating operations through `flock(2)` on `/run/boxpilot/lock`. All subsequent plans (managed core, activation, migration, etc.) consume the IPC contract and helper plumbing established here, so this plan locks down the contract surface for v1.0.

**Tech Stack:** Rust 2021, Tauri 2, Vue 3 + TypeScript + Vite, `zbus` 5 for D-Bus, `tokio` 1 (multi-thread), `serde` / `serde_json` / `toml`, `tracing` for logging, `fs2` for advisory file locks, `nix` for `User::from_uid`, polkit `org.freedesktop.PolicyKit1` D-Bus interface plus a JS rules file for controller-aware authorization.

**Worktree note:** Recommended to run this plan inside a git worktree (`git worktree add ../BoxPilot-skeleton skeleton`) so the executor can iterate without disturbing the spec branch. Not required.

**Out of scope for this plan (deliberate):**

- Real implementations of any helper method other than `service.status`. The other 17 methods from §6.3 are stubbed to return `NotImplemented`. They get bodies in plans #2–#9.
- Profile activation pipeline, file-descriptor passing, bundle unpacking — all in plan #5.
- Managed core install / upgrade / rollback / adopt — plan #2.
- Service unit generation, sandboxing directives, drift detection — plan #3.
- Diagnostics export, schema-aware redaction — plan #8.
- The full Home / Profiles / Settings UI — plan #7. This plan ships a single placeholder page that proves the helper round-trip.
- `.deb` postinst / prerm scripting — plan #9. This plan installs files via a dev `Makefile` for local iteration.

---

## File Structure

Workspace root `/home/connor-johnson/workspace/BoxPilot/`:

```text
Cargo.toml                                  # workspace manifest
rust-toolchain.toml                         # pin stable Rust
.gitignore
Makefile                                    # dev install/uninstall targets
crates/
  boxpilot-ipc/
    Cargo.toml
    src/
      lib.rs                                # re-exports
      method.rs                             # HelperMethod enum + action classification
      error.rs                              # HelperError enum, HelperResult
      response.rs                           # ServiceStatusResponse, UnitState, …
      config.rs                             # BoxpilotConfig, schema_version check
  boxpilotd/
    Cargo.toml
    src/
      main.rs                               # tokio entrypoint, D-Bus server bootstrap
      lock.rs                               # flock(2) on /run/boxpilot/lock
      controller.rs                         # ControllerState + claim/orphan logic
      credentials.rs                        # CallerIdentity, UnixUserResolver trait
      authority.rs                          # Authority trait + DBusAuthority + MockAuthority
      systemd.rs                            # SystemdQuery trait + DBusSystemd + MockSystemd
      context.rs                            # HelperContext composing the above
      dispatch.rs                           # authorize_call() — credentials → polkit → lock
      iface.rs                              # zbus #[interface] for app.boxpilot.Helper1
      paths.rs                              # canonical filesystem paths (overridable in tests)
  boxpilot-tauri/
    Cargo.toml
    tauri.conf.json
    build.rs
    src/
      lib.rs                                # tauri::Builder, command registration
      main.rs                               # binary entry → lib::run()
      helper_client.rs                      # zbus client calling app.boxpilot.Helper1
      commands.rs                           # #[tauri::command] wrappers
frontend/
  package.json
  tsconfig.json
  tsconfig.node.json
  vite.config.ts
  index.html
  src/
    main.ts
    App.vue
    api/
      helper.ts                             # invoke() wrappers around Tauri commands
      types.ts                              # TS mirrors of boxpilot-ipc response types
packaging/
  linux/
    dbus/
      system-services/app.boxpilot.Helper.service
      system.d/app.boxpilot.helper.conf
    polkit-1/
      actions/app.boxpilot.helper.policy
      rules.d/49-boxpilot.rules
docs/
  superpowers/
    specs/2026-04-27-boxpilot-linux-design.md
    plans/2026-04-27-boxpilot-skeleton-and-helperd.md   # this file
```

Files split by responsibility, not by layer. `boxpilot-ipc` is the only crate both `boxpilotd` and `boxpilot-tauri` depend on, so changes to wire types touch one place.

---

## Naming Contract (locked at the top, referenced by every later task)

Three name spaces show up in this plan; do not confuse them:

| Concept                        | Form                                       | Example                                       |
|--------------------------------|--------------------------------------------|-----------------------------------------------|
| **Logical action** (§6.3)      | dotted, underscores                        | `service.status`, `profile.activate_bundle`   |
| **D-Bus method** (zbus)        | CamelCase                                  | `ServiceStatus`, `ProfileActivateBundle`      |
| **polkit action ID**           | `app.boxpilot.helper.<dotted-with-dashes>` | `app.boxpilot.helper.profile.activate-bundle` |

The `HelperMethod` enum in `boxpilot-ipc::method` is the canonical mapping table.

D-Bus identifiers:

- Bus name (well-known, owned by `boxpilotd`): `app.boxpilot.Helper`
- Interface name: `app.boxpilot.Helper1`
- Object path: `/app/boxpilot/Helper`

Filesystem paths (all referenced via `boxpilotd::paths`, override-able in tests):

- `/etc/boxpilot/boxpilot.toml`
- `/etc/boxpilot/controller-uid` (plaintext integer, world-readable, used by polkit JS rule)
- `/run/boxpilot/lock`

---

## Task 1: Workspace skeleton

**Files:**
- Create: `Cargo.toml`
- Create: `rust-toolchain.toml`
- Create: `.gitignore`

- [ ] **Step 1: Write workspace `Cargo.toml`**

```toml
[workspace]
resolver = "2"
members = ["crates/boxpilot-ipc", "crates/boxpilotd", "crates/boxpilot-tauri"]

[workspace.package]
edition = "2021"
rust-version = "1.78"
license = "GPL-3.0-or-later"

[workspace.dependencies]
serde = { version = "1", features = ["derive"] }
serde_json = "1"
toml = "0.8"
thiserror = "1"
anyhow = "1"
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
tokio = { version = "1", features = ["macros", "rt-multi-thread", "signal", "fs", "sync"] }
zbus = { version = "5", default-features = false, features = ["tokio"] }
fs2 = "0.4"
nix = { version = "0.29", features = ["user", "fs"] }
tempfile = "3"
pretty_assertions = "1"

[profile.release]
lto = "thin"
codegen-units = 1
strip = "symbols"
```

- [ ] **Step 2: Write `rust-toolchain.toml`**

```toml
[toolchain]
channel = "stable"
components = ["rustfmt", "clippy"]
profile = "minimal"
```

- [ ] **Step 3: Write `.gitignore`**

```text
/target/
**/*.rs.bk
/frontend/node_modules/
/frontend/dist/
/crates/boxpilot-tauri/target/
.env
.DS_Store
*.log
```

- [ ] **Step 4: Verify workspace parses**

Run: `cargo metadata --no-deps --format-version 1 >/dev/null`
Expected: exit 0 (it succeeds even before crates exist; metadata reports `members: []` until task 2 adds them, then the empty-members case produces a warning but no error).

- [ ] **Step 5: Commit**

```bash
git add Cargo.toml rust-toolchain.toml .gitignore
git commit -m "chore: initialize workspace skeleton"
```

---

## Task 2: `boxpilot-ipc` crate stub

**Files:**
- Create: `crates/boxpilot-ipc/Cargo.toml`
- Create: `crates/boxpilot-ipc/src/lib.rs`

- [ ] **Step 1: Write the failing test**

`crates/boxpilot-ipc/src/lib.rs`:

```rust
#[cfg(test)]
mod tests {
    #[test]
    fn crate_compiles() {
        // Sentinel — replaced by real tests in subsequent tasks.
        assert_eq!(2 + 2, 4);
    }
}
```

`crates/boxpilot-ipc/Cargo.toml`:

```toml
[package]
name = "boxpilot-ipc"
version = "0.1.0"
edition.workspace = true
rust-version.workspace = true
license.workspace = true

[dependencies]
serde.workspace = true
serde_json.workspace = true
toml.workspace = true
thiserror.workspace = true

[dev-dependencies]
pretty_assertions.workspace = true
```

- [ ] **Step 2: Run the test**

Run: `cargo test -p boxpilot-ipc`
Expected: `test crate_compiles ... ok`, 1 passed.

- [ ] **Step 3: Commit**

```bash
git add crates/boxpilot-ipc
git commit -m "feat(ipc): create boxpilot-ipc crate skeleton"
```

---

## Task 3: `HelperMethod` enum + serde round-trip

**Files:**
- Create: `crates/boxpilot-ipc/src/method.rs`
- Modify: `crates/boxpilot-ipc/src/lib.rs`

- [ ] **Step 1: Write the failing test**

Append to `crates/boxpilot-ipc/src/method.rs`:

```rust
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
```

`crates/boxpilot-ipc/src/lib.rs`:

```rust
pub mod method;
pub use method::HelperMethod;

#[cfg(test)]
mod tests {
    #[test]
    fn crate_compiles() {
        assert_eq!(2 + 2, 4);
    }
}
```

- [ ] **Step 2: Run tests, expect them to fail**

Run: `cargo test -p boxpilot-ipc method`
Expected: compiler error (the test file doesn't exist as a sibling to lib.rs yet) or, after creating it, the four tests should pass on first compile because the implementation is included with the test. **Adjust:** the convention here is "write the test first". For an enum-only task, the test and the impl are inseparable — the test asserts the enum exists. Step 2 verifies after the file is in place.

- [ ] **Step 3: Run tests, expect them to pass**

Run: `cargo test -p boxpilot-ipc method`
Expected: 4 tests passed (`count_matches_spec`, `known_action_round_trips`, `underscore_variants_use_underscores_on_wire`, `unknown_action_fails_to_deserialize`).

- [ ] **Step 4: Commit**

```bash
git add crates/boxpilot-ipc/src/method.rs crates/boxpilot-ipc/src/lib.rs
git commit -m "feat(ipc): add HelperMethod enum mirroring spec §6.3"
```

---

## Task 4: Action classification (polkit ID, mutating, high-risk)

**Files:**
- Modify: `crates/boxpilot-ipc/src/method.rs`

- [ ] **Step 1: Write the failing test**

Append to `crates/boxpilot-ipc/src/method.rs`:

```rust
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
```

- [ ] **Step 2: Run tests, expect them to pass**

Run: `cargo test -p boxpilot-ipc method`
Expected: all 9 tests in `method` (4 from task 3 + 5 from this task) pass.

- [ ] **Step 3: Commit**

```bash
git add crates/boxpilot-ipc/src/method.rs
git commit -m "feat(ipc): classify HelperMethod auth class + polkit action ID"
```

---

## Task 5: `HelperError` enum

**Files:**
- Create: `crates/boxpilot-ipc/src/error.rs`
- Modify: `crates/boxpilot-ipc/src/lib.rs`

- [ ] **Step 1: Write the test and impl**

`crates/boxpilot-ipc/src/error.rs`:

```rust
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Wire error type returned to the GUI. Concrete strings match spec terminal
/// states (§6.6, §10) so the UI can branch on them deterministically.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Error)]
#[serde(tag = "code", rename_all = "snake_case")]
pub enum HelperError {
    /// Helper method exists but is not implemented in this build (plan #1
    /// returns this for everything except `service.status`).
    #[error("not implemented")]
    NotImplemented,

    #[error("not authorized by polkit")]
    NotAuthorized,

    /// Caller is a local user but is not the controller; mutating actions
    /// are refused.
    #[error("caller is not the controller user")]
    NotController,

    /// `controller_uid` resolves to a UID that no longer exists (§6.6).
    #[error("controller_uid points at a deleted user")]
    ControllerOrphaned,

    /// No controller has been claimed yet and the caller asked for a
    /// mutating action without going through the claim flow.
    #[error("no controller has been initialized")]
    ControllerNotSet,

    /// `boxpilot.toml`'s `schema_version` is unknown to this build.
    #[error("unsupported schema_version: {got}")]
    UnsupportedSchemaVersion { got: u32 },

    /// Could not acquire `/run/boxpilot/lock` — another mutating call is
    /// already in flight.
    #[error("helper busy: another privileged operation is in progress")]
    Busy,

    /// Anything systemd-related — querying a unit, parsing properties, etc.
    #[error("systemd error: {message}")]
    Systemd { message: String },

    /// Anything D-Bus-transport-related not covered above.
    #[error("ipc error: {message}")]
    Ipc { message: String },
}

pub type HelperResult<T> = Result<T, HelperError>;

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn discriminant_matches_spec_terminals() {
        let v = serde_json::to_value(HelperError::ControllerOrphaned).unwrap();
        assert_eq!(v, serde_json::json!({"code": "controller_orphaned"}));
    }

    #[test]
    fn parametric_error_round_trip() {
        let e = HelperError::UnsupportedSchemaVersion { got: 99 };
        let s = serde_json::to_string(&e).unwrap();
        let back: HelperError = serde_json::from_str(&s).unwrap();
        assert_eq!(back, e);
    }
}
```

Append to `crates/boxpilot-ipc/src/lib.rs`:

```rust
pub mod error;
pub use error::{HelperError, HelperResult};
```

- [ ] **Step 2: Run tests**

Run: `cargo test -p boxpilot-ipc error`
Expected: 2 tests pass.

- [ ] **Step 3: Commit**

```bash
git add crates/boxpilot-ipc/src/error.rs crates/boxpilot-ipc/src/lib.rs
git commit -m "feat(ipc): add HelperError enum with spec-named terminal states"
```

---

## Task 6: `ServiceStatusResponse` types

**Files:**
- Create: `crates/boxpilot-ipc/src/response.rs`
- Modify: `crates/boxpilot-ipc/src/lib.rs`

- [ ] **Step 1: Write the test and impl**

`crates/boxpilot-ipc/src/response.rs`:

```rust
use serde::{Deserialize, Serialize};

/// Mirrors `systemctl show` `ActiveState`/`SubState`/`LoadState`/`NRestarts`
/// fields plus a sentinel for "the unit doesn't exist".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum UnitState {
    /// `systemctl` doesn't know about `boxpilot-sing-box.service` at all.
    NotFound,
    Known {
        active_state: String,    // active | inactive | failed | activating | reloading | deactivating
        sub_state: String,       // running | dead | start-pre | failed | …
        load_state: String,      // loaded | not-found | error | masked | …
        n_restarts: u32,
        exec_main_status: i32,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServiceStatusResponse {
    pub unit_name: String,             // "boxpilot-sing-box.service"
    pub unit_state: UnitState,
    /// Snapshot of `controller_uid` resolution at call time. Useful for the
    /// Home page to surface `controller_orphaned` (§6.6) without a second RTT.
    pub controller: ControllerStatus,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum ControllerStatus {
    Unset,
    Set { uid: u32, username: String },
    Orphaned { uid: u32 },
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn unit_state_not_found_serialization() {
        let v = serde_json::to_value(UnitState::NotFound).unwrap();
        assert_eq!(v, serde_json::json!({"kind": "not_found"}));
    }

    #[test]
    fn unit_state_known_round_trip() {
        let s = UnitState::Known {
            active_state: "active".into(),
            sub_state: "running".into(),
            load_state: "loaded".into(),
            n_restarts: 0,
            exec_main_status: 0,
        };
        let json = serde_json::to_string(&s).unwrap();
        let back: UnitState = serde_json::from_str(&json).unwrap();
        assert_eq!(back, s);
    }

    #[test]
    fn controller_status_orphaned_round_trip() {
        let c = ControllerStatus::Orphaned { uid: 1500 };
        let json = serde_json::to_string(&c).unwrap();
        let back: ControllerStatus = serde_json::from_str(&json).unwrap();
        assert_eq!(back, c);
    }
}
```

Append to `crates/boxpilot-ipc/src/lib.rs`:

```rust
pub mod response;
pub use response::{ControllerStatus, ServiceStatusResponse, UnitState};
```

- [ ] **Step 2: Run tests**

Run: `cargo test -p boxpilot-ipc response`
Expected: 3 tests pass.

- [ ] **Step 3: Commit**

```bash
git add crates/boxpilot-ipc/src/response.rs crates/boxpilot-ipc/src/lib.rs
git commit -m "feat(ipc): add ServiceStatusResponse / UnitState / ControllerStatus"
```

---

## Task 7: `BoxpilotConfig` + schema_version rejection

**Files:**
- Create: `crates/boxpilot-ipc/src/config.rs`
- Modify: `crates/boxpilot-ipc/src/lib.rs`

- [ ] **Step 1: Write the test and impl**

`crates/boxpilot-ipc/src/config.rs`:

```rust
use crate::error::{HelperError, HelperResult};
use serde::{Deserialize, Serialize};

pub const CURRENT_SCHEMA_VERSION: u32 = 1;

/// Maps to `/etc/boxpilot/boxpilot.toml` (spec §5.3). Optional fields stay
/// `None` until the corresponding install/activate plan adds them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BoxpilotConfig {
    pub schema_version: u32,
    #[serde(default = "default_target_service")]
    pub target_service: String,
    #[serde(default)]
    pub core_path: Option<String>,
    #[serde(default)]
    pub core_state: Option<CoreState>,
    #[serde(default)]
    pub controller_uid: Option<u32>,
    #[serde(default)]
    pub active_profile_id: Option<String>,
    #[serde(default)]
    pub active_profile_name: Option<String>,
    #[serde(default)]
    pub active_profile_sha256: Option<String>,
    #[serde(default)]
    pub active_release_id: Option<String>,
    #[serde(default)]
    pub activated_at: Option<String>,
}

fn default_target_service() -> String {
    "boxpilot-sing-box.service".to_string()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CoreState {
    External,
    ManagedInstalled,
    ManagedAdopted,
}

impl BoxpilotConfig {
    /// Parse from the on-disk TOML. Rejects unknown `schema_version` per §5.3.
    pub fn parse(text: &str) -> HelperResult<Self> {
        // Step 1: peek schema_version without committing to the full schema,
        // so a future-version file produces a clean `UnsupportedSchemaVersion`
        // error rather than an unrelated "unknown field" error.
        #[derive(Deserialize)]
        struct Peek {
            schema_version: u32,
        }
        let peek: Peek = toml::from_str(text)
            .map_err(|e| HelperError::Ipc { message: format!("config parse: {e}") })?;
        if peek.schema_version != CURRENT_SCHEMA_VERSION {
            return Err(HelperError::UnsupportedSchemaVersion { got: peek.schema_version });
        }
        let cfg: BoxpilotConfig = toml::from_str(text)
            .map_err(|e| HelperError::Ipc { message: format!("config parse: {e}") })?;
        Ok(cfg)
    }

    pub fn to_toml(&self) -> String {
        // Encoded back via `toml::to_string` for atomic-write callers in
        // future plans (the activation pipeline writes boxpilot.toml.new
        // and renames it into place — see spec §10 step 13).
        toml::to_string(self).expect("BoxpilotConfig serializes")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const V1_FULL: &str = r#"
schema_version = 1
target_service = "boxpilot-sing-box.service"
core_path = "/var/lib/boxpilot/cores/current/sing-box"
core_state = "managed-installed"
controller_uid = 1000
"#;

    #[test]
    fn parses_v1_with_optional_fields_missing() {
        let cfg = BoxpilotConfig::parse("schema_version = 1\n").unwrap();
        assert_eq!(cfg.schema_version, 1);
        assert_eq!(cfg.target_service, "boxpilot-sing-box.service");
        assert_eq!(cfg.controller_uid, None);
    }

    #[test]
    fn parses_v1_full() {
        let cfg = BoxpilotConfig::parse(V1_FULL).unwrap();
        assert_eq!(cfg.controller_uid, Some(1000));
        assert_eq!(cfg.core_state, Some(CoreState::ManagedInstalled));
    }

    #[test]
    fn rejects_unknown_schema_version() {
        let r = BoxpilotConfig::parse("schema_version = 2\n");
        assert!(matches!(r, Err(HelperError::UnsupportedSchemaVersion { got: 2 })));
    }

    #[test]
    fn rejects_zero_or_missing_schema_version() {
        let r = BoxpilotConfig::parse("");
        assert!(matches!(r, Err(HelperError::Ipc { .. })));
    }

    #[test]
    fn round_trip_via_toml() {
        let cfg = BoxpilotConfig::parse(V1_FULL).unwrap();
        let text = cfg.to_toml();
        let back = BoxpilotConfig::parse(&text).unwrap();
        assert_eq!(back, cfg);
    }
}
```

Append to `crates/boxpilot-ipc/src/lib.rs`:

```rust
pub mod config;
pub use config::{BoxpilotConfig, CoreState, CURRENT_SCHEMA_VERSION};
```

- [ ] **Step 2: Run tests**

Run: `cargo test -p boxpilot-ipc config`
Expected: 5 tests pass.

- [ ] **Step 3: Commit**

```bash
git add crates/boxpilot-ipc/src/config.rs crates/boxpilot-ipc/src/lib.rs
git commit -m "feat(ipc): BoxpilotConfig with schema_version rejection (§5.3)"
```

---

## Task 8: `boxpilotd` crate skeleton

**Files:**
- Create: `crates/boxpilotd/Cargo.toml`
- Create: `crates/boxpilotd/src/main.rs`
- Create: `crates/boxpilotd/src/paths.rs`

- [ ] **Step 1: Write `Cargo.toml`**

```toml
[package]
name = "boxpilotd"
version = "0.1.0"
edition.workspace = true
rust-version.workspace = true
license.workspace = true

[[bin]]
name = "boxpilotd"
path = "src/main.rs"

[dependencies]
boxpilot-ipc = { path = "../boxpilot-ipc" }
serde.workspace = true
serde_json.workspace = true
toml.workspace = true
thiserror.workspace = true
anyhow.workspace = true
tracing.workspace = true
tracing-subscriber.workspace = true
tokio = { workspace = true, features = ["macros", "rt-multi-thread", "signal", "fs", "sync"] }
zbus.workspace = true
fs2.workspace = true
nix.workspace = true
async-trait = "0.1"

[dev-dependencies]
tempfile.workspace = true
pretty_assertions.workspace = true
tokio = { workspace = true, features = ["macros", "rt-multi-thread", "test-util"] }
```

- [ ] **Step 2: Write `paths.rs`**

```rust
//! Canonical filesystem paths used by the helper. Tests construct
//! `Paths::with_root(tmpdir)` so unit tests can run as a normal user without
//! touching real `/etc` or `/run`.

use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct Paths {
    root: PathBuf,
}

impl Paths {
    /// Production paths rooted at `/`.
    pub fn system() -> Self {
        Self { root: PathBuf::from("/") }
    }

    /// Test/dev paths rooted at an arbitrary directory.
    pub fn with_root(root: impl AsRef<Path>) -> Self {
        Self { root: root.as_ref().to_path_buf() }
    }

    pub fn boxpilot_toml(&self) -> PathBuf {
        self.root.join("etc/boxpilot/boxpilot.toml")
    }

    pub fn controller_uid_file(&self) -> PathBuf {
        self.root.join("etc/boxpilot/controller-uid")
    }

    pub fn run_lock(&self) -> PathBuf {
        self.root.join("run/boxpilot/lock")
    }

    pub fn run_dir(&self) -> PathBuf {
        self.root.join("run/boxpilot")
    }

    pub fn etc_dir(&self) -> PathBuf {
        self.root.join("etc/boxpilot")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn system_paths_anchor_at_root() {
        let p = Paths::system();
        assert_eq!(p.boxpilot_toml(), PathBuf::from("/etc/boxpilot/boxpilot.toml"));
        assert_eq!(p.run_lock(), PathBuf::from("/run/boxpilot/lock"));
    }

    #[test]
    fn with_root_relocates_everything() {
        let p = Paths::with_root("/tmp/fake");
        assert_eq!(p.boxpilot_toml(), PathBuf::from("/tmp/fake/etc/boxpilot/boxpilot.toml"));
    }
}
```

- [ ] **Step 3: Write `main.rs` (no D-Bus yet, just process bring-up)**

```rust
//! `boxpilotd` — privileged helper for BoxPilot. Activated on the system bus
//! by D-Bus; always runs as root. See spec §6.

mod paths;

use anyhow::Result;
use tracing::info;

#[tokio::main]
async fn main() -> Result<()> {
    init_tracing();
    info!(version = env!("CARGO_PKG_VERSION"), "boxpilotd starting");
    // Real D-Bus / signal-handling wiring lands in task 18.
    Ok(())
}

fn init_tracing() {
    use tracing_subscriber::{fmt, EnvFilter};
    let filter = EnvFilter::try_from_env("BOXPILOTD_LOG")
        .unwrap_or_else(|_| EnvFilter::new("boxpilotd=info"));
    fmt().with_env_filter(filter).with_target(false).init();
}
```

- [ ] **Step 4: Build and run**

Run: `cargo run --bin boxpilotd`
Expected: prints `INFO boxpilotd starting version=0.1.0` and exits 0.

Run: `cargo test -p boxpilotd paths`
Expected: 2 tests pass.

- [ ] **Step 5: Commit**

```bash
git add crates/boxpilotd
git commit -m "feat(boxpilotd): bring-up skeleton with Paths abstraction"
```

---

## Task 9: Global advisory lock

**Files:**
- Create: `crates/boxpilotd/src/lock.rs`
- Modify: `crates/boxpilotd/src/main.rs`

- [ ] **Step 1: Write the failing test**

`crates/boxpilotd/src/lock.rs`:

```rust
//! Global advisory lock on `/run/boxpilot/lock`. Held for any privileged
//! mutating operation (spec §6.4). `/run` is tmpfs and is cleared on reboot,
//! so a stale lock cannot survive a crash-restart.

use boxpilot_ipc::HelperError;
use fs2::FileExt;
use std::fs::{File, OpenOptions};
use std::io::ErrorKind;
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;

/// RAII guard. The flock is released on drop.
pub struct LockGuard {
    file: File,
}

impl Drop for LockGuard {
    fn drop(&mut self) {
        // Best-effort unlock; if it fails the kernel will release on close.
        let _ = self.file.unlock();
    }
}

/// Try to acquire the advisory lock. Returns [`HelperError::Busy`] if another
/// holder is present. The parent directory is created if missing.
pub fn try_acquire(lock_path: &Path) -> Result<LockGuard, HelperError> {
    if let Some(parent) = lock_path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| HelperError::Ipc {
            message: format!("create {parent:?}: {e}"),
        })?;
    }
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o644)
        .open(lock_path)
        .map_err(|e| HelperError::Ipc { message: format!("open lock: {e}") })?;
    match file.try_lock_exclusive() {
        Ok(()) => Ok(LockGuard { file }),
        Err(e) if e.kind() == ErrorKind::WouldBlock => Err(HelperError::Busy),
        Err(e) => Err(HelperError::Ipc { message: format!("flock: {e}") }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn acquires_when_unheld() {
        let dir = tempdir().unwrap();
        let lock = dir.path().join("lock");
        let _g = try_acquire(&lock).unwrap();
        assert!(lock.exists());
    }

    #[test]
    fn second_concurrent_acquire_returns_busy() {
        let dir = tempdir().unwrap();
        let lock = dir.path().join("lock");
        let _g1 = try_acquire(&lock).unwrap();
        let r2 = try_acquire(&lock);
        assert!(matches!(r2, Err(HelperError::Busy)));
    }

    #[test]
    fn dropping_guard_releases_lock() {
        let dir = tempdir().unwrap();
        let lock = dir.path().join("lock");
        {
            let _g1 = try_acquire(&lock).unwrap();
        }
        let _g2 = try_acquire(&lock).expect("lock should be free after drop");
    }
}
```

Modify `crates/boxpilotd/src/main.rs` to register the module:

```rust
mod lock;
mod paths;
```

- [ ] **Step 2: Run tests, verify they pass**

Run: `cargo test -p boxpilotd lock`
Expected: 3 tests pass.

- [ ] **Step 3: Commit**

```bash
git add crates/boxpilotd/src/lock.rs crates/boxpilotd/src/main.rs
git commit -m "feat(boxpilotd): /run/boxpilot/lock global advisory lock (§6.4)"
```

---

## Task 10: Controller-UID state machine

**Files:**
- Create: `crates/boxpilotd/src/controller.rs`
- Modify: `crates/boxpilotd/src/main.rs`

- [ ] **Step 1: Write the failing test**

`crates/boxpilotd/src/controller.rs`:

```rust
//! Controller-user model (spec §6.2, §6.6). The first authorized mutating
//! caller becomes the controller; mismatches/orphans are reported through
//! [`ControllerState`].

use boxpilot_ipc::ControllerStatus;

/// User lookup is split out behind a trait so unit tests don't depend on
/// real `/etc/passwd` state.
pub trait UserLookup: Send + Sync {
    fn lookup_username(&self, uid: u32) -> Option<String>;
}

pub struct PasswdLookup;

impl UserLookup for PasswdLookup {
    fn lookup_username(&self, uid: u32) -> Option<String> {
        nix::unistd::User::from_uid(nix::unistd::Uid::from_raw(uid))
            .ok()
            .flatten()
            .map(|u| u.name)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ControllerState {
    Unset,
    Set { uid: u32, username: String },
    Orphaned { uid: u32 },
}

impl ControllerState {
    pub fn from_uid(uid: Option<u32>, lookup: &dyn UserLookup) -> Self {
        match uid {
            None => ControllerState::Unset,
            Some(uid) => match lookup.lookup_username(uid) {
                Some(username) => ControllerState::Set { uid, username },
                None => ControllerState::Orphaned { uid },
            },
        }
    }

    pub fn is_controller(&self, caller_uid: u32) -> bool {
        matches!(self, ControllerState::Set { uid, .. } if *uid == caller_uid)
    }

    pub fn to_status(&self) -> ControllerStatus {
        match self {
            ControllerState::Unset => ControllerStatus::Unset,
            ControllerState::Set { uid, username } => {
                ControllerStatus::Set { uid: *uid, username: username.clone() }
            }
            ControllerState::Orphaned { uid } => ControllerStatus::Orphaned { uid: *uid },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::Mutex;

    struct Fixed(Mutex<HashMap<u32, String>>);
    impl Fixed {
        fn new(rows: &[(u32, &str)]) -> Self {
            Self(Mutex::new(rows.iter().map(|(u, n)| (*u, n.to_string())).collect()))
        }
    }
    impl UserLookup for Fixed {
        fn lookup_username(&self, uid: u32) -> Option<String> {
            self.0.lock().unwrap().get(&uid).cloned()
        }
    }

    #[test]
    fn no_uid_is_unset() {
        let lookup = Fixed::new(&[]);
        assert_eq!(ControllerState::from_uid(None, &lookup), ControllerState::Unset);
    }

    #[test]
    fn live_uid_is_set() {
        let lookup = Fixed::new(&[(1000, "alice")]);
        let s = ControllerState::from_uid(Some(1000), &lookup);
        assert_eq!(s, ControllerState::Set { uid: 1000, username: "alice".into() });
    }

    #[test]
    fn missing_uid_is_orphaned() {
        let lookup = Fixed::new(&[]);
        let s = ControllerState::from_uid(Some(1500), &lookup);
        assert_eq!(s, ControllerState::Orphaned { uid: 1500 });
    }

    #[test]
    fn is_controller_only_when_set_and_matching() {
        let lookup = Fixed::new(&[(1000, "alice")]);
        let s = ControllerState::from_uid(Some(1000), &lookup);
        assert!(s.is_controller(1000));
        assert!(!s.is_controller(1001));

        let unset = ControllerState::from_uid(None, &lookup);
        assert!(!unset.is_controller(1000));

        let orphan = ControllerState::Orphaned { uid: 1000 };
        assert!(!orphan.is_controller(1000));
    }
}
```

Append to `main.rs`:

```rust
mod controller;
```

- [ ] **Step 2: Run tests**

Run: `cargo test -p boxpilotd controller`
Expected: 4 tests pass.

- [ ] **Step 3: Commit**

```bash
git add crates/boxpilotd/src/controller.rs crates/boxpilotd/src/main.rs
git commit -m "feat(boxpilotd): controller-UID state machine (§6.2, §6.6)"
```

---

## Task 11: Caller credentials abstraction

**Files:**
- Create: `crates/boxpilotd/src/credentials.rs`
- Modify: `crates/boxpilotd/src/main.rs`

- [ ] **Step 1: Write the test and impl**

`crates/boxpilotd/src/credentials.rs`:

```rust
//! Caller-identity extraction. **Identity must come from the D-Bus
//! connection, never from the request body** — spec §6.1.

use async_trait::async_trait;
use boxpilot_ipc::HelperError;
use zbus::Connection;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallerIdentity {
    pub uid: u32,
    pub sender: String,
}

#[async_trait]
pub trait CallerResolver: Send + Sync {
    async fn resolve(&self, sender: &str) -> Result<u32, HelperError>;
}

/// Real resolver: calls `org.freedesktop.DBus.GetConnectionUnixUser` on the
/// system bus.
pub struct DBusCallerResolver {
    conn: Connection,
}

impl DBusCallerResolver {
    pub fn new(conn: Connection) -> Self {
        Self { conn }
    }
}

#[async_trait]
impl CallerResolver for DBusCallerResolver {
    async fn resolve(&self, sender: &str) -> Result<u32, HelperError> {
        let proxy = zbus::fdo::DBusProxy::new(&self.conn)
            .await
            .map_err(|e| HelperError::Ipc { message: format!("DBusProxy: {e}") })?;
        let uid = proxy
            .get_connection_unix_user(sender.try_into().map_err(|e| HelperError::Ipc {
                message: format!("bad sender name {sender}: {e}"),
            })?)
            .await
            .map_err(|e| HelperError::Ipc {
                message: format!("GetConnectionUnixUser({sender}): {e}"),
            })?;
        Ok(uid)
    }
}

#[cfg(test)]
pub mod testing {
    use super::*;
    use std::collections::HashMap;
    use std::sync::Mutex;

    pub struct FixedResolver(pub Mutex<HashMap<String, u32>>);

    impl FixedResolver {
        pub fn with(rows: &[(&str, u32)]) -> Self {
            Self(Mutex::new(rows.iter().map(|(s, u)| (s.to_string(), *u)).collect()))
        }
    }

    #[async_trait]
    impl CallerResolver for FixedResolver {
        async fn resolve(&self, sender: &str) -> Result<u32, HelperError> {
            self.0
                .lock()
                .unwrap()
                .get(sender)
                .copied()
                .ok_or_else(|| HelperError::Ipc {
                    message: format!("test: unknown sender {sender}"),
                })
        }
    }

    #[tokio::test]
    async fn fixed_resolver_returns_canned_uid() {
        let r = FixedResolver::with(&[(":1.42", 1000)]);
        assert_eq!(r.resolve(":1.42").await.unwrap(), 1000);
    }
}
```

Append to `main.rs`:

```rust
mod credentials;
```

- [ ] **Step 2: Run tests**

Run: `cargo test -p boxpilotd credentials`
Expected: 1 test passes (`testing::fixed_resolver_returns_canned_uid`).

- [ ] **Step 3: Commit**

```bash
git add crates/boxpilotd/src/credentials.rs crates/boxpilotd/src/main.rs
git commit -m "feat(boxpilotd): CallerResolver trait + DBusCallerResolver"
```

---

## Task 12: polkit `Authority` trait

**Files:**
- Create: `crates/boxpilotd/src/authority.rs`
- Modify: `crates/boxpilotd/src/main.rs`

- [ ] **Step 1: Write the test and impl**

`crates/boxpilotd/src/authority.rs`:

```rust
//! polkit authorization. Calls
//! `org.freedesktop.PolicyKit1.Authority.CheckAuthorization` on the system
//! bus. Subject is constructed from the caller's D-Bus bus name (`:x.y`)
//! using `kind = "system-bus-name"`.

use async_trait::async_trait;
use boxpilot_ipc::HelperError;
use std::collections::HashMap;
use zbus::{
    proxy,
    zvariant::{OwnedValue, Value},
    Connection,
};

#[async_trait]
pub trait Authority: Send + Sync {
    /// Returns Ok(true) if authorized, Ok(false) if denied (including auth
    /// dismissal), Err if polkit itself errors.
    async fn check(&self, action_id: &str, sender_bus_name: &str) -> Result<bool, HelperError>;
}

#[proxy(
    interface = "org.freedesktop.PolicyKit1.Authority",
    default_service = "org.freedesktop.PolicyKit1",
    default_path = "/org/freedesktop/PolicyKit1/Authority"
)]
trait PolkitAuthority {
    #[zbus(name = "CheckAuthorization")]
    fn check_authorization(
        &self,
        subject: &(&str, HashMap<&str, Value<'_>>),
        action_id: &str,
        details: HashMap<&str, &str>,
        flags: u32,
        cancellation_id: &str,
    ) -> zbus::Result<(bool, bool, HashMap<String, String>)>;
}

const FLAG_ALLOW_USER_INTERACTION: u32 = 1;

pub struct DBusAuthority {
    conn: Connection,
}

impl DBusAuthority {
    pub fn new(conn: Connection) -> Self {
        Self { conn }
    }
}

#[async_trait]
impl Authority for DBusAuthority {
    async fn check(&self, action_id: &str, sender_bus_name: &str) -> Result<bool, HelperError> {
        let proxy = PolkitAuthorityProxy::new(&self.conn)
            .await
            .map_err(|e| HelperError::Ipc { message: format!("polkit proxy: {e}") })?;

        let mut subject_data: HashMap<&str, Value<'_>> = HashMap::new();
        let bus_name_value = Value::Str(sender_bus_name.into());
        subject_data.insert("name", bus_name_value);

        let (is_authorized, _is_challenge, _details) = proxy
            .check_authorization(
                &("system-bus-name", subject_data),
                action_id,
                HashMap::new(),
                FLAG_ALLOW_USER_INTERACTION,
                "", // cancellation id (unused)
            )
            .await
            .map_err(|e| HelperError::Ipc {
                message: format!("polkit CheckAuthorization({action_id}): {e}"),
            })?;
        // Reference OwnedValue to silence unused-import warnings if zbus
        // changes its re-exports between minor versions.
        let _ = std::marker::PhantomData::<OwnedValue>;
        Ok(is_authorized)
    }
}

#[cfg(test)]
pub mod testing {
    use super::*;
    use std::collections::HashMap as Map;
    use std::sync::Mutex;

    pub struct CannedAuthority(pub Mutex<Map<String, bool>>);

    impl CannedAuthority {
        pub fn allowing(actions: &[&str]) -> Self {
            Self(Mutex::new(actions.iter().map(|a| (a.to_string(), true)).collect()))
        }
        pub fn denying(actions: &[&str]) -> Self {
            Self(Mutex::new(actions.iter().map(|a| (a.to_string(), false)).collect()))
        }
    }

    #[async_trait]
    impl Authority for CannedAuthority {
        async fn check(&self, action_id: &str, _sender: &str) -> Result<bool, HelperError> {
            let map = self.0.lock().unwrap();
            map.get(action_id).copied().ok_or_else(|| HelperError::Ipc {
                message: format!("test: unconfigured action {action_id}"),
            })
        }
    }

    #[tokio::test]
    async fn canned_allow() {
        let a = CannedAuthority::allowing(&["app.boxpilot.helper.service.status"]);
        assert!(a.check("app.boxpilot.helper.service.status", ":1.5").await.unwrap());
    }

    #[tokio::test]
    async fn canned_deny() {
        let a = CannedAuthority::denying(&["app.boxpilot.helper.service.start"]);
        assert!(!a.check("app.boxpilot.helper.service.start", ":1.5").await.unwrap());
    }
}
```

Append to `main.rs`:

```rust
mod authority;
```

- [ ] **Step 2: Run tests**

Run: `cargo test -p boxpilotd authority`
Expected: 2 tests pass.

- [ ] **Step 3: Commit**

```bash
git add crates/boxpilotd/src/authority.rs crates/boxpilotd/src/main.rs
git commit -m "feat(boxpilotd): polkit Authority trait + DBusAuthority + CannedAuthority"
```

---

## Task 13: `SystemdQuery` trait + DBus impl + mock

**Files:**
- Create: `crates/boxpilotd/src/systemd.rs`
- Modify: `crates/boxpilotd/src/main.rs`

- [ ] **Step 1: Write the test and impl**

`crates/boxpilotd/src/systemd.rs`:

```rust
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
            .map_err(|e| HelperError::Systemd { message: format!("manager proxy: {e}") })?;

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
            Err(e) => return Err(HelperError::Systemd { message: format!("GetUnit: {e}") }),
        };

        let unit = SystemdUnitProxy::builder(&self.conn)
            .destination("org.freedesktop.systemd1")
            .map_err(|e| HelperError::Systemd { message: e.to_string() })?
            .path(unit_path.clone())
            .map_err(|e| HelperError::Systemd { message: e.to_string() })?
            .build()
            .await
            .map_err(|e| HelperError::Systemd { message: e.to_string() })?;
        let active_state = unit.active_state().await.map_err(systemd_err)?;
        let sub_state = unit.sub_state().await.map_err(systemd_err)?;
        let load_state = unit.load_state().await.map_err(systemd_err)?;

        let svc = SystemdServiceProxy::builder(&self.conn)
            .destination("org.freedesktop.systemd1")
            .map_err(|e| HelperError::Systemd { message: e.to_string() })?
            .path(unit_path)
            .map_err(|e| HelperError::Systemd { message: e.to_string() })?
            .build()
            .await
            .map_err(|e| HelperError::Systemd { message: e.to_string() })?;
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
    HelperError::Systemd { message: e.to_string() }
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
        let q = FixedSystemd { answer: UnitState::NotFound };
        assert_eq!(q.unit_state("anything").await.unwrap(), UnitState::NotFound);
    }
}
```

Append to `main.rs`:

```rust
mod systemd;
```

- [ ] **Step 2: Run tests**

Run: `cargo test -p boxpilotd systemd`
Expected: 1 test passes.

- [ ] **Step 3: Commit**

```bash
git add crates/boxpilotd/src/systemd.rs crates/boxpilotd/src/main.rs
git commit -m "feat(boxpilotd): SystemdQuery trait + DBusSystemd + FixedSystemd"
```

---

## Task 14: `HelperContext` composing dependencies

**Files:**
- Create: `crates/boxpilotd/src/context.rs`
- Modify: `crates/boxpilotd/src/main.rs`

- [ ] **Step 1: Write the test and impl**

`crates/boxpilotd/src/context.rs`:

```rust
//! Bundle of trait objects used by every method handler. Keeps the
//! [`crate::iface::Helper`] D-Bus interface struct small and lets unit tests
//! swap any dependency.

use crate::authority::Authority;
use crate::controller::{ControllerState, UserLookup};
use crate::credentials::CallerResolver;
use crate::paths::Paths;
use crate::systemd::SystemdQuery;
use boxpilot_ipc::{BoxpilotConfig, HelperError, HelperResult};
use std::sync::Arc;
use tokio::sync::RwLock;

pub struct HelperContext {
    pub paths: Paths,
    pub callers: Arc<dyn CallerResolver>,
    pub authority: Arc<dyn Authority>,
    pub systemd: Arc<dyn SystemdQuery>,
    pub user_lookup: Arc<dyn UserLookup>,
    /// Cached parsed config; reloaded on demand. Held inside an RwLock so a
    /// future "reload-on-SIGHUP" task can swap the config atomically.
    config: RwLock<Option<BoxpilotConfig>>,
}

impl HelperContext {
    pub fn new(
        paths: Paths,
        callers: Arc<dyn CallerResolver>,
        authority: Arc<dyn Authority>,
        systemd: Arc<dyn SystemdQuery>,
        user_lookup: Arc<dyn UserLookup>,
    ) -> Self {
        Self {
            paths,
            callers,
            authority,
            systemd,
            user_lookup,
            config: RwLock::new(None),
        }
    }

    /// Read or re-read `boxpilot.toml`. Missing file → returns a freshly
    /// minted v1 config with no fields populated, so the helper still
    /// answers `service.status` on a fresh box (controller is `Unset`).
    pub async fn load_config(&self) -> HelperResult<BoxpilotConfig> {
        let path = self.paths.boxpilot_toml();
        match tokio::fs::read_to_string(&path).await {
            Ok(text) => {
                let cfg = BoxpilotConfig::parse(&text)?;
                *self.config.write().await = Some(cfg.clone());
                Ok(cfg)
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                Ok(BoxpilotConfig {
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
                })
            }
            Err(e) => Err(HelperError::Ipc {
                message: format!("read {path:?}: {e}"),
            }),
        }
    }

    pub async fn controller_state(&self) -> HelperResult<ControllerState> {
        let cfg = self.load_config().await?;
        Ok(ControllerState::from_uid(cfg.controller_uid, &*self.user_lookup))
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
        HelperContext::new(
            paths,
            Arc::new(FixedResolver::with(callers)),
            Arc::new(authority),
            Arc::new(FixedSystemd { answer: systemd_answer }),
            Arc::new(PasswdLookup),
        )
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
        assert!(matches!(r, Err(HelperError::UnsupportedSchemaVersion { got: 2 })));
    }
}
```

Append to `main.rs`:

```rust
mod context;
```

- [ ] **Step 2: Run tests**

Run: `cargo test -p boxpilotd context`
Expected: 3 tests pass.

- [ ] **Step 3: Commit**

```bash
git add crates/boxpilotd/src/context.rs crates/boxpilotd/src/main.rs
git commit -m "feat(boxpilotd): HelperContext composing the dependency traits"
```

---

## Task 15: `authorize_call` dispatch helper

**Files:**
- Create: `crates/boxpilotd/src/dispatch.rs`
- Modify: `crates/boxpilotd/src/main.rs`

- [ ] **Step 1: Write the test and impl**

`crates/boxpilotd/src/dispatch.rs`:

```rust
//! Single chokepoint every interface method passes through:
//! 1. resolve caller UID from D-Bus connection credentials
//! 2. compute controller status, surface `controller_orphaned` (§6.6)
//! 3. for mutating calls without a controller, refuse (`ControllerNotSet`)
//! 4. ask polkit for authorization
//! 5. for mutating calls, acquire `/run/boxpilot/lock`
//! 6. invoke the action body
//!
//! Step 6 is generic over the action body so each interface method stays
//! a 1-2 line wrapper.

use crate::context::HelperContext;
use crate::controller::ControllerState;
use crate::lock::{self, LockGuard};
use boxpilot_ipc::{HelperError, HelperMethod, HelperResult};

pub struct AuthorizedCall {
    pub caller_uid: u32,
    pub controller: ControllerState,
    /// Held only when [`HelperMethod::is_mutating`] is true.
    _lock: Option<LockGuard>,
}

pub async fn authorize(
    ctx: &HelperContext,
    sender_bus_name: &str,
    method: HelperMethod,
) -> HelperResult<AuthorizedCall> {
    let caller_uid = ctx.callers.resolve(sender_bus_name).await?;
    let controller = ctx.controller_state().await?;

    if let ControllerState::Orphaned { .. } = controller {
        // Read-only methods are still allowed; mutating ones are blocked
        // until controller.transfer succeeds (§6.6).
        if method.is_mutating() {
            return Err(HelperError::ControllerOrphaned);
        }
    }

    if method.is_mutating() {
        if matches!(controller, ControllerState::Unset) {
            // Plan #1 ships no path that would set the controller, so this
            // branch is reachable only by a non-status mutating method
            // (all of which return NotImplemented here). Keeping the check
            // wired makes the dispatch contract correct for plan #2 onward.
            return Err(HelperError::ControllerNotSet);
        }
    }

    let action_id = method.polkit_action_id();
    let allowed = ctx.authority.check(&action_id, sender_bus_name).await?;
    if !allowed {
        return Err(HelperError::NotAuthorized);
    }

    let lock = if method.is_mutating() {
        Some(lock::try_acquire(&ctx.paths.run_lock())?)
    } else {
        None
    };

    Ok(AuthorizedCall { caller_uid, controller, _lock: lock })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::authority::testing::CannedAuthority;
    use crate::context::testing::ctx_with;
    use boxpilot_ipc::UnitState;
    use tempfile::tempdir;

    #[tokio::test]
    async fn read_only_call_with_polkit_yes_succeeds() {
        let tmp = tempdir().unwrap();
        let ctx = ctx_with(
            &tmp,
            None,
            CannedAuthority::allowing(&["app.boxpilot.helper.service.status"]),
            UnitState::NotFound,
            &[(":1.42", 1000)],
        );
        let r = authorize(&ctx, ":1.42", HelperMethod::ServiceStatus).await;
        assert!(r.is_ok());
    }

    #[tokio::test]
    async fn read_only_call_with_polkit_no_returns_not_authorized() {
        let tmp = tempdir().unwrap();
        let ctx = ctx_with(
            &tmp,
            None,
            CannedAuthority::denying(&["app.boxpilot.helper.service.status"]),
            UnitState::NotFound,
            &[(":1.42", 1000)],
        );
        let r = authorize(&ctx, ":1.42", HelperMethod::ServiceStatus).await;
        assert!(matches!(r, Err(HelperError::NotAuthorized)));
    }

    #[tokio::test]
    async fn mutating_call_without_controller_returns_controller_not_set() {
        let tmp = tempdir().unwrap();
        let ctx = ctx_with(
            &tmp,
            None,
            CannedAuthority::allowing(&["app.boxpilot.helper.service.start"]),
            UnitState::NotFound,
            &[(":1.42", 1000)],
        );
        let r = authorize(&ctx, ":1.42", HelperMethod::ServiceStart).await;
        assert!(matches!(r, Err(HelperError::ControllerNotSet)));
    }

    #[tokio::test]
    async fn mutating_call_with_orphaned_controller_returns_orphaned() {
        let tmp = tempdir().unwrap();
        // 4_000_000_000 is virtually guaranteed not to map to a real user.
        let ctx = ctx_with(
            &tmp,
            Some("schema_version = 1\ncontroller_uid = 4000000000\n"),
            CannedAuthority::allowing(&["app.boxpilot.helper.service.start"]),
            UnitState::NotFound,
            &[(":1.42", 1000)],
        );
        let r = authorize(&ctx, ":1.42", HelperMethod::ServiceStart).await;
        assert!(matches!(r, Err(HelperError::ControllerOrphaned)));
    }

    #[tokio::test]
    async fn read_only_call_with_orphaned_controller_still_succeeds() {
        let tmp = tempdir().unwrap();
        let ctx = ctx_with(
            &tmp,
            Some("schema_version = 1\ncontroller_uid = 4000000000\n"),
            CannedAuthority::allowing(&["app.boxpilot.helper.service.status"]),
            UnitState::NotFound,
            &[(":1.42", 1000)],
        );
        let r = authorize(&ctx, ":1.42", HelperMethod::ServiceStatus).await;
        assert!(r.is_ok());
    }
}
```

Append to `main.rs`:

```rust
mod dispatch;
```

- [ ] **Step 2: Run tests**

Run: `cargo test -p boxpilotd dispatch`
Expected: 5 tests pass.

- [ ] **Step 3: Commit**

```bash
git add crates/boxpilotd/src/dispatch.rs crates/boxpilotd/src/main.rs
git commit -m "feat(boxpilotd): authorize() dispatch chokepoint with full §6 contract"
```

---

## Task 16: `Helper` D-Bus interface — service_status implementation

**Files:**
- Create: `crates/boxpilotd/src/iface.rs`
- Modify: `crates/boxpilotd/src/main.rs`

- [ ] **Step 1: Write the test and impl**

`crates/boxpilotd/src/iface.rs`:

```rust
//! `app.boxpilot.Helper1` D-Bus interface. Each method goes through
//! [`crate::dispatch::authorize`] before doing any work.
//!
//! Method names on the bus are CamelCase per D-Bus convention; the logical
//! action mapping is in `boxpilot_ipc::HelperMethod`.

use crate::context::HelperContext;
use crate::dispatch;
use boxpilot_ipc::{HelperError, HelperMethod, ServiceStatusResponse};
use std::sync::Arc;
use tracing::{instrument, warn};
use zbus::interface;

pub struct Helper {
    ctx: Arc<HelperContext>,
}

impl Helper {
    pub fn new(ctx: Arc<HelperContext>) -> Self {
        Self { ctx }
    }
}

/// Convert a HelperError into a zbus method error. The error name follows
/// reverse-DNS form so the GUI can branch on `e.name()`.
fn to_zbus_err(e: HelperError) -> zbus::fdo::Error {
    let name = match &e {
        HelperError::NotImplemented => "app.boxpilot.Helper1.NotImplemented",
        HelperError::NotAuthorized => "app.boxpilot.Helper1.NotAuthorized",
        HelperError::NotController => "app.boxpilot.Helper1.NotController",
        HelperError::ControllerOrphaned => "app.boxpilot.Helper1.ControllerOrphaned",
        HelperError::ControllerNotSet => "app.boxpilot.Helper1.ControllerNotSet",
        HelperError::UnsupportedSchemaVersion { .. } => {
            "app.boxpilot.Helper1.UnsupportedSchemaVersion"
        }
        HelperError::Busy => "app.boxpilot.Helper1.Busy",
        HelperError::Systemd { .. } => "app.boxpilot.Helper1.Systemd",
        HelperError::Ipc { .. } => "app.boxpilot.Helper1.Ipc",
    };
    let msg = e.to_string();
    // We use zbus::fdo::Error::Failed as the carrier; the precise mapping
    // gets refined when zbus exposes a way to set arbitrary error names
    // from interface methods. For now, encode the typed name into the
    // message prefix so the GUI can still discriminate.
    zbus::fdo::Error::Failed(format!("{name}: {msg}"))
}

#[interface(name = "app.boxpilot.Helper1")]
impl Helper {
    /// Returns spec §3.1 / §6.3 `service.status`. Read-only; no controller
    /// required; orphaned controller is reported in the response, not as an
    /// error.
    #[instrument(skip(self, header))]
    async fn service_status(
        &self,
        #[zbus(header)] header: zbus::message::Header<'_>,
    ) -> zbus::fdo::Result<String> {
        let sender = header.sender().ok_or_else(|| {
            zbus::fdo::Error::Failed(
                "app.boxpilot.Helper1.Ipc: missing sender on incoming message".into(),
            )
        })?;
        let resp = self.do_service_status(&sender.to_string()).await.map_err(to_zbus_err)?;
        // Wire format on D-Bus is a single JSON string. We use JSON rather
        // than a nested zbus dict so the IPC types live in one Rust type
        // hierarchy and the GUI can deserialize via serde without a
        // bespoke zvariant→TS layer.
        Ok(serde_json::to_string(&resp).map_err(|e| {
            zbus::fdo::Error::Failed(format!("app.boxpilot.Helper1.Ipc: serialize: {e}"))
        })?)
    }

    // ----- Stubs for the other 18 actions (filled in by plans #2-#9). -----
    async fn service_start(&self) -> zbus::fdo::Result<String> { stub() }
    async fn service_stop(&self) -> zbus::fdo::Result<String> { stub() }
    async fn service_restart(&self) -> zbus::fdo::Result<String> { stub() }
    async fn service_enable(&self) -> zbus::fdo::Result<String> { stub() }
    async fn service_disable(&self) -> zbus::fdo::Result<String> { stub() }
    async fn service_install_managed(&self) -> zbus::fdo::Result<String> { stub() }
    async fn service_logs(&self) -> zbus::fdo::Result<String> { stub() }
    async fn profile_activate_bundle(&self) -> zbus::fdo::Result<String> { stub() }
    async fn profile_rollback_release(&self) -> zbus::fdo::Result<String> { stub() }
    async fn core_discover(&self) -> zbus::fdo::Result<String> { stub() }
    async fn core_install_managed(&self) -> zbus::fdo::Result<String> { stub() }
    async fn core_upgrade_managed(&self) -> zbus::fdo::Result<String> { stub() }
    async fn core_rollback_managed(&self) -> zbus::fdo::Result<String> { stub() }
    async fn core_adopt(&self) -> zbus::fdo::Result<String> { stub() }
    async fn legacy_observe_service(&self) -> zbus::fdo::Result<String> { stub() }
    async fn legacy_migrate_service(&self) -> zbus::fdo::Result<String> { stub() }
    async fn controller_transfer(&self) -> zbus::fdo::Result<String> { stub() }
    async fn diagnostics_export_redacted(&self) -> zbus::fdo::Result<String> { stub() }
}

fn stub() -> zbus::fdo::Result<String> {
    warn!("called a not-yet-implemented helper method");
    Err(to_zbus_err(HelperError::NotImplemented))
}

impl Helper {
    async fn do_service_status(
        &self,
        sender_bus_name: &str,
    ) -> Result<ServiceStatusResponse, HelperError> {
        let _call = dispatch::authorize(&self.ctx, sender_bus_name, HelperMethod::ServiceStatus).await?;
        let cfg = self.ctx.load_config().await?;
        let unit_name = cfg.target_service.clone();
        let unit_state = self.ctx.systemd.unit_state(&unit_name).await?;
        let controller = self.ctx.controller_state().await?.to_status();
        Ok(ServiceStatusResponse { unit_name, unit_state, controller })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::authority::testing::CannedAuthority;
    use crate::context::testing::ctx_with;
    use boxpilot_ipc::UnitState;
    use tempfile::tempdir;

    #[tokio::test]
    async fn service_status_passes_through_unit_not_found() {
        let tmp = tempdir().unwrap();
        let ctx = Arc::new(ctx_with(
            &tmp,
            None,
            CannedAuthority::allowing(&["app.boxpilot.helper.service.status"]),
            UnitState::NotFound,
            &[(":1.42", 1000)],
        ));
        let h = Helper::new(ctx);
        let resp = h.do_service_status(":1.42").await.unwrap();
        assert_eq!(resp.unit_name, "boxpilot-sing-box.service");
        assert_eq!(resp.unit_state, UnitState::NotFound);
    }

    #[tokio::test]
    async fn service_status_returns_known_state_when_unit_exists() {
        let tmp = tempdir().unwrap();
        let known = UnitState::Known {
            active_state: "active".into(),
            sub_state: "running".into(),
            load_state: "loaded".into(),
            n_restarts: 2,
            exec_main_status: 0,
        };
        let ctx = Arc::new(ctx_with(
            &tmp,
            Some("schema_version = 1\ncontroller_uid = 1000\n"),
            CannedAuthority::allowing(&["app.boxpilot.helper.service.status"]),
            known.clone(),
            &[(":1.42", 1000)],
        ));
        let h = Helper::new(ctx);
        let resp = h.do_service_status(":1.42").await.unwrap();
        assert_eq!(resp.unit_state, known);
    }

    #[tokio::test]
    async fn service_status_denied_by_polkit_returns_not_authorized() {
        let tmp = tempdir().unwrap();
        let ctx = Arc::new(ctx_with(
            &tmp,
            None,
            CannedAuthority::denying(&["app.boxpilot.helper.service.status"]),
            UnitState::NotFound,
            &[(":1.42", 1000)],
        ));
        let h = Helper::new(ctx);
        let r = h.do_service_status(":1.42").await;
        assert!(matches!(r, Err(HelperError::NotAuthorized)));
    }
}
```

Append to `main.rs`:

```rust
mod iface;
```

- [ ] **Step 2: Run tests**

Run: `cargo test -p boxpilotd iface`
Expected: 3 tests pass.

- [ ] **Step 3: Commit**

```bash
git add crates/boxpilotd/src/iface.rs crates/boxpilotd/src/main.rs
git commit -m "feat(boxpilotd): app.boxpilot.Helper1 interface with service.status wired"
```

---

## Task 17: Wire D-Bus server bring-up in `main.rs`

**Files:**
- Modify: `crates/boxpilotd/src/main.rs`

- [ ] **Step 1: Replace `main.rs`**

```rust
//! `boxpilotd` — privileged helper for BoxPilot. Activated on the system bus
//! by D-Bus; always runs as root. See spec §6.

mod authority;
mod context;
mod controller;
mod credentials;
mod dispatch;
mod iface;
mod lock;
mod paths;
mod systemd;

use anyhow::{Context, Result};
use std::sync::Arc;
use tracing::{error, info};

const BUS_NAME: &str = "app.boxpilot.Helper";
const OBJECT_PATH: &str = "/app/boxpilot/Helper";

#[tokio::main]
async fn main() -> Result<()> {
    init_tracing();
    info!(version = env!("CARGO_PKG_VERSION"), "boxpilotd starting");

    if let Err(e) = ensure_running_as_root() {
        error!("refusing to start: {e}");
        std::process::exit(2);
    }

    let conn = zbus::connection::Builder::system()
        .context("connect to system bus")?
        .build()
        .await
        .context("system bus build")?;

    let ctx = Arc::new(context::HelperContext::new(
        paths::Paths::system(),
        Arc::new(credentials::DBusCallerResolver::new(conn.clone())),
        Arc::new(authority::DBusAuthority::new(conn.clone())),
        Arc::new(systemd::DBusSystemd::new(conn.clone())),
        Arc::new(controller::PasswdLookup),
    ));

    let helper = iface::Helper::new(ctx);
    conn.object_server()
        .at(OBJECT_PATH, helper)
        .await
        .context("register Helper at object path")?;
    conn.request_name(BUS_NAME).await.context("acquire bus name")?;
    info!(bus = BUS_NAME, "ready");

    // Block until SIGTERM / SIGINT.
    let mut sigterm = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
    let mut sigint = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt())?;
    tokio::select! {
        _ = sigterm.recv() => info!("SIGTERM received"),
        _ = sigint.recv()  => info!("SIGINT received"),
    }
    info!("shutting down");
    Ok(())
}

fn ensure_running_as_root() -> Result<()> {
    let uid = nix::unistd::Uid::current();
    if !uid.is_root() {
        anyhow::bail!("must run as root (uid 0); current uid is {uid}");
    }
    Ok(())
}

fn init_tracing() {
    use tracing_subscriber::{fmt, EnvFilter};
    let filter = EnvFilter::try_from_env("BOXPILOTD_LOG")
        .unwrap_or_else(|_| EnvFilter::new("boxpilotd=info"));
    fmt().with_env_filter(filter).with_target(false).init();
}
```

- [ ] **Step 2: Build**

Run: `cargo build --bin boxpilotd`
Expected: builds cleanly with no warnings (run `cargo clippy --bin boxpilotd -- -D warnings` to enforce).

- [ ] **Step 3: Smoke run as non-root (must refuse)**

Run: `cargo run --bin boxpilotd`
Expected: exits with `error: refusing to start: must run as root...` and exit code 2.

- [ ] **Step 4: Run all crate tests**

Run: `cargo test --workspace`
Expected: every test green; nothing broken by the refactor.

- [ ] **Step 5: Commit**

```bash
git add crates/boxpilotd/src/main.rs
git commit -m "feat(boxpilotd): wire D-Bus server with system-bus name and root guard"
```

---

## Task 18: D-Bus system-bus access policy

**Files:**
- Create: `packaging/linux/dbus/system.d/app.boxpilot.helper.conf`

- [ ] **Step 1: Write the file**

```xml
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE busconfig PUBLIC
 "-//freedesktop//DTD D-Bus Bus Configuration 1.0//EN"
 "https://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd">
<busconfig>
  <!-- Only root may own the bus name. -->
  <policy user="root">
    <allow own="app.boxpilot.Helper"/>
    <allow send_destination="app.boxpilot.Helper"/>
  </policy>

  <!-- Any local user may *call* into the helper; method-level
       authorization is enforced by polkit at call time, not by
       D-Bus policy. See spec §5.1. -->
  <policy context="default">
    <allow send_destination="app.boxpilot.Helper"
           send_interface="app.boxpilot.Helper1"/>
    <allow send_destination="app.boxpilot.Helper"
           send_interface="org.freedesktop.DBus.Introspectable"/>
    <allow send_destination="app.boxpilot.Helper"
           send_interface="org.freedesktop.DBus.Properties"/>
    <allow send_destination="app.boxpilot.Helper"
           send_interface="org.freedesktop.DBus.Peer"/>
  </policy>
</busconfig>
```

- [ ] **Step 2: Validate XML well-formedness**

Run: `xmllint --noout packaging/linux/dbus/system.d/app.boxpilot.helper.conf`
Expected: exit 0.

- [ ] **Step 3: Commit**

```bash
git add packaging/linux/dbus/system.d/app.boxpilot.helper.conf
git commit -m "feat(packaging): D-Bus system-bus access policy for app.boxpilot.Helper"
```

---

## Task 19: D-Bus system-service activation file

**Files:**
- Create: `packaging/linux/dbus/system-services/app.boxpilot.Helper.service`

- [ ] **Step 1: Write the file**

```ini
[D-BUS Service]
Name=app.boxpilot.Helper
Exec=/usr/lib/boxpilot/boxpilotd
User=root
SystemdService=boxpilotd.service
```

The `SystemdService=` line lets D-Bus delegate startup to systemd, which is the recommended modern path; the unit itself ships with the `.deb` (plan #9). Until plan #9, the dev `Makefile` (task 27) installs `boxpilotd` to `/usr/lib/boxpilot/boxpilotd` and relies on the `Exec=` line for direct activation.

- [ ] **Step 2: Commit**

```bash
git add packaging/linux/dbus/system-services/app.boxpilot.Helper.service
git commit -m "feat(packaging): D-Bus system-service activation file"
```

---

## Task 20: polkit XML actions

**Files:**
- Create: `packaging/linux/polkit-1/actions/app.boxpilot.helper.policy`

- [ ] **Step 1: Write the file**

```xml
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE policyconfig PUBLIC
 "-//freedesktop//DTD PolicyKit Policy Configuration 1.0//EN"
 "https://www.freedesktop.org/standards/PolicyKit/1/policyconfig.dtd">
<policyconfig>
  <vendor>BoxPilot</vendor>
  <vendor_url>https://boxpilot.app/</vendor_url>

  <!-- Defaults below are the *non-controller* tiers from spec §6.3.
       The controller user is promoted to a less-prompting tier by the
       JS rules file 49-boxpilot.rules. -->

  <!-- Read-only / observation: any local user, no auth. -->
  <action id="app.boxpilot.helper.service.status">
    <description>View BoxPilot service status</description>
    <message>Authentication is required to view BoxPilot service status</message>
    <defaults>
      <allow_any>yes</allow_any>
      <allow_inactive>yes</allow_inactive>
      <allow_active>yes</allow_active>
    </defaults>
  </action>
  <action id="app.boxpilot.helper.service.logs">
    <description>Read BoxPilot service logs</description>
    <message>Authentication is required to read BoxPilot service logs</message>
    <defaults><allow_any>yes</allow_any><allow_inactive>yes</allow_inactive><allow_active>yes</allow_active></defaults>
  </action>
  <action id="app.boxpilot.helper.core.discover">
    <description>Discover sing-box cores on this system</description>
    <message>Authentication is required to discover sing-box cores</message>
    <defaults><allow_any>yes</allow_any><allow_inactive>yes</allow_inactive><allow_active>yes</allow_active></defaults>
  </action>
  <action id="app.boxpilot.helper.legacy.observe-service">
    <description>Observe an existing sing-box service</description>
    <message>Authentication is required to observe an existing sing-box service</message>
    <defaults><allow_any>yes</allow_any><allow_inactive>yes</allow_inactive><allow_active>yes</allow_active></defaults>
  </action>

  <!-- Mutating: admin auth, cached for the session. -->
  <action id="app.boxpilot.helper.service.start">
    <description>Start the BoxPilot sing-box service</description>
    <message>Authentication is required to start BoxPilot</message>
    <defaults><allow_any>auth_admin_keep</allow_any><allow_inactive>auth_admin_keep</allow_inactive><allow_active>auth_admin_keep</allow_active></defaults>
  </action>
  <action id="app.boxpilot.helper.service.stop">
    <description>Stop the BoxPilot sing-box service</description>
    <message>Authentication is required to stop BoxPilot</message>
    <defaults><allow_any>auth_admin_keep</allow_any><allow_inactive>auth_admin_keep</allow_inactive><allow_active>auth_admin_keep</allow_active></defaults>
  </action>
  <action id="app.boxpilot.helper.service.restart">
    <description>Restart the BoxPilot sing-box service</description>
    <message>Authentication is required to restart BoxPilot</message>
    <defaults><allow_any>auth_admin_keep</allow_any><allow_inactive>auth_admin_keep</allow_inactive><allow_active>auth_admin_keep</allow_active></defaults>
  </action>
  <action id="app.boxpilot.helper.service.enable">
    <description>Enable the BoxPilot sing-box service at boot</description>
    <message>Authentication is required to enable BoxPilot at boot</message>
    <defaults><allow_any>auth_admin_keep</allow_any><allow_inactive>auth_admin_keep</allow_inactive><allow_active>auth_admin_keep</allow_active></defaults>
  </action>
  <action id="app.boxpilot.helper.service.disable">
    <description>Disable the BoxPilot sing-box service at boot</description>
    <message>Authentication is required to disable BoxPilot at boot</message>
    <defaults><allow_any>auth_admin_keep</allow_any><allow_inactive>auth_admin_keep</allow_inactive><allow_active>auth_admin_keep</allow_active></defaults>
  </action>
  <action id="app.boxpilot.helper.service.install-managed">
    <description>Install the BoxPilot-managed sing-box service unit</description>
    <message>Authentication is required to install the BoxPilot service</message>
    <defaults><allow_any>auth_admin_keep</allow_any><allow_inactive>auth_admin_keep</allow_inactive><allow_active>auth_admin_keep</allow_active></defaults>
  </action>
  <action id="app.boxpilot.helper.profile.activate-bundle">
    <description>Activate a BoxPilot profile</description>
    <message>Authentication is required to activate this profile</message>
    <defaults><allow_any>auth_admin_keep</allow_any><allow_inactive>auth_admin_keep</allow_inactive><allow_active>auth_admin_keep</allow_active></defaults>
  </action>
  <action id="app.boxpilot.helper.profile.rollback-release">
    <description>Roll back to a previous BoxPilot profile release</description>
    <message>Authentication is required to roll back this profile</message>
    <defaults><allow_any>auth_admin_keep</allow_any><allow_inactive>auth_admin_keep</allow_inactive><allow_active>auth_admin_keep</allow_active></defaults>
  </action>
  <action id="app.boxpilot.helper.core.install-managed">
    <description>Install a managed sing-box core</description>
    <message>Authentication is required to install a sing-box core</message>
    <defaults><allow_any>auth_admin_keep</allow_any><allow_inactive>auth_admin_keep</allow_inactive><allow_active>auth_admin_keep</allow_active></defaults>
  </action>
  <action id="app.boxpilot.helper.core.upgrade-managed">
    <description>Upgrade the managed sing-box core</description>
    <message>Authentication is required to upgrade the sing-box core</message>
    <defaults><allow_any>auth_admin_keep</allow_any><allow_inactive>auth_admin_keep</allow_inactive><allow_active>auth_admin_keep</allow_active></defaults>
  </action>
  <action id="app.boxpilot.helper.core.rollback-managed">
    <description>Roll back the managed sing-box core</description>
    <message>Authentication is required to roll back the sing-box core</message>
    <defaults><allow_any>auth_admin_keep</allow_any><allow_inactive>auth_admin_keep</allow_inactive><allow_active>auth_admin_keep</allow_active></defaults>
  </action>
  <action id="app.boxpilot.helper.core.adopt">
    <description>Adopt an existing sing-box binary as managed</description>
    <message>Authentication is required to adopt this sing-box binary</message>
    <defaults><allow_any>auth_admin_keep</allow_any><allow_inactive>auth_admin_keep</allow_inactive><allow_active>auth_admin_keep</allow_active></defaults>
  </action>
  <action id="app.boxpilot.helper.diagnostics.export-redacted">
    <description>Export redacted BoxPilot diagnostics</description>
    <message>Authentication is required to export BoxPilot diagnostics</message>
    <defaults><allow_any>auth_admin_keep</allow_any><allow_inactive>auth_admin_keep</allow_inactive><allow_active>auth_admin_keep</allow_active></defaults>
  </action>

  <!-- High-risk: admin auth, never cached (§6.3 “always re-prompt; no caching”). -->
  <action id="app.boxpilot.helper.controller.transfer">
    <description>Transfer the BoxPilot controller user</description>
    <message>Authentication is required to transfer the BoxPilot controller</message>
    <defaults><allow_any>auth_admin</allow_any><allow_inactive>auth_admin</allow_inactive><allow_active>auth_admin</allow_active></defaults>
  </action>
  <action id="app.boxpilot.helper.legacy.migrate-service">
    <description>Migrate an existing sing-box service to BoxPilot</description>
    <message>Authentication is required to migrate the existing sing-box service</message>
    <defaults><allow_any>auth_admin</allow_any><allow_inactive>auth_admin</allow_inactive><allow_active>auth_admin</allow_active></defaults>
  </action>
</policyconfig>
```

- [ ] **Step 2: Validate XML well-formedness**

Run: `xmllint --noout packaging/linux/polkit-1/actions/app.boxpilot.helper.policy`
Expected: exit 0.

- [ ] **Step 3: Verify every HelperMethod has an action ID**

This guards against drift between the Rust enum and the XML. Add to `crates/boxpilot-ipc/tests/policy_drift.rs`:

```rust
use boxpilot_ipc::HelperMethod;
use std::collections::HashSet;

#[test]
fn every_helper_method_has_a_polkit_action_id_in_the_xml() {
    let xml = std::fs::read_to_string(
        concat!(env!("CARGO_MANIFEST_DIR"), "/../../packaging/linux/polkit-1/actions/app.boxpilot.helper.policy"),
    )
    .expect("read policy XML");

    let mut declared: HashSet<String> = HashSet::new();
    for line in xml.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("<action id=\"") {
            if let Some(end) = rest.find('"') {
                declared.insert(rest[..end].to_string());
            }
        }
    }

    for m in HelperMethod::ALL {
        let id = m.polkit_action_id();
        assert!(
            declared.contains(&id),
            "polkit policy XML is missing action {id} (HelperMethod::{m:?})"
        );
    }
    assert_eq!(declared.len(), HelperMethod::ALL.len(), "extra action IDs in policy XML");
}
```

Run: `cargo test -p boxpilot-ipc --test policy_drift`
Expected: 1 test passes.

- [ ] **Step 4: Commit**

```bash
git add packaging/linux/polkit-1/actions/app.boxpilot.helper.policy crates/boxpilot-ipc/tests/policy_drift.rs
git commit -m "feat(packaging): polkit policy with all 19 actions + drift test"
```

---

## Task 21: polkit JS rule for controller user

**Files:**
- Create: `packaging/linux/polkit-1/rules.d/49-boxpilot.rules`

- [ ] **Step 1: Write the file**

```javascript
// 49-boxpilot.rules — promote the BoxPilot controller user to less-prompting
// auth tiers (spec §6.3). The controller UID lives in
// /etc/boxpilot/controller-uid (a single integer). When the file is missing
// or unparseable, this rule defers to the XML defaults — i.e. all callers
// are treated as non-controllers, which is the safe direction.

polkit.addRule(function(action, subject) {
    if (action.id.indexOf("app.boxpilot.helper.") !== 0) {
        return;
    }

    var controllerUid = null;
    try {
        // polkit.spawn is synchronous and blocks the polkit daemon, so we
        // keep the spawned command minimal: `cat` of a small integer file.
        var raw = polkit.spawn(["/usr/bin/cat", "/etc/boxpilot/controller-uid"]);
        var trimmed = raw.replace(/[^0-9]/g, "");
        if (trimmed.length > 0) {
            controllerUid = parseInt(trimmed, 10);
        }
    } catch (e) {
        // File missing or cat failed — fall through to XML defaults.
        return;
    }

    if (controllerUid === null || isNaN(controllerUid)) {
        return;
    }
    if (subject.user === undefined || subject.user === null) {
        return;
    }
    if (subject.user.uid !== controllerUid) {
        // Non-controller: keep XML defaults.
        return;
    }

    // Controller path. Map the action's authorization class:
    //   read-only → YES (no prompt)
    //   high-risk → AUTH_ADMIN (always prompt; no cache)
    //   mutating  → AUTH_SELF_KEEP (controller proves identity, cached)
    var id = action.id;

    // Read-only set
    if (id === "app.boxpilot.helper.service.status" ||
        id === "app.boxpilot.helper.service.logs" ||
        id === "app.boxpilot.helper.core.discover" ||
        id === "app.boxpilot.helper.legacy.observe-service") {
        return polkit.Result.YES;
    }

    // High-risk set
    if (id === "app.boxpilot.helper.controller.transfer" ||
        id === "app.boxpilot.helper.legacy.migrate-service") {
        return polkit.Result.AUTH_ADMIN;
    }

    // Everything else under our namespace is mutating.
    return polkit.Result.AUTH_SELF_KEEP;
});
```

- [ ] **Step 2: Verify the rule loads (lint via duplicate file in /tmp + polkit syntax)**

There is no offline polkit-rule linter. Sanity-check the file by:

Run: `node --check packaging/linux/polkit-1/rules.d/49-boxpilot.rules || true`

The `polkit.*` symbols won't resolve under bare Node, so `node --check` will warn but at least catches syntax errors. The real verification is task 28's manual smoke test.

- [ ] **Step 3: Commit**

```bash
git add packaging/linux/polkit-1/rules.d/49-boxpilot.rules
git commit -m "feat(packaging): polkit JS rule promoting controller user (§6.3)"
```

---

## Task 22: Tauri 2 backend crate

**Files:**
- Create: `crates/boxpilot-tauri/Cargo.toml`
- Create: `crates/boxpilot-tauri/build.rs`
- Create: `crates/boxpilot-tauri/tauri.conf.json`
- Create: `crates/boxpilot-tauri/src/main.rs`
- Create: `crates/boxpilot-tauri/src/lib.rs`
- Create: `crates/boxpilot-tauri/capabilities/default.json`

- [ ] **Step 1: Write `Cargo.toml`**

```toml
[package]
name = "boxpilot"
version = "0.1.0"
edition.workspace = true
rust-version.workspace = true
license.workspace = true

[lib]
name = "boxpilot"
crate-type = ["staticlib", "cdylib", "rlib"]

[[bin]]
name = "boxpilot"
path = "src/main.rs"

[build-dependencies]
tauri-build = { version = "2", features = [] }

[dependencies]
boxpilot-ipc = { path = "../boxpilot-ipc" }
serde.workspace = true
serde_json.workspace = true
thiserror.workspace = true
tracing.workspace = true
tracing-subscriber.workspace = true
tauri = { version = "2", features = [] }
zbus = { version = "5", default-features = false, features = ["tokio"] }
tokio = { workspace = true }
```

- [ ] **Step 2: Write `build.rs`**

```rust
fn main() {
    tauri_build::build();
}
```

- [ ] **Step 3: Write `tauri.conf.json`**

```json
{
  "$schema": "https://schema.tauri.app/config/2",
  "productName": "BoxPilot",
  "version": "0.1.0",
  "identifier": "app.boxpilot",
  "build": {
    "beforeDevCommand": "npm --prefix ../../frontend run dev",
    "beforeBuildCommand": "npm --prefix ../../frontend run build",
    "devUrl": "http://localhost:1420",
    "frontendDist": "../../frontend/dist"
  },
  "app": {
    "windows": [
      {
        "title": "BoxPilot",
        "width": 1024,
        "height": 720,
        "resizable": true,
        "fullscreen": false
      }
    ],
    "security": {
      "csp": null
    }
  },
  "bundle": {
    "active": false,
    "targets": "all",
    "category": "Utility"
  }
}
```

- [ ] **Step 4: Write `capabilities/default.json`**

```json
{
  "$schema": "https://schema.tauri.app/capabilities/2",
  "identifier": "default",
  "description": "BoxPilot default capability set",
  "windows": ["main"],
  "permissions": [
    "core:default"
  ]
}
```

- [ ] **Step 5: Write `src/lib.rs` and `src/main.rs`**

`src/lib.rs`:

```rust
pub mod helper_client;
pub mod commands;

pub fn run() {
    init_tracing();
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            commands::helper_service_status,
            commands::helper_ping,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

fn init_tracing() {
    use tracing_subscriber::{fmt, EnvFilter};
    let filter = EnvFilter::try_from_env("BOXPILOT_LOG")
        .unwrap_or_else(|_| EnvFilter::new("boxpilot=info"));
    let _ = fmt().with_env_filter(filter).with_target(false).try_init();
}
```

`src/main.rs`:

```rust
fn main() {
    boxpilot::run();
}
```

- [ ] **Step 6: Verify it parses (no frontend yet, so cargo build fails on resource bundling — use `cargo check` until frontend exists)**

Run: `cargo check -p boxpilot`
Expected: builds dependency graph; tauri-build emits a warning about missing `frontendDist` (acceptable until task 25).

- [ ] **Step 7: Commit**

```bash
git add crates/boxpilot-tauri
git commit -m "feat(tauri): boxpilot crate skeleton with capabilities"
```

---

## Task 23: Tauri helper-client + commands

**Files:**
- Create: `crates/boxpilot-tauri/src/helper_client.rs`
- Create: `crates/boxpilot-tauri/src/commands.rs`

- [ ] **Step 1: Write `helper_client.rs`**

```rust
//! Tauri-side D-Bus client. Calls `app.boxpilot.Helper1` as the running GUI
//! user and surfaces the typed JSON response back to Vue.

use boxpilot_ipc::ServiceStatusResponse;
use thiserror::Error;
use zbus::{proxy, Connection};

#[proxy(
    interface = "app.boxpilot.Helper1",
    default_service = "app.boxpilot.Helper",
    default_path = "/app/boxpilot/Helper"
)]
trait Helper {
    fn service_status(&self) -> zbus::Result<String>;
}

#[derive(Debug, Error)]
pub enum ClientError {
    #[error("connect to system bus: {0}")]
    Connect(#[from] zbus::Error),
    #[error("decode response: {0}")]
    Decode(String),
}

pub struct HelperClient {
    conn: Connection,
}

impl HelperClient {
    pub async fn connect() -> Result<Self, ClientError> {
        Ok(Self { conn: Connection::system().await? })
    }

    pub async fn service_status(&self) -> Result<ServiceStatusResponse, ClientError> {
        let proxy = HelperProxy::new(&self.conn).await?;
        let json = proxy.service_status().await?;
        serde_json::from_str(&json).map_err(|e| ClientError::Decode(e.to_string()))
    }
}
```

- [ ] **Step 2: Write `commands.rs`**

```rust
//! Tauri commands invoked from the Vue frontend via `invoke()`.

use crate::helper_client::HelperClient;
use boxpilot_ipc::ServiceStatusResponse;
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct CommandError {
    pub code: String,
    pub message: String,
}

impl<E: std::fmt::Display> From<E> for CommandError {
    fn from(e: E) -> Self {
        let s = e.to_string();
        // Split "app.boxpilot.Helper1.X: msg" into code/message if present.
        if let Some(rest) = s.strip_prefix("app.boxpilot.Helper1.") {
            if let Some((code, msg)) = rest.split_once(": ") {
                return CommandError { code: code.into(), message: msg.into() };
            }
        }
        CommandError { code: "ipc".into(), message: s }
    }
}

#[tauri::command]
pub async fn helper_service_status() -> Result<ServiceStatusResponse, CommandError> {
    let client = HelperClient::connect().await?;
    Ok(client.service_status().await?)
}

#[tauri::command]
pub async fn helper_ping() -> Result<&'static str, CommandError> {
    let _client = HelperClient::connect().await?;
    Ok("ok")
}
```

- [ ] **Step 3: Build**

Run: `cargo check -p boxpilot`
Expected: clean.

- [ ] **Step 4: Commit**

```bash
git add crates/boxpilot-tauri/src/helper_client.rs crates/boxpilot-tauri/src/commands.rs
git commit -m "feat(tauri): helper-client + commands for service.status round-trip"
```

---

## Task 24: Vue 3 + Vite frontend skeleton

**Files:**
- Create: `frontend/package.json`
- Create: `frontend/tsconfig.json`
- Create: `frontend/tsconfig.node.json`
- Create: `frontend/vite.config.ts`
- Create: `frontend/index.html`
- Create: `frontend/src/main.ts`
- Create: `frontend/src/App.vue`

- [ ] **Step 1: Write `package.json`**

```json
{
  "name": "boxpilot-frontend",
  "private": true,
  "version": "0.1.0",
  "type": "module",
  "scripts": {
    "dev": "vite",
    "build": "vue-tsc -b && vite build",
    "preview": "vite preview"
  },
  "dependencies": {
    "@tauri-apps/api": "^2",
    "vue": "^3.5"
  },
  "devDependencies": {
    "@vitejs/plugin-vue": "^5",
    "typescript": "^5.4",
    "vite": "^5",
    "vue-tsc": "^2"
  }
}
```

- [ ] **Step 2: Write `tsconfig.json`**

```json
{
  "compilerOptions": {
    "target": "ES2022",
    "module": "ESNext",
    "moduleResolution": "Bundler",
    "strict": true,
    "jsx": "preserve",
    "isolatedModules": true,
    "esModuleInterop": true,
    "skipLibCheck": true,
    "lib": ["ES2022", "DOM"],
    "types": ["vite/client"],
    "resolveJsonModule": true,
    "allowImportingTsExtensions": false,
    "useDefineForClassFields": true
  },
  "include": ["src/**/*.ts", "src/**/*.vue"],
  "references": [{ "path": "./tsconfig.node.json" }]
}
```

- [ ] **Step 3: Write `tsconfig.node.json`**

```json
{
  "compilerOptions": {
    "composite": true,
    "module": "ESNext",
    "moduleResolution": "Bundler",
    "target": "ES2022",
    "skipLibCheck": true,
    "types": ["node"]
  },
  "include": ["vite.config.ts"]
}
```

- [ ] **Step 4: Write `vite.config.ts`**

```ts
import { defineConfig } from "vite";
import vue from "@vitejs/plugin-vue";

export default defineConfig({
  plugins: [vue()],
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
  },
  envPrefix: ["VITE_", "TAURI_ENV_*"],
});
```

- [ ] **Step 5: Write `index.html`**

```html
<!doctype html>
<html lang="en">
  <head>
    <meta charset="UTF-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1.0" />
    <title>BoxPilot</title>
  </head>
  <body>
    <div id="app"></div>
    <script type="module" src="/src/main.ts"></script>
  </body>
</html>
```

- [ ] **Step 6: Write `src/main.ts`**

```ts
import { createApp } from "vue";
import App from "./App.vue";

createApp(App).mount("#app");
```

- [ ] **Step 7: Write `src/App.vue` (placeholder, real impl in task 26)**

```vue
<script setup lang="ts">
const greeting = "BoxPilot — skeleton";
</script>

<template>
  <main>
    <h1>{{ greeting }}</h1>
    <p>Plan #1 placeholder. The status panel lands in the next task.</p>
  </main>
</template>

<style>
body { font-family: system-ui, sans-serif; padding: 2rem; }
</style>
```

- [ ] **Step 8: Install + build**

Run: `npm --prefix frontend install && npm --prefix frontend run build`
Expected: `dist/` populated with `index.html`, `assets/*`. No TypeScript errors.

- [ ] **Step 9: Commit**

```bash
git add frontend
git commit -m "feat(frontend): Vue 3 + Vite skeleton"
```

---

## Task 25: Frontend API wrapper + status panel

**Files:**
- Create: `frontend/src/api/types.ts`
- Create: `frontend/src/api/helper.ts`
- Modify: `frontend/src/App.vue`

- [ ] **Step 1: Write `api/types.ts` (mirrors `boxpilot-ipc::response`)**

```ts
export type UnitState =
  | { kind: "not_found" }
  | {
      kind: "known";
      active_state: string;
      sub_state: string;
      load_state: string;
      n_restarts: number;
      exec_main_status: number;
    };

export type ControllerStatus =
  | { kind: "unset" }
  | { kind: "set"; uid: number; username: string }
  | { kind: "orphaned"; uid: number };

export interface ServiceStatusResponse {
  unit_name: string;
  unit_state: UnitState;
  controller: ControllerStatus;
}

export interface CommandError {
  code: string;
  message: string;
}
```

- [ ] **Step 2: Write `api/helper.ts`**

```ts
import { invoke } from "@tauri-apps/api/core";
import type { ServiceStatusResponse, CommandError } from "./types";

export async function serviceStatus(): Promise<ServiceStatusResponse> {
  return await invoke<ServiceStatusResponse>("helper_service_status");
}

export async function ping(): Promise<string> {
  return await invoke<string>("helper_ping");
}

export function isCommandError(e: unknown): e is CommandError {
  return (
    typeof e === "object" &&
    e !== null &&
    "code" in (e as Record<string, unknown>) &&
    "message" in (e as Record<string, unknown>)
  );
}
```

- [ ] **Step 3: Replace `src/App.vue`**

```vue
<script setup lang="ts">
import { ref } from "vue";
import { serviceStatus, isCommandError } from "./api/helper";
import type { ServiceStatusResponse } from "./api/types";

const loading = ref(false);
const status = ref<ServiceStatusResponse | null>(null);
const error = ref<{ code: string; message: string } | null>(null);

async function check() {
  loading.value = true;
  error.value = null;
  try {
    status.value = await serviceStatus();
  } catch (e) {
    if (isCommandError(e)) {
      error.value = e;
    } else {
      error.value = { code: "unknown", message: String(e) };
    }
    status.value = null;
  } finally {
    loading.value = false;
  }
}
</script>

<template>
  <main>
    <h1>BoxPilot</h1>
    <p>Plan #1 — helper round-trip smoke test.</p>
    <button :disabled="loading" @click="check">
      {{ loading ? "Checking..." : "Check service.status" }}
    </button>

    <section v-if="error" class="err">
      <h2>Error</h2>
      <code>{{ error.code }}</code>
      <p>{{ error.message }}</p>
    </section>

    <section v-if="status" class="ok">
      <h2>Service: {{ status.unit_name }}</h2>
      <pre>{{ JSON.stringify(status, null, 2) }}</pre>
    </section>
  </main>
</template>

<style>
body { font-family: system-ui, sans-serif; padding: 2rem; max-width: 60rem; margin: auto; }
button { padding: 0.5rem 1rem; font-size: 1rem; }
section.err { margin-top: 1.5rem; padding: 1rem; background: #fee; border-radius: 0.5rem; }
section.ok { margin-top: 1.5rem; padding: 1rem; background: #efe; border-radius: 0.5rem; }
pre { white-space: pre-wrap; }
</style>
```

- [ ] **Step 4: Build**

Run: `npm --prefix frontend run build`
Expected: clean build, no TS errors.

- [ ] **Step 5: Commit**

```bash
git add frontend/src
git commit -m "feat(frontend): helper API wrapper + service.status panel"
```

---

## Task 26: Dev install Makefile

**Files:**
- Create: `Makefile`

- [ ] **Step 1: Write `Makefile`**

```makefile
# Dev-install BoxPilot's privileged side onto the local machine. Plan #9
# replaces this with proper .deb postinst/prerm scripts.

PREFIX        ?= /usr
DBUS_SYS_DIR  ?= $(PREFIX)/share/dbus-1
POLKIT_DIR    ?= $(PREFIX)/share/polkit-1
LIB_DIR       ?= $(PREFIX)/lib/boxpilot
BIN_DIR       ?= $(PREFIX)/bin
ETC_DIR       ?= /etc/boxpilot

CARGO         ?= cargo
INSTALL       ?= install

.PHONY: build-helper install-helper uninstall-helper run-gui

build-helper:
	$(CARGO) build --release -p boxpilotd

install-helper: build-helper
	$(INSTALL) -d -m 0755 $(LIB_DIR)
	$(INSTALL) -D -m 0755 target/release/boxpilotd $(LIB_DIR)/boxpilotd
	$(INSTALL) -D -m 0644 packaging/linux/dbus/system-services/app.boxpilot.Helper.service \
	    $(DBUS_SYS_DIR)/system-services/app.boxpilot.Helper.service
	$(INSTALL) -D -m 0644 packaging/linux/dbus/system.d/app.boxpilot.helper.conf \
	    $(DBUS_SYS_DIR)/system.d/app.boxpilot.helper.conf
	$(INSTALL) -D -m 0644 packaging/linux/polkit-1/actions/app.boxpilot.helper.policy \
	    $(POLKIT_DIR)/actions/app.boxpilot.helper.policy
	$(INSTALL) -D -m 0644 packaging/linux/polkit-1/rules.d/49-boxpilot.rules \
	    $(POLKIT_DIR)/rules.d/49-boxpilot.rules
	$(INSTALL) -d -m 0755 $(ETC_DIR)
	# /etc/boxpilot/controller-uid is left absent on dev installs; the polkit
	# JS rule treats absence as "no controller, fall through to defaults".
	systemctl reload dbus.service || systemctl restart dbus.service

uninstall-helper:
	rm -f $(LIB_DIR)/boxpilotd
	rm -f $(DBUS_SYS_DIR)/system-services/app.boxpilot.Helper.service
	rm -f $(DBUS_SYS_DIR)/system.d/app.boxpilot.helper.conf
	rm -f $(POLKIT_DIR)/actions/app.boxpilot.helper.policy
	rm -f $(POLKIT_DIR)/rules.d/49-boxpilot.rules
	systemctl reload dbus.service || systemctl restart dbus.service

run-gui:
	cd crates/boxpilot-tauri && cargo tauri dev
```

- [ ] **Step 2: Commit**

```bash
git add Makefile
git commit -m "chore: dev install/uninstall Makefile"
```

---

## Task 27: Manual integration smoke verification (documented procedure)

**Files:**
- Create: `docs/superpowers/plans/2026-04-27-skeleton-smoke-procedure.md`

- [ ] **Step 1: Write the procedure**

```markdown
# Plan #1 manual smoke procedure

This isn't an automated test — system-bus + polkit testing requires a real
desktop session. Run this after task 26 completes, on a Debian/Ubuntu system
with a graphical login active.

## 1. Install the privileged side

```bash
cargo build --release -p boxpilotd
sudo make install-helper
```

Verify the files landed:

```bash
ls /usr/lib/boxpilot/boxpilotd
ls /usr/share/dbus-1/system-services/app.boxpilot.Helper.service
ls /usr/share/dbus-1/system.d/app.boxpilot.helper.conf
ls /usr/share/polkit-1/actions/app.boxpilot.helper.policy
ls /usr/share/polkit-1/rules.d/49-boxpilot.rules
```

## 2. Verify the bus picks up the service file

```bash
gdbus introspect --system --dest app.boxpilot.Helper --object-path /app/boxpilot/Helper
```

Expected: D-Bus auto-activates `boxpilotd`, the introspection reply includes
the `app.boxpilot.Helper1` interface with `ServiceStatus`, `ServiceStart`, …
(19 methods total).

## 3. Verify polkit actions are registered

```bash
pkaction --action-id app.boxpilot.helper.service.status
pkaction --action-id app.boxpilot.helper.profile.activate-bundle
pkaction --action-id app.boxpilot.helper.controller.transfer
```

Expected: each command returns the action description, vendor, and defaults
matching the XML.

## 4. Call ServiceStatus directly via D-Bus

```bash
gdbus call --system --dest app.boxpilot.Helper \
  --object-path /app/boxpilot/Helper \
  --method app.boxpilot.Helper1.ServiceStatus
```

Expected (clean machine, no `boxpilot-sing-box.service` installed yet):
returns a JSON string whose decoded payload is

```json
{
  "unit_name": "boxpilot-sing-box.service",
  "unit_state": { "kind": "not_found" },
  "controller": { "kind": "unset" }
}
```

No polkit prompt because `service.status` is `yes/yes/yes`.

## 5. Call a stubbed mutating method

```bash
gdbus call --system --dest app.boxpilot.Helper \
  --object-path /app/boxpilot/Helper \
  --method app.boxpilot.Helper1.ServiceStart
```

Expected: polkit admin prompt appears (because no controller is set so XML
defaults apply). After authenticating, the call still returns
`app.boxpilot.Helper1.ControllerNotSet: no controller has been initialized`
because the controller-set path doesn't exist in plan #1.

## 6. Run the GUI

```bash
make run-gui
```

Expected: a Tauri window opens. Click **Check service.status**. The status
panel populates with the same JSON as in step 4.

## 7. Tear down

```bash
sudo make uninstall-helper
```
```

- [ ] **Step 2: Commit**

```bash
git add docs/superpowers/plans/2026-04-27-skeleton-smoke-procedure.md
git commit -m "docs: plan #1 manual smoke procedure"
```

---

## Task 28: README pointing at spec + plan

**Files:**
- Create: `README.md`

- [ ] **Step 1: Write `README.md`**

```markdown
# BoxPilot

Linux desktop control panel for system-installed `sing-box`.

- **Design spec:** [`docs/superpowers/specs/2026-04-27-boxpilot-linux-design.md`](docs/superpowers/specs/2026-04-27-boxpilot-linux-design.md)
- **Plan #1 — skeleton + helperd:** [`docs/superpowers/plans/2026-04-27-boxpilot-skeleton-and-helperd.md`](docs/superpowers/plans/2026-04-27-boxpilot-skeleton-and-helperd.md)

## Status

Pre-1.0. Plan #1 establishes the workspace, the unprivileged Tauri GUI
shell, and the root D-Bus helper `boxpilotd` with its `service.status`
round-trip. All other privileged actions are stubbed; later plans (#2–#9)
fill them in.

## Layout

- `crates/boxpilot-ipc/` — shared serde types and config schema
- `crates/boxpilotd/` — root D-Bus helper (system-bus activated)
- `crates/boxpilot-tauri/` — Tauri 2 app (Rust side)
- `frontend/` — Vue 3 + TS + Vite (web side)
- `packaging/linux/` — D-Bus + polkit files (installed by `make install-helper`)

## Quick start (dev)

```bash
cargo build --release -p boxpilotd
sudo make install-helper
make run-gui
```

After clicking **Check service.status**, the panel shows the JSON returned
by `app.boxpilot.Helper1.ServiceStatus` — `unit_state.kind: not_found`
until plan #3 generates the unit.

## Building from source

Requires: Rust 1.78+, Node 20+, `polkit-daemon`, `dbus-daemon`, a polkit
authentication agent (any modern desktop ships one).

## License

GPL-3.0-or-later.
```

- [ ] **Step 2: Commit**

```bash
git add README.md
git commit -m "docs: project README"
```

---

## Task 29: Workspace-wide lint + test gate

**Files:**
- Create: `.github/workflows/ci.yml` (skipped if user prefers no GitHub CI; in that case create `scripts/check.sh` instead)

- [ ] **Step 1: Write `scripts/check.sh`**

```bash
#!/usr/bin/env bash
# Local CI gate. Run before every PR.
set -euo pipefail

cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
( cd frontend && npm ci && npm run build )
xmllint --noout packaging/linux/dbus/system.d/app.boxpilot.helper.conf
xmllint --noout packaging/linux/polkit-1/actions/app.boxpilot.helper.policy
echo "All checks passed."
```

Make it executable:

Run: `chmod +x scripts/check.sh`

- [ ] **Step 2: Run it**

Run: `./scripts/check.sh`
Expected: exits 0 with `All checks passed.`

- [ ] **Step 3: Commit**

```bash
git add scripts/check.sh
git commit -m "chore: local check.sh gate (fmt + clippy + tests + xmllint + frontend build)"
```

---

## Task 30: Plan-completion checkpoint

- [ ] **Step 1: Verify the deliverable from the plan goal**

Re-read the goal at the top of this file. Plan #1 is complete when:

1. `cargo test --workspace` is green.
2. `./scripts/check.sh` is green.
3. The manual smoke procedure (task 27) passes through step 6 — Tauri window opens, "Check service.status" returns the JSON envelope, and `gdbus call …ServiceStart` returns `ControllerNotSet`.

- [ ] **Step 2: Decide handoff**

Three follow-ups on the path to v1.0:
- Plan #2 — managed core lifecycle (depends on this plan's helper contract).
- Plan #3 — managed systemd service (depends on plan #2's `core.discover`/`core.install_managed`).
- Plan #4 — user-side profile store (no helper dependency; can run in parallel with #2/#3).

If executor agrees, mark this plan complete and merge the worktree branch.

- [ ] **Step 3: Final commit (if anything still uncommitted)**

```bash
git status
# nothing should remain; if a stray edit is here, fold it in:
# git add … && git commit -m "chore: cleanup"
```

---

## Self-Review (executed against the spec)

Spec coverage:

| Spec section | Plan #1 coverage |
|---|---|
| §1 Stack — Tauri 2, Vue 3 + TS + Vite, Rust, polkit-guarded `boxpilotd` | Tasks 1, 8, 22, 24 |
| §5.1 Application package files (paths) | Task 26 (`Makefile`) |
| §5.3 `boxpilot.toml` schema, schema_version rejection | Task 7 |
| §6.1 D-Bus activation, polkit-guarded, no GUI-as-root, identity from connection creds | Tasks 11, 17, 18, 19 |
| §6.2 Controller user model | Tasks 10, 14, 15 |
| §6.3 Action whitelist, polkit action ID mapping, auth classes | Tasks 3, 4, 16, 20, 21 |
| §6.4 `/run/boxpilot/lock` with flock(2) | Tasks 9, 15 |
| §6.5 Trusted executable paths | **Deferred to plan #2** (no managed core in this plan, so no path to gate yet). Noted explicitly in plan #2's preconditions. |
| §6.6 Controller initialization, transfer, `controller_orphaned` | State machine: tasks 10, 15. Initialization triggers on first authorized mutating call — no such call lands in plan #1, so the bit-flip path is left for plan #2's `core.install_managed` task. |
| §7 Managed systemd service | **Deferred to plan #3.** |
| §8 Existing `sing-box.service` handling | **Deferred to plan #6.** |
| §9 Profile bundle model | **Deferred to plans #4 (user side) / #5 (system side).** |
| §10 Activation, rollback, GC | **Deferred to plan #5.** |
| §11 Core management | **Deferred to plan #2.** |
| §12 Runtime / Clash-like API | **Deferred to plan #3.** |
| §13 Drift detection | Plan #1 surfaces `controller_orphaned` in `service.status` (task 16), which is one drift signal listed in §13. Full §13 ships with plan #3. |
| §14 Security and privacy | This plan ships secret-free wire types and never logs request bodies (helper interface methods are `#[instrument(skip(self, header))]`). The schema-aware redaction walker (full §14) is plan #8. |
| §15 Packaging | Dev install via `Makefile` (task 26). `.deb` is plan #9. |
| §16 Acceptance criteria | Items 1, 2 (no arbitrary commands/paths), 12 (Home displays runtime truth) are partially in this plan; remaining items track to later plans. |

Placeholder scan: re-read each task's code blocks. No `TBD`, no "implement appropriately", no bare "write tests" — every test step has its assertions inline. Type names are consistent: `HelperMethod`, `HelperError`, `HelperResult`, `ServiceStatusResponse`, `UnitState`, `ControllerStatus`, `ControllerState`, `Authority`, `SystemdQuery`, `CallerResolver`, `UserLookup`, `Paths`, `HelperContext`, `LockGuard`, `BUS_NAME`, `OBJECT_PATH` — used identically wherever they appear.

Type/method consistency:

- `HelperMethod::polkit_action_id` used in tasks 4, 15, 16, 20 — same return type (`String`), same dash convention.
- `Paths::run_lock`, `boxpilot_toml`, `controller_uid_file`, `etc_dir` defined in task 8 and consumed in tasks 14, 15, 26 — all match.
- `dispatch::authorize` signature in task 15 (`async fn authorize(ctx: &HelperContext, sender_bus_name: &str, method: HelperMethod) -> HelperResult<AuthorizedCall>`) is what task 16's `do_service_status` calls.
- `to_zbus_err` in task 16 uses error-name strings that the frontend `CommandError::from` in task 23 splits on `: ` — round-trip preserves `code` field.

No drift detected. Plan is ready.

---

## Execution Handoff

Plan complete and saved to `docs/superpowers/plans/2026-04-27-boxpilot-skeleton-and-helperd.md`.

Two execution options:

1. **Subagent-Driven (recommended)** — fresh subagent per task, review between tasks, fast iteration.
2. **Inline Execution** — execute tasks in this session using `superpowers:executing-plans`, batched with checkpoints.
