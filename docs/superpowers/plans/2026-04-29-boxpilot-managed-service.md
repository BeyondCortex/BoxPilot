# BoxPilot Managed Service Implementation Plan (Plan #3)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement the seven `service.*` actions from the §6.3 helper whitelist (`service.start` / `service.stop` / `service.restart` / `service.enable` / `service.disable` / `service.install_managed` / `service.logs`) so BoxPilot can generate, install, control, and tail logs of the `boxpilot-sing-box.service` unit; replace the synchronous `polkit.spawn(["/usr/bin/cat", …])` in `49-boxpilot.rules` with a daemon-managed polkit drop-in that exposes `var BOXPILOT_CONTROLLER`.

**Architecture:** Build on plan #1's `dispatch::authorize` chokepoint and plan #2's `core::commit::StateCommit` without changing their public shapes. Helper-side code lives under `crates/boxpilotd/src/service/` (4 modules: `unit`, `install`, `control`, `logs`). The systemd D-Bus trait grows mutating verbs (`start_unit`, `stop_unit`, `restart_unit`, `enable_unit_files`, `disable_unit_files`, `reload`) on top of plan #1's read-only `unit_state`. Journal reading uses a `JournalReader` trait backed by `journalctl(1)` — keeping the spawn pattern out of the polkit-hot path. `StateCommit` is extended to atomically write `/etc/polkit-1/rules.d/48-boxpilot-controller.rules` alongside `/etc/boxpilot/controller-name`, so the polkit JS rule no longer spawns `cat` per authorization. Tauri exposes 7 typed commands; frontend gets a `ServicePanel.vue` replacing the inline Home check button.

**Tech Stack:** Rust 2021, existing `zbus 5` / `tokio` / `serde` / `tracing` stack from plans #1–#2 (no new crate dependencies). Frontend Vue 3 + TS + Vite (no new deps).

**Worktree:** Branch from `main` once plan #2 is merged (`git worktree add .worktrees/managed-service -b managed-service main`). All commits below land on the `managed-service` branch.

**Out of scope (deferred):**
- Profile activation pipeline (`profile.activate_bundle` / `profile.rollback_release`) → plan #5.
- Existing `sing-box.service` observation / migration (`legacy.*`) → plan #6.
- §7.2 runtime-verification *as part of an activation transaction* → plan #5. This plan ships the building block (`service::verify::wait_for_running`) that plan #5 will compose into the 5-second window.
- Drift detection panel → plan #7 (signals from `service.status` already cover the lower layer).
- §6.3 whitelist stays at 19 methods. **Do NOT modify `boxpilot_ipc::method::HelperMethod::ALL` count.** The `policy_drift` integration test still expects 19.
- §7.1 sandbox tuning (e.g. adding `ProtectKernelTunables`) — keep the directives literally as in §7.1.

---

## File Structure

```
crates/boxpilot-ipc/src/
  service.rs                       # NEW — request/response types for the 7 service.* methods
  lib.rs                           # MODIFY — pub mod service; pub use service::*

crates/boxpilotd/src/
  service/
    mod.rs                         # NEW
    unit.rs                        # NEW — pure unit-template renderer (§7.1)
    install.rs                     # NEW — atomic write of /etc/systemd/system/<unit> + daemon-reload
    control.rs                     # NEW — start/stop/restart/enable/disable
    logs.rs                        # NEW — bounded journalctl tail
    verify.rs                      # NEW — post-restart verification helper (used here + reused by plan #5)
  systemd.rs                       # MODIFY — rename SystemdQuery -> Systemd; add start_unit / stop_unit /
                                   #          restart_unit / enable_unit_files / disable_unit_files /
                                   #          reload; add JournalReader trait + JournalctlProcess impl
  paths.rs                         # MODIFY — add systemd_unit_path() + polkit_controller_dropin_path()
  context.rs                       # MODIFY — add `journal: Arc<dyn JournalReader>` field; rename systemd field type
  iface.rs                         # MODIFY — replace 7 stubs with real bodies
  core/commit.rs                   # MODIFY — extend StateCommit to also write the polkit drop-in atomically
  main.rs                          # MODIFY — wire JournalctlProcess into HelperContext

packaging/linux/polkit-1/rules.d/
  49-boxpilot.rules                # MODIFY — read `BOXPILOT_CONTROLLER` global var; remove polkit.spawn(cat)

packaging/linux/systemd/
  boxpilot-sing-box.service.in     # NEW — reference template (mirrors src/service/unit.rs); doc-only

crates/boxpilot-tauri/src/
  helper_client.rs                 # MODIFY — 7 new zbus proxy methods
  commands.rs                      # MODIFY — 7 new #[tauri::command] wrappers
  lib.rs                           # MODIFY — register new commands in invoke_handler!

frontend/src/
  api/types.ts                     # MODIFY — TS mirrors of new request/response types
  api/helper.ts                    # MODIFY — 7 invoke wrappers
  components/ServicePanel.vue      # NEW — Start/Stop/Restart/Enable/Disable/Install + log tail
  App.vue                          # MODIFY — mount ServicePanel under the Home tab (replaces inline check button)

docs/superpowers/plans/
  2026-04-29-managed-service-smoke-procedure.md  # NEW — manual smoke run on a real systemd box
```

---

## Task 1: ServiceControl / ServiceLogs / ServiceInstall IPC types

**Files:**
- Create: `crates/boxpilot-ipc/src/service.rs`
- Modify: `crates/boxpilot-ipc/src/lib.rs`

The five control verbs (start/stop/restart/enable/disable) and `service.install_managed` all return the post-operation `UnitState` so the GUI doesn't need a second round-trip to `service.status`. `service.logs` takes a clamped line count and returns raw text lines.

- [ ] **Step 1: Write the failing test (round-trip ServiceControlResponse)**

Append to `crates/boxpilot-ipc/src/service.rs` (new file):

```rust
use serde::{Deserialize, Serialize};

use crate::response::UnitState;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServiceControlResponse {
    pub unit_state: UnitState,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServiceInstallManagedResponse {
    pub unit_state: UnitState,
    pub generated_unit_path: String,
    pub claimed_controller: bool,
}

/// `lines` is clamped to `1..=1000` by the helper before invoking journalctl.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServiceLogsRequest {
    pub lines: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServiceLogsResponse {
    pub lines: Vec<String>,
    /// True when the helper clamped the requested count down to the cap.
    pub truncated: bool,
}

pub const SERVICE_LOGS_MAX_LINES: u32 = 1000;
pub const SERVICE_LOGS_DEFAULT_LINES: u32 = 200;

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn control_response_round_trip() {
        let r = ServiceControlResponse {
            unit_state: UnitState::NotFound,
        };
        let s = serde_json::to_string(&r).unwrap();
        let back: ServiceControlResponse = serde_json::from_str(&s).unwrap();
        assert_eq!(back, r);
    }

    #[test]
    fn install_response_round_trip() {
        let r = ServiceInstallManagedResponse {
            unit_state: UnitState::NotFound,
            generated_unit_path: "/etc/systemd/system/boxpilot-sing-box.service".into(),
            claimed_controller: false,
        };
        let s = serde_json::to_string(&r).unwrap();
        let back: ServiceInstallManagedResponse = serde_json::from_str(&s).unwrap();
        assert_eq!(back, r);
    }

    #[test]
    fn logs_request_round_trip() {
        let r = ServiceLogsRequest { lines: 100 };
        let s = serde_json::to_string(&r).unwrap();
        let back: ServiceLogsRequest = serde_json::from_str(&s).unwrap();
        assert_eq!(back, r);
    }

    #[test]
    fn logs_response_round_trip() {
        let r = ServiceLogsResponse {
            lines: vec!["line1".into(), "line2".into()],
            truncated: false,
        };
        let s = serde_json::to_string(&r).unwrap();
        let back: ServiceLogsResponse = serde_json::from_str(&s).unwrap();
        assert_eq!(back, r);
    }

    #[test]
    fn cap_constants_match_spec() {
        assert_eq!(SERVICE_LOGS_MAX_LINES, 1000);
        assert!(SERVICE_LOGS_DEFAULT_LINES <= SERVICE_LOGS_MAX_LINES);
    }
}
```

- [ ] **Step 2: Re-export in `lib.rs`**

Edit `crates/boxpilot-ipc/src/lib.rs` — append after the `install_state` re-export block:

```rust
pub mod service;
pub use service::{
    ServiceControlResponse, ServiceInstallManagedResponse, ServiceLogsRequest,
    ServiceLogsResponse, SERVICE_LOGS_DEFAULT_LINES, SERVICE_LOGS_MAX_LINES,
};
```

- [ ] **Step 3: Run tests to verify**

Run: `cargo test -p boxpilot-ipc service::`
Expected: 5 tests pass (the four round-trips + cap constants).

- [ ] **Step 4: Commit**

```bash
git add crates/boxpilot-ipc/src/service.rs crates/boxpilot-ipc/src/lib.rs
git commit -m "feat(ipc): service.* request/response types"
```

---

## Task 2: Extend the systemd trait with mutating verbs (read-only test impl first)

**Files:**
- Modify: `crates/boxpilotd/src/systemd.rs`

We rename `SystemdQuery` → `Systemd` and add the manager methods we need. `FixedSystemd` (test impl) gets a `Mutex<Vec<RecordedCall>>` so we can assert the dispatcher made the right call.

- [ ] **Step 1: Write the failing test (FixedSystemd records calls)**

Add to `crates/boxpilotd/src/systemd.rs` inside the existing `pub mod testing { … }` block:

```rust
    use std::sync::Mutex;

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
    async fn recording_systemd_records_start_unit() {
        let s = RecordingSystemd::new(UnitState::NotFound);
        s.start_unit("boxpilot-sing-box.service").await.unwrap();
        assert_eq!(
            s.calls(),
            vec![RecordedCall::StartUnit("boxpilot-sing-box.service".into())]
        );
    }
```

- [ ] **Step 2: Update the trait definition (top of file)**

In `crates/boxpilotd/src/systemd.rs`, replace the existing `pub trait SystemdQuery` block:

```rust
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
```

Keep the `SystemdManagerProxy` / `SystemdUnitProxy` / `SystemdServiceProxy` trait declarations as-is from plan #1.

- [ ] **Step 3: Update `FixedSystemd` to implement the new trait**

Replace the existing `FixedSystemd` block in `pub mod testing` with one that returns `Ok(())` for all mutating methods (the recording variant we just added handles call assertions). The two co-exist: tests that don't care about call sequence keep using `FixedSystemd`; tests that need to verify a verb fired use `RecordingSystemd`.

```rust
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
```

- [ ] **Step 4: Run the failing-then-passing tests**

Run: `cargo test -p boxpilotd systemd::testing`
Expected: passes (FixedSystemd + RecordingSystemd both compile against the new trait).

The compiler will also flag every existing `Arc<dyn SystemdQuery>` site — fix the type-name change in **Task 4** (context).

- [ ] **Step 5: Commit (without DBusSystemd impl yet — that's Task 3)**

```bash
git add crates/boxpilotd/src/systemd.rs
git commit -m "refactor(systemd): rename SystemdQuery -> Systemd; add mutating verbs (test impls only)"
```

---

## Task 3: Implement the new systemd verbs in `DBusSystemd`

**Files:**
- Modify: `crates/boxpilotd/src/systemd.rs`

The existing `SystemdManagerProxy` only declares `get_unit` and `load_unit`. Add the manager methods we need.

- [ ] **Step 1: Extend `SystemdManagerProxy` with the manager methods**

Edit the `#[proxy(...)] trait SystemdManager` block:

```rust
#[proxy(
    interface = "org.freedesktop.systemd1.Manager",
    default_service = "org.freedesktop.systemd1",
    default_path = "/org/freedesktop/systemd1"
)]
trait SystemdManager {
    fn get_unit(&self, name: &str) -> zbus::Result<zbus::zvariant::OwnedObjectPath>;
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
```

- [ ] **Step 2: Implement the `Systemd` trait methods on `DBusSystemd`**

Append after the existing `unit_state` impl in `impl Systemd for DBusSystemd`:

```rust
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
```

The existing `fn systemd_err(e: zbus::Error) -> HelperError { … }` already exists — reuse it.

- [ ] **Step 3: Build (no new tests — DBusSystemd is exercised by the smoke procedure)**

Run: `cargo build -p boxpilotd`
Expected: clean build (no warnings about the new methods being unused).

- [ ] **Step 4: Commit**

```bash
git add crates/boxpilotd/src/systemd.rs
git commit -m "feat(systemd): wire StartUnit/StopUnit/RestartUnit/EnableUnitFiles/DisableUnitFiles/Reload"
```

---

## Task 4: JournalReader trait + journalctl process backend

**Files:**
- Modify: `crates/boxpilotd/src/systemd.rs` (or create a sibling `journal.rs` — keeping it co-located in `systemd.rs` for simplicity since the trait is small)

`service.logs` reads a bounded tail. Implementing the journal D-Bus interface (`org.freedesktop.systemd1.Manager.GetUnitFileState` is read-only; the journal lives on `sd-bus-journal` which is awkward over D-Bus). `journalctl(1)` is always present on systemd boxes and gives plain-text output. We trade a process spawn per `service.logs` call for ~30 lines of code.

- [ ] **Step 1: Append the trait + impl + test to `systemd.rs`**

Append at the end of `crates/boxpilotd/src/systemd.rs` (before the `#[cfg(test)] pub mod testing` block):

```rust
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
```

- [ ] **Step 2: Add a test stub to the existing `pub mod testing` block**

```rust
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
```

- [ ] **Step 3: Run tests**

Run: `cargo test -p boxpilotd systemd::testing::fixed_journal`
Expected: passes.

- [ ] **Step 4: Commit**

```bash
git add crates/boxpilotd/src/systemd.rs
git commit -m "feat(systemd): add JournalReader trait + JournalctlProcess backend"
```

---

## Task 5: paths.rs — systemd unit path + polkit drop-in path

**Files:**
- Modify: `crates/boxpilotd/src/paths.rs`

We need two new path helpers:
- `systemd_unit_path()` returns `/etc/systemd/system/boxpilot-sing-box.service` (under the test root in tests).
- `polkit_controller_dropin_path()` returns `/etc/polkit-1/rules.d/48-boxpilot-controller.rules`. Lexical sort runs `48-…` before `49-boxpilot.rules`, so the var is in scope when the main rule reads it.

- [ ] **Step 1: Write the failing tests**

Append to `crates/boxpilotd/src/paths.rs` `mod tests`:

```rust
    #[test]
    fn systemd_unit_path_under_etc_systemd_system() {
        let p = Paths::system();
        assert_eq!(
            p.systemd_unit_path(),
            PathBuf::from("/etc/systemd/system/boxpilot-sing-box.service")
        );
    }

    #[test]
    fn polkit_dropin_path_uses_48_prefix_so_it_loads_before_49() {
        let p = Paths::system();
        assert_eq!(
            p.polkit_controller_dropin_path(),
            PathBuf::from("/etc/polkit-1/rules.d/48-boxpilot-controller.rules")
        );
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p boxpilotd paths::tests`
Expected: fail with "no method named `systemd_unit_path`".

- [ ] **Step 3: Add the path helpers**

In `crates/boxpilotd/src/paths.rs`, before `#[cfg(test)] mod tests`:

```rust
    /// `/etc/systemd/system/boxpilot-sing-box.service`. Written by
    /// `service.install_managed`. Plan #3 keeps the unit name hard-coded
    /// because it must match `BoxpilotConfig::target_service`'s default.
    pub fn systemd_unit_path(&self) -> PathBuf {
        self.root.join("etc/systemd/system/boxpilot-sing-box.service")
    }

    /// `/etc/polkit-1/rules.d/48-boxpilot-controller.rules`. The daemon
    /// rewrites this file under `/run/boxpilot/lock` whenever the
    /// controller is claimed or transferred, so `49-boxpilot.rules` can
    /// read `BOXPILOT_CONTROLLER` directly instead of spawning `cat`.
    /// `48-` sorts before `49-` and polkit evaluates rules.d/* in lexical
    /// order, so the var is in scope when the main rule runs.
    pub fn polkit_controller_dropin_path(&self) -> PathBuf {
        self.root
            .join("etc/polkit-1/rules.d/48-boxpilot-controller.rules")
    }
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p boxpilotd paths::tests`
Expected: 4 tests pass.

- [ ] **Step 5: Commit**

```bash
git add crates/boxpilotd/src/paths.rs
git commit -m "feat(paths): systemd_unit_path + polkit_controller_dropin_path"
```

---

## Task 6: Unit-template renderer (pure function)

**Files:**
- Create: `crates/boxpilotd/src/service/mod.rs`
- Create: `crates/boxpilotd/src/service/unit.rs`

Pure function, no I/O, no traits — easy to test.

- [ ] **Step 1: Add the module declaration**

Create `crates/boxpilotd/src/service/mod.rs`:

```rust
pub mod control;
pub mod install;
pub mod logs;
pub mod unit;
pub mod verify;
```

(`control` / `install` / `logs` / `verify` files are empty placeholders this task; later tasks fill them.)

Create empty stubs:

```rust
// crates/boxpilotd/src/service/control.rs
```
```rust
// crates/boxpilotd/src/service/install.rs
```
```rust
// crates/boxpilotd/src/service/logs.rs
```
```rust
// crates/boxpilotd/src/service/verify.rs
```

Also add `mod service;` to `crates/boxpilotd/src/main.rs` (alongside the existing `mod systemd;` etc.).

- [ ] **Step 2: Write the failing test**

Create `crates/boxpilotd/src/service/unit.rs` with the test first:

```rust
//! Pure renderer for `boxpilot-sing-box.service` (spec §7.1). No I/O.
//! The test below is the source of truth for the unit content; if the
//! string changes you must update both this file and the reference
//! template in `packaging/linux/systemd/boxpilot-sing-box.service.in`.

use std::path::Path;

pub fn render(core_path: &Path) -> String {
    format!(
        "[Unit]\n\
Description=BoxPilot managed sing-box service\n\
Documentation=https://sing-box.sagernet.org/\n\
After=network-online.target nss-lookup.target\n\
Wants=network-online.target\n\
\n\
[Service]\n\
Type=simple\n\
User=root\n\
UMask=0077\n\
WorkingDirectory=/etc/boxpilot/active\n\
ExecStartPre={core} check -c config.json\n\
ExecStart={core} run -c config.json\n\
Restart=on-failure\n\
RestartSec=5s\n\
StartLimitIntervalSec=300\n\
StartLimitBurst=5\n\
LimitNOFILE=1048576\n\
\n\
# Sandboxing — keep what TUN / auto_redirect need, drop everything else (spec \u{a7}7.1)\n\
NoNewPrivileges=true\n\
CapabilityBoundingSet=CAP_NET_ADMIN CAP_NET_BIND_SERVICE CAP_NET_RAW\n\
AmbientCapabilities=CAP_NET_ADMIN CAP_NET_BIND_SERVICE CAP_NET_RAW\n\
ProtectSystem=strict\n\
ProtectHome=true\n\
PrivateTmp=true\n\
ProtectControlGroups=true\n\
RestrictNamespaces=true\n\
RestrictRealtime=true\n\
LockPersonality=true\n\
RestrictAddressFamilies=AF_UNIX AF_INET AF_INET6 AF_NETLINK AF_PACKET\n\
ReadWritePaths=/etc/boxpilot/active\n\
\n\
[Install]\n\
WantedBy=multi-user.target\n",
        core = core_path.display()
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn render_substitutes_core_path_in_exec_start() {
        let s = render(&PathBuf::from("/var/lib/boxpilot/cores/current/sing-box"));
        assert!(s.contains("ExecStart=/var/lib/boxpilot/cores/current/sing-box run -c config.json"));
        assert!(s.contains("ExecStartPre=/var/lib/boxpilot/cores/current/sing-box check -c config.json"));
    }

    #[test]
    fn render_includes_required_sandbox_directives() {
        let s = render(&PathBuf::from("/usr/bin/sing-box"));
        for must_have in [
            "NoNewPrivileges=true",
            "CapabilityBoundingSet=CAP_NET_ADMIN CAP_NET_BIND_SERVICE CAP_NET_RAW",
            "AmbientCapabilities=CAP_NET_ADMIN CAP_NET_BIND_SERVICE CAP_NET_RAW",
            "ProtectSystem=strict",
            "ProtectHome=true",
            "PrivateTmp=true",
            "ProtectControlGroups=true",
            "RestrictNamespaces=true",
            "RestrictRealtime=true",
            "LockPersonality=true",
            "RestrictAddressFamilies=AF_UNIX AF_INET AF_INET6 AF_NETLINK AF_PACKET",
            "ReadWritePaths=/etc/boxpilot/active",
            "WorkingDirectory=/etc/boxpilot/active",
        ] {
            assert!(s.contains(must_have), "missing: {must_have}\n----\n{s}");
        }
    }

    /// Spec §7.1 explicitly does NOT set ProtectKernelTunables because
    /// auto_redirect writes to /proc/sys/net/* sysctls. Catch a future
    /// PR that "tightens" sandboxing and silently breaks auto_redirect.
    #[test]
    fn render_does_not_set_protect_kernel_tunables() {
        let s = render(&PathBuf::from("/x"));
        assert!(
            !s.contains("ProtectKernelTunables"),
            "auto_redirect needs sysctl writes — see spec \u{a7}7.1"
        );
    }

    #[test]
    fn render_install_section_targets_multi_user() {
        let s = render(&PathBuf::from("/x"));
        assert!(s.contains("[Install]\nWantedBy=multi-user.target"));
    }
}
```

- [ ] **Step 3: Run tests**

Run: `cargo test -p boxpilotd service::unit`
Expected: 4 tests pass.

- [ ] **Step 4: Commit**

```bash
git add crates/boxpilotd/src/service/ crates/boxpilotd/src/main.rs
git commit -m "feat(service): pure unit-template renderer (sec 7.1)"
```

---

## Task 7: Reference unit template under `packaging/linux/systemd/`

**Files:**
- Create: `packaging/linux/systemd/boxpilot-sing-box.service.in`

Documentation-only mirror of `service::unit::render`. The daemon never reads it — it's a reference for reviewers / package maintainers / sysadmins running `cat` on a clean checkout.

- [ ] **Step 1: Write the file**

Create `packaging/linux/systemd/boxpilot-sing-box.service.in`:

```ini
# Reference template for the unit produced at runtime by
# `boxpilotd`'s `service.install_managed` action.
# `@CORE_PATH@` is replaced with the active core path (e.g.
# `/var/lib/boxpilot/cores/current/sing-box`). The actual file
# under `/etc/systemd/system/` is generated — do NOT install
# this `.in` file directly. Source of truth: `service::unit::render`.

[Unit]
Description=BoxPilot managed sing-box service
Documentation=https://sing-box.sagernet.org/
After=network-online.target nss-lookup.target
Wants=network-online.target

[Service]
Type=simple
User=root
UMask=0077
WorkingDirectory=/etc/boxpilot/active
ExecStartPre=@CORE_PATH@ check -c config.json
ExecStart=@CORE_PATH@ run -c config.json
Restart=on-failure
RestartSec=5s
StartLimitIntervalSec=300
StartLimitBurst=5
LimitNOFILE=1048576

# Sandboxing — see spec §7.1
NoNewPrivileges=true
CapabilityBoundingSet=CAP_NET_ADMIN CAP_NET_BIND_SERVICE CAP_NET_RAW
AmbientCapabilities=CAP_NET_ADMIN CAP_NET_BIND_SERVICE CAP_NET_RAW
ProtectSystem=strict
ProtectHome=true
PrivateTmp=true
ProtectControlGroups=true
RestrictNamespaces=true
RestrictRealtime=true
LockPersonality=true
RestrictAddressFamilies=AF_UNIX AF_INET AF_INET6 AF_NETLINK AF_PACKET
ReadWritePaths=/etc/boxpilot/active

[Install]
WantedBy=multi-user.target
```

- [ ] **Step 2: Commit**

```bash
git add packaging/linux/systemd/boxpilot-sing-box.service.in
git commit -m "docs(packaging): reference systemd unit template"
```

---

## Task 8: Extend StateCommit to atomically write the polkit drop-in

**Files:**
- Modify: `crates/boxpilotd/src/core/commit.rs`

When `controller` is `Some`, `StateCommit::apply` already writes `controller-name.new` and renames it. Extend it to also stage `48-boxpilot-controller.rules.new` containing `var BOXPILOT_CONTROLLER = "<username>";` and rename it in the same commit phase. Both files describe the *same* fact — the controller's username — so they must move together or roll back together.

- [ ] **Step 1: Write the failing test**

Append to `crates/boxpilotd/src/core/commit.rs` `mod tests`:

```rust
    #[tokio::test]
    async fn apply_writes_polkit_dropin_when_claiming_controller() {
        let tmp = tempdir().unwrap();
        let paths = paths_for(&tmp);
        let commit = StateCommit {
            paths: paths.clone(),
            toml_updates: TomlUpdates::default(),
            controller: Some(ControllerWrites {
                uid: 1000,
                username: "alice".into(),
            }),
            install_state: InstallState::empty(),
            current_symlink_target: None,
        };
        commit.apply().await.unwrap();
        let dropin = tokio::fs::read_to_string(paths.polkit_controller_dropin_path())
            .await
            .unwrap();
        // The drop-in is the simplest possible polkit JS that exposes a global var.
        assert!(dropin.contains("var BOXPILOT_CONTROLLER = \"alice\";"));
    }

    #[tokio::test]
    async fn apply_does_not_write_polkit_dropin_when_not_claiming() {
        let tmp = tempdir().unwrap();
        let paths = paths_for(&tmp);
        let commit = StateCommit {
            paths: paths.clone(),
            toml_updates: TomlUpdates {
                core_path: Some("/x".into()),
                core_state: Some(CoreState::ManagedInstalled),
            },
            controller: None,
            install_state: InstallState::empty(),
            current_symlink_target: None,
        };
        commit.apply().await.unwrap();
        assert!(!paths.polkit_controller_dropin_path().exists());
    }

    #[tokio::test]
    async fn apply_escapes_quotes_and_backslashes_in_username() {
        // No real user has these chars in their name, but the renderer
        // is a security-relevant trust boundary: a malformed identifier
        // must not let an attacker inject `; AnyOtherStatement;`.
        let tmp = tempdir().unwrap();
        let paths = paths_for(&tmp);
        let commit = StateCommit {
            paths: paths.clone(),
            toml_updates: TomlUpdates::default(),
            controller: Some(ControllerWrites {
                uid: 1000,
                username: r#"a"\b"#.into(),
            }),
            install_state: InstallState::empty(),
            current_symlink_target: None,
        };
        commit.apply().await.unwrap();
        let dropin = tokio::fs::read_to_string(paths.polkit_controller_dropin_path())
            .await
            .unwrap();
        // Backslash and double-quote are escaped; no raw injection vector.
        assert!(dropin.contains(r#"var BOXPILOT_CONTROLLER = "a\"\\b";"#));
    }
```

- [ ] **Step 2: Add the renderer + extend `apply`**

In `crates/boxpilotd/src/core/commit.rs`, add a free helper above `impl StateCommit`:

```rust
/// Render the polkit drop-in body. Username is JSON-escaped so a hostile
/// (or just unusual) username cannot inject additional polkit JS.
fn render_polkit_dropin(username: &str) -> String {
    // serde_json::to_string adds the surrounding quotes and escapes \ and ".
    let escaped = serde_json::to_string(username).expect("string serializes");
    format!(
        "// Generated by boxpilotd alongside /etc/boxpilot/controller-name.\n\
         // Do not edit — it is overwritten on every controller claim/transfer.\n\
         var BOXPILOT_CONTROLLER = {escaped};\n"
    )
}
```

Then extend `StateCommit::apply` to stage and rename the drop-in alongside the controller-name file. Insert after the existing `controller_name_tmp` block (step 1c) and add a matching commit step 2c.5 *after* the controller-name rename:

```rust
        // 1c'. polkit-controller-dropin.rules.new (only when claiming controller)
        let polkit_dropin_tmp = if let Some(c) = &self.controller {
            let dropin_path = self.paths.polkit_controller_dropin_path();
            if let Some(parent) = dropin_path.parent() {
                tokio::fs::create_dir_all(parent)
                    .await
                    .map_err(|e| HelperError::Ipc {
                        message: format!("mkdir polkit rules.d parent: {e}"),
                    })?;
            }
            let tmp = dropin_path.with_extension("rules.new");
            tokio::fs::write(&tmp, render_polkit_dropin(&c.username))
                .await
                .map_err(|e| HelperError::Ipc {
                    message: format!("stage polkit drop-in: {e}"),
                })?;
            fsync_path(&tmp).await?;
            Some((tmp, dropin_path))
        } else {
            None
        };
```

…and after the existing `2c. controller-name` rename:

```rust
        // 2c'. polkit drop-in (right after controller-name so the two are
        // visible as a single unit to anyone reading /etc).
        if let Some((tmp, final_path)) = polkit_dropin_tmp {
            tokio::fs::rename(&tmp, &final_path)
                .await
                .map_err(|e| HelperError::Ipc {
                    message: format!("rename polkit drop-in: {e}"),
                })?;
        }
```

- [ ] **Step 3: Run tests**

Run: `cargo test -p boxpilotd core::commit::tests`
Expected: existing 2 tests still pass + 3 new ones pass = 5 total.

- [ ] **Step 4: Commit**

```bash
git add crates/boxpilotd/src/core/commit.rs
git commit -m "feat(commit): atomically write polkit drop-in alongside controller-name"
```

---

## Task 9: Replace `polkit.spawn(cat)` in `49-boxpilot.rules`

**Files:**
- Modify: `packaging/linux/polkit-1/rules.d/49-boxpilot.rules`

The drop-in from Task 8 sets `var BOXPILOT_CONTROLLER = "<username>"` at file-load time. The main rule reads that global var and skips the `polkit.spawn` call entirely.

- [ ] **Step 1: Replace the rule body**

Overwrite `packaging/linux/polkit-1/rules.d/49-boxpilot.rules`:

```javascript
// 49-boxpilot.rules — promote the BoxPilot controller user to less-prompting
// auth tiers (spec §6.3). The controller's username comes from
// `48-boxpilot-controller.rules`, which the boxpilotd daemon writes
// atomically alongside `/etc/boxpilot/controller-name`. Polkit evaluates
// rules.d/* in lexical order, so the `48-` file runs first and the
// `BOXPILOT_CONTROLLER` global is in scope here.
//
// When `48-boxpilot-controller.rules` is absent (e.g. fresh install before
// any controller has been claimed), `BOXPILOT_CONTROLLER` is undefined —
// we fall through to XML defaults, which is the safe direction.

polkit.addRule(function(action, subject) {
    if (action.id.indexOf("app.boxpilot.helper.") !== 0) {
        return;
    }

    if (typeof BOXPILOT_CONTROLLER === "undefined" || BOXPILOT_CONTROLLER === null
        || BOXPILOT_CONTROLLER === "") {
        return;
    }

    if (subject.user !== BOXPILOT_CONTROLLER) {
        // Non-controller: keep XML defaults.
        return;
    }

    // Controller path. Map the action's authorization class:
    //   read-only → YES (no prompt)
    //   high-risk → AUTH_ADMIN (always prompt; no cache)
    //   mutating  → AUTH_SELF_KEEP (controller proves identity, cached)
    var id = action.id;

    if (id === "app.boxpilot.helper.service.status" ||
        id === "app.boxpilot.helper.service.logs" ||
        id === "app.boxpilot.helper.core.discover" ||
        id === "app.boxpilot.helper.legacy.observe-service") {
        return polkit.Result.YES;
    }

    if (id === "app.boxpilot.helper.controller.transfer" ||
        id === "app.boxpilot.helper.legacy.migrate-service") {
        return polkit.Result.AUTH_ADMIN;
    }

    return polkit.Result.AUTH_SELF_KEEP;
});
```

- [ ] **Step 2: The policy-drift integration test still passes**

Run: `cargo test -p boxpilot-ipc --test policy_drift`
Expected: passes (the test reads the XML, not the JS rule, and we changed neither action count nor IDs).

- [ ] **Step 3: Commit**

```bash
git add packaging/linux/polkit-1/rules.d/49-boxpilot.rules
git commit -m "perf(polkit): read BOXPILOT_CONTROLLER global instead of spawning cat per check"
```

---

## Task 10: `service::install` — atomic unit write + daemon-reload

**Files:**
- Modify: `crates/boxpilotd/src/service/install.rs`

Pre-conditions enforced inside the helper (post-polkit, post-lock):
- `boxpilot.toml` has a `core_path` (a managed core has been installed; otherwise return `HelperError::Ipc { message: "no core configured" }` — explicit failure rather than writing a unit referencing nothing).
- The `core_path` passes the §6.5 trust check.

- [ ] **Step 1: Write the failing test**

Replace `crates/boxpilotd/src/service/install.rs` (was a stub):

```rust
//! `service.install_managed` (§6.3): generate the unit text, write it
//! atomically to `/etc/systemd/system/boxpilot-sing-box.service`, then
//! `daemon-reload` so a subsequent `start_unit` finds it.
//!
//! This module assumes the caller (iface.rs) has already gone through
//! `dispatch::authorize` and is holding `/run/boxpilot/lock`.

use crate::core::trust::{
    default_allowed_prefixes, verify_executable_path, FsMetadataProvider, TrustError,
};
use crate::dispatch::ControllerWrites;
use crate::paths::Paths;
use crate::service::unit;
use crate::systemd::Systemd;
use boxpilot_ipc::{
    BoxpilotConfig, HelperError, HelperResult, ServiceInstallManagedResponse, UnitState,
};
use std::path::PathBuf;

pub struct InstallDeps<'a> {
    pub paths: Paths,
    pub systemd: &'a dyn Systemd,
    pub fs: &'a dyn FsMetadataProvider,
}

pub async fn install_managed(
    cfg: &BoxpilotConfig,
    deps: &InstallDeps<'_>,
    _controller: Option<ControllerWrites>,
) -> HelperResult<ServiceInstallManagedResponse> {
    let core_path_str = cfg.core_path.as_deref().ok_or_else(|| HelperError::Ipc {
        message: "no core configured — install or adopt a core first".into(),
    })?;
    let core_path = PathBuf::from(core_path_str);

    // Trust check on the core path *before* baking it into a root-run unit.
    let prefixes = default_allowed_prefixes();
    verify_executable_path(deps.fs, &core_path, &prefixes).map_err(map_trust_err)?;

    let unit_text = unit::render(&core_path);
    let target = deps.paths.systemd_unit_path();
    write_unit_atomic(&target, &unit_text).await?;

    deps.systemd.reload().await?;
    let unit_state = deps.systemd.unit_state("boxpilot-sing-box.service").await?;

    Ok(ServiceInstallManagedResponse {
        unit_state,
        generated_unit_path: target.to_string_lossy().to_string(),
        // claimed_controller is filled by the iface wrapper from the dispatch
        // result — keep it false here since the body itself has no view.
        claimed_controller: false,
    })
}

async fn write_unit_atomic(target: &std::path::Path, text: &str) -> HelperResult<()> {
    if let Some(parent) = target.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|e| HelperError::Ipc {
                message: format!("mkdir unit parent: {e}"),
            })?;
    }
    let tmp = target.with_extension("service.new");
    tokio::fs::write(&tmp, text)
        .await
        .map_err(|e| HelperError::Ipc {
            message: format!("write unit: {e}"),
        })?;
    let f = tokio::fs::OpenOptions::new()
        .read(true)
        .open(&tmp)
        .await
        .map_err(|e| HelperError::Ipc {
            message: format!("open unit for fsync: {e}"),
        })?;
    f.sync_all().await.map_err(|e| HelperError::Ipc {
        message: format!("fsync unit: {e}"),
    })?;
    tokio::fs::rename(&tmp, target)
        .await
        .map_err(|e| HelperError::Ipc {
            message: format!("rename unit: {e}"),
        })?;
    Ok(())
}

fn map_trust_err(e: TrustError) -> HelperError {
    HelperError::Ipc {
        message: format!("trust check failed: {e}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::trust::{FileKind, FileStat};
    use crate::systemd::testing::{FixedSystemd, RecordingSystemd};
    use boxpilot_ipc::CoreState;
    use std::path::Path;
    use tempfile::tempdir;

    /// A permissive FS that says every probed file is a root-owned regular
    /// file with mode 0o755 — sufficient to pass the §6.5 trust check
    /// against the staged `cores/current/sing-box` symlink target.
    struct PermissiveFs;
    impl FsMetadataProvider for PermissiveFs {
        fn stat(&self, p: &Path) -> std::io::Result<FileStat> {
            let kind = if p.file_name().map(|n| n == "sing-box").unwrap_or(false) {
                FileKind::Regular
            } else {
                FileKind::Directory
            };
            Ok(FileStat { uid: 0, gid: 0, mode: 0o755, kind })
        }
        fn read_link(&self, _: &Path) -> std::io::Result<std::path::PathBuf> {
            Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "no symlinks",
            ))
        }
    }

    fn cfg_with_core(path: &str) -> BoxpilotConfig {
        BoxpilotConfig {
            schema_version: 1,
            target_service: "boxpilot-sing-box.service".into(),
            core_path: Some(path.into()),
            core_state: Some(CoreState::ManagedInstalled),
            controller_uid: Some(1000),
            active_profile_id: None,
            active_profile_name: None,
            active_profile_sha256: None,
            active_release_id: None,
            activated_at: None,
        }
    }

    #[tokio::test]
    async fn install_writes_unit_file_and_reloads() {
        let tmp = tempdir().unwrap();
        let paths = Paths::with_root(tmp.path());
        let systemd = RecordingSystemd::new(UnitState::Known {
            active_state: "inactive".into(),
            sub_state: "dead".into(),
            load_state: "loaded".into(),
            n_restarts: 0,
            exec_main_status: 0,
        });
        let fs = PermissiveFs;
        let cfg = cfg_with_core("/usr/bin/sing-box");
        let deps = InstallDeps { paths: paths.clone(), systemd: &systemd, fs: &fs };

        let resp = install_managed(&cfg, &deps, None).await.unwrap();

        let written = tokio::fs::read_to_string(paths.systemd_unit_path()).await.unwrap();
        assert!(written.contains("ExecStart=/usr/bin/sing-box run -c config.json"));
        assert!(matches!(resp.unit_state, UnitState::Known { .. }));
        let calls = systemd.calls();
        assert!(
            calls.iter().any(|c| matches!(c, crate::systemd::testing::RecordedCall::Reload)),
            "expected daemon-reload, got {calls:?}"
        );
    }

    #[tokio::test]
    async fn install_without_core_path_returns_explicit_error() {
        let tmp = tempdir().unwrap();
        let paths = Paths::with_root(tmp.path());
        let systemd = FixedSystemd { answer: UnitState::NotFound };
        let fs = PermissiveFs;
        let mut cfg = cfg_with_core("/x");
        cfg.core_path = None;
        let deps = InstallDeps { paths, systemd: &systemd, fs: &fs };
        let r = install_managed(&cfg, &deps, None).await;
        assert!(matches!(r, Err(HelperError::Ipc { .. })));
    }

    #[tokio::test]
    async fn install_with_untrusted_core_path_aborts_before_writing() {
        let tmp = tempdir().unwrap();
        let paths = Paths::with_root(tmp.path());
        let systemd = FixedSystemd { answer: UnitState::NotFound };
        // Reject everything — simulates §6.5 failure.
        struct DenyFs;
        impl FsMetadataProvider for DenyFs {
            fn stat(&self, _: &Path) -> std::io::Result<FileStat> {
                Err(std::io::Error::new(std::io::ErrorKind::NotFound, "denied"))
            }
            fn read_link(&self, _: &Path) -> std::io::Result<std::path::PathBuf> {
                Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, "x"))
            }
        }
        let fs = DenyFs;
        let cfg = cfg_with_core("/home/evil/sing-box");
        let deps = InstallDeps { paths: paths.clone(), systemd: &systemd, fs: &fs };
        let r = install_managed(&cfg, &deps, None).await;
        assert!(matches!(r, Err(HelperError::Ipc { .. })));
        // Critical: the unit file must NOT have been written.
        assert!(!paths.systemd_unit_path().exists());
    }
}
```

- [ ] **Step 2: Run tests**

Run: `cargo test -p boxpilotd service::install`
Expected: 3 tests pass.

- [ ] **Step 3: Commit**

```bash
git add crates/boxpilotd/src/service/install.rs
git commit -m "feat(service): atomic unit write + daemon-reload for service.install_managed"
```

---

## Task 11: `service::control` — start / stop / restart / enable / disable

**Files:**
- Modify: `crates/boxpilotd/src/service/control.rs`

Each verb does one systemd call followed by a `unit_state` query so the response carries the post-op state.

- [ ] **Step 1: Write the failing test**

Replace `crates/boxpilotd/src/service/control.rs`:

```rust
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

pub async fn run(verb: Verb, unit_name: &str, systemd: &dyn Systemd) -> HelperResult<ServiceControlResponse> {
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
        let resp = run(Verb::Start, "boxpilot-sing-box.service", &s).await.unwrap();
        assert!(matches!(resp.unit_state, UnitState::Known { .. }));
        assert_eq!(
            s.calls(),
            vec![RecordedCall::StartUnit("boxpilot-sing-box.service".into())]
        );
    }

    #[tokio::test]
    async fn enable_invokes_enable_unit_files() {
        let s = RecordingSystemd::new(UnitState::NotFound);
        run(Verb::Enable, "boxpilot-sing-box.service", &s).await.unwrap();
        assert_eq!(
            s.calls(),
            vec![RecordedCall::EnableUnitFiles(vec!["boxpilot-sing-box.service".into()])]
        );
    }

    #[tokio::test]
    async fn disable_invokes_disable_unit_files() {
        let s = RecordingSystemd::new(UnitState::NotFound);
        run(Verb::Disable, "boxpilot-sing-box.service", &s).await.unwrap();
        assert_eq!(
            s.calls(),
            vec![RecordedCall::DisableUnitFiles(vec!["boxpilot-sing-box.service".into()])]
        );
    }

    #[tokio::test]
    async fn restart_invokes_restart_unit() {
        let s = RecordingSystemd::new(UnitState::NotFound);
        run(Verb::Restart, "boxpilot-sing-box.service", &s).await.unwrap();
        assert_eq!(
            s.calls(),
            vec![RecordedCall::RestartUnit("boxpilot-sing-box.service".into())]
        );
    }

    #[tokio::test]
    async fn stop_invokes_stop_unit() {
        let s = RecordingSystemd::new(UnitState::NotFound);
        run(Verb::Stop, "boxpilot-sing-box.service", &s).await.unwrap();
        assert_eq!(
            s.calls(),
            vec![RecordedCall::StopUnit("boxpilot-sing-box.service".into())]
        );
    }
}
```

- [ ] **Step 2: Run tests**

Run: `cargo test -p boxpilotd service::control`
Expected: 5 tests pass.

- [ ] **Step 3: Commit**

```bash
git add crates/boxpilotd/src/service/control.rs
git commit -m "feat(service): start/stop/restart/enable/disable verbs"
```

---

## Task 12: `service::logs` — clamped journalctl tail

**Files:**
- Modify: `crates/boxpilotd/src/service/logs.rs`

- [ ] **Step 1: Write the failing test**

Replace `crates/boxpilotd/src/service/logs.rs`:

```rust
//! `service.logs` (§6.3): bounded journalctl tail for the managed unit.

use crate::systemd::JournalReader;
use boxpilot_ipc::{
    HelperResult, ServiceLogsRequest, ServiceLogsResponse, SERVICE_LOGS_DEFAULT_LINES,
    SERVICE_LOGS_MAX_LINES,
};

pub async fn read(
    req: &ServiceLogsRequest,
    unit_name: &str,
    journal: &dyn JournalReader,
) -> HelperResult<ServiceLogsResponse> {
    let requested = if req.lines == 0 {
        SERVICE_LOGS_DEFAULT_LINES
    } else {
        req.lines
    };
    let clamped = requested.min(SERVICE_LOGS_MAX_LINES);
    let truncated = clamped < requested;
    let lines = journal.tail(unit_name, clamped).await?;
    Ok(ServiceLogsResponse { lines, truncated })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::systemd::testing::FixedJournal;

    #[tokio::test]
    async fn zero_request_uses_default() {
        let j = FixedJournal {
            lines: vec!["a".into()],
        };
        let r = read(&ServiceLogsRequest { lines: 0 }, "u", &j).await.unwrap();
        assert_eq!(r.lines, vec!["a".to_string()]);
        assert!(!r.truncated);
    }

    #[tokio::test]
    async fn over_max_is_clamped_and_truncated_flag_set() {
        let j = FixedJournal { lines: Vec::new() };
        let r = read(&ServiceLogsRequest { lines: 10_000 }, "u", &j).await.unwrap();
        assert!(r.truncated);
    }

    #[tokio::test]
    async fn under_max_passes_through_untruncated() {
        let j = FixedJournal { lines: Vec::new() };
        let r = read(&ServiceLogsRequest { lines: 50 }, "u", &j).await.unwrap();
        assert!(!r.truncated);
    }
}
```

- [ ] **Step 2: Run tests**

Run: `cargo test -p boxpilotd service::logs`
Expected: 3 tests pass.

- [ ] **Step 3: Commit**

```bash
git add crates/boxpilotd/src/service/logs.rs
git commit -m "feat(service): clamped journalctl tail for service.logs"
```

---

## Task 13: `service::verify` — post-op `wait_for_running` helper (plan #5 reuse)

**Files:**
- Modify: `crates/boxpilotd/src/service/verify.rs`

Spec §7.2 specifies a 5 s default verification window after a service change. Plan #5 (activation) needs this wired into the activation transaction; plan #3 ships the helper and exercises it from the smoke procedure but does **not** wire it into Start/Restart yet — those return the immediate post-op `unit_state`. Putting the helper here lets plan #5 add the await without re-architecting the flow.

- [ ] **Step 1: Write the failing test**

Replace `crates/boxpilotd/src/service/verify.rs`:

```rust
//! Spec §7.2 runtime verification helper. Polls the unit until it
//! reaches active/running with `n_restarts` unchanged from the
//! pre-operation snapshot, or the deadline elapses.
//!
//! Plan #3 only ships this helper; plan #5 (profile activation) wires
//! it into the activation transaction. Keeping it here means plan #5
//! adds an await rather than restructuring the flow.

use crate::systemd::Systemd;
use boxpilot_ipc::{HelperResult, UnitState};
use std::time::{Duration, Instant};

/// Default per spec §7.2: 5 seconds.
pub const DEFAULT_WINDOW: Duration = Duration::from_secs(5);
/// Spec §7.2 cap.
pub const MAX_WINDOW: Duration = Duration::from_secs(30);
/// Polling cadence inside the window.
pub const POLL_INTERVAL: Duration = Duration::from_millis(250);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VerifyOutcome {
    Running,
    Stuck { final_state: UnitState },
    NotFound,
}

pub async fn wait_for_running(
    unit_name: &str,
    pre_n_restarts: u32,
    window: Duration,
    systemd: &dyn Systemd,
) -> HelperResult<VerifyOutcome> {
    let window = window.min(MAX_WINDOW);
    let deadline = Instant::now() + window;
    loop {
        let state = systemd.unit_state(unit_name).await?;
        match &state {
            UnitState::NotFound => return Ok(VerifyOutcome::NotFound),
            UnitState::Known {
                active_state,
                sub_state,
                n_restarts,
                ..
            } if active_state == "active"
                && sub_state == "running"
                && *n_restarts == pre_n_restarts =>
            {
                return Ok(VerifyOutcome::Running);
            }
            _ => {}
        }
        if Instant::now() >= deadline {
            return Ok(VerifyOutcome::Stuck { final_state: state });
        }
        tokio::time::sleep(POLL_INTERVAL).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::systemd::testing::FixedSystemd;

    #[tokio::test]
    async fn returns_running_when_state_already_active() {
        let s = FixedSystemd {
            answer: UnitState::Known {
                active_state: "active".into(),
                sub_state: "running".into(),
                load_state: "loaded".into(),
                n_restarts: 0,
                exec_main_status: 0,
            },
        };
        let o = wait_for_running("u", 0, Duration::from_millis(50), &s).await.unwrap();
        assert_eq!(o, VerifyOutcome::Running);
    }

    #[tokio::test]
    async fn returns_stuck_when_state_never_reaches_running_within_window() {
        let s = FixedSystemd {
            answer: UnitState::Known {
                active_state: "activating".into(),
                sub_state: "start".into(),
                load_state: "loaded".into(),
                n_restarts: 0,
                exec_main_status: 0,
            },
        };
        let o = wait_for_running("u", 0, Duration::from_millis(100), &s).await.unwrap();
        assert!(matches!(o, VerifyOutcome::Stuck { .. }));
    }

    #[tokio::test]
    async fn returns_not_found_when_unit_missing() {
        let s = FixedSystemd { answer: UnitState::NotFound };
        let o = wait_for_running("u", 0, Duration::from_millis(50), &s).await.unwrap();
        assert_eq!(o, VerifyOutcome::NotFound);
    }

    #[tokio::test]
    async fn restarts_diff_from_pre_means_stuck_even_if_active() {
        // n_restarts incremented since pre-op → service crashed-then-relaunched
        // inside the window; treat as stuck rather than success (§7.2).
        let s = FixedSystemd {
            answer: UnitState::Known {
                active_state: "active".into(),
                sub_state: "running".into(),
                load_state: "loaded".into(),
                n_restarts: 3,
                exec_main_status: 0,
            },
        };
        let o = wait_for_running("u", 0, Duration::from_millis(50), &s).await.unwrap();
        assert!(matches!(o, VerifyOutcome::Stuck { .. }));
    }
}
```

- [ ] **Step 2: Run tests**

Run: `cargo test -p boxpilotd service::verify`
Expected: 4 tests pass.

- [ ] **Step 3: Commit**

```bash
git add crates/boxpilotd/src/service/verify.rs
git commit -m "feat(service): wait_for_running helper (plan #5 will compose this)"
```

---

## Task 14: HelperContext gains `journal` field; rename systemd field type

**Files:**
- Modify: `crates/boxpilotd/src/context.rs`
- Modify: `crates/boxpilotd/src/main.rs`

- [ ] **Step 1: Update `HelperContext`**

In `crates/boxpilotd/src/context.rs`:

- Replace `use crate::systemd::SystemdQuery;` with `use crate::systemd::{JournalReader, Systemd};`.
- Change the `systemd` field type to `Arc<dyn Systemd>`.
- Add a new `pub journal: Arc<dyn JournalReader>` field.
- Add `journal: Arc<dyn JournalReader>` to `HelperContext::new`'s parameter list.
- In `pub mod testing`, add `journal: Arc<crate::systemd::testing::FixedJournal>` defaulting to an empty-line journal.

Rendered diff:

```rust
use crate::authority::Authority;
use crate::controller::{ControllerState, UserLookup};
use crate::core::download::Downloader;
use crate::core::github::GithubClient;
use crate::core::trust::{FsMetadataProvider, VersionChecker};
use crate::credentials::CallerResolver;
use crate::paths::Paths;
use crate::systemd::{JournalReader, Systemd};
// …

pub struct HelperContext {
    pub paths: Paths,
    pub callers: Arc<dyn CallerResolver>,
    pub authority: Arc<dyn Authority>,
    pub systemd: Arc<dyn Systemd>,
    pub journal: Arc<dyn JournalReader>,
    pub user_lookup: Arc<dyn UserLookup>,
    pub github: Arc<dyn GithubClient>,
    pub downloader: Arc<dyn Downloader>,
    pub fs_meta: Arc<dyn FsMetadataProvider>,
    pub version_checker: Arc<dyn VersionChecker>,
}

impl HelperContext {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        paths: Paths,
        callers: Arc<dyn CallerResolver>,
        authority: Arc<dyn Authority>,
        systemd: Arc<dyn Systemd>,
        journal: Arc<dyn JournalReader>,
        user_lookup: Arc<dyn UserLookup>,
        github: Arc<dyn GithubClient>,
        downloader: Arc<dyn Downloader>,
        fs_meta: Arc<dyn FsMetadataProvider>,
        version_checker: Arc<dyn VersionChecker>,
    ) -> Self {
        Self {
            paths,
            callers,
            authority,
            systemd,
            journal,
            user_lookup,
            github,
            downloader,
            fs_meta,
            version_checker,
        }
    }
    // load_config / controller_state unchanged.
}
```

In `pub mod testing` (`ctx_with`), add a journal:

```rust
        let journal = Arc::new(crate::systemd::testing::FixedJournal { lines: Vec::new() });
        HelperContext::new(
            paths,
            Arc::new(FixedResolver::with(callers)),
            Arc::new(authority),
            Arc::new(FixedSystemd { answer: systemd_answer }),
            journal,
            Arc::new(PasswdLookup),
            github,
            downloader,
            fs_meta,
            version_checker,
        )
```

Also in `pub mod testing` (`crates/boxpilotd/src/context.rs`):

1. Promote the existing private `struct PermissiveTestFs` to `pub struct PermissiveTestFs` so iface tests can use it.

2. Add a constructor that lets tests inject pre-built journal lines and a pre-built `Arc<RecordingSystemd>`:

```rust
    /// Build a context wired to a caller-supplied `Arc<RecordingSystemd>`
    /// so the test can assert on which verb fired after a method runs.
    /// Returns the ctx; the caller already has the Arc.
    pub fn ctx_with_recording(
        tmp: &TempDir,
        config: Option<&str>,
        authority: CannedAuthority,
        rec: Arc<crate::systemd::testing::RecordingSystemd>,
        callers: &[(&str, u32)],
    ) -> HelperContext {
        let paths = Paths::with_root(tmp.path());
        std::fs::create_dir_all(paths.etc_dir()).unwrap();
        if let Some(text) = config {
            std::fs::write(paths.boxpilot_toml(), text).unwrap();
        }
        let github = Arc::new(crate::core::github::testing::CannedGithubClient {
            latest: Ok("1.10.0".into()),
            sha256sums: Ok(None),
        });
        let downloader = Arc::new(crate::core::download::testing::FixedDownloader::new(Vec::new()));
        let journal = Arc::new(crate::systemd::testing::FixedJournal { lines: Vec::new() });
        HelperContext::new(
            paths,
            Arc::new(FixedResolver::with(callers)),
            Arc::new(authority),
            rec,
            journal,
            Arc::new(PasswdLookup),
            github,
            downloader,
            Arc::new(PermissiveTestFs),
            Arc::new(crate::core::trust::version_testing::FixedVersionChecker::ok(
                "sing-box version 1.10.0",
            )),
        )
    }

    /// Like `ctx_with` but lets the caller seed the journal tail with
    /// canned lines for `service.logs` tests.
    pub fn ctx_with_journal_lines(
        tmp: &TempDir,
        config: Option<&str>,
        authority: CannedAuthority,
        systemd_answer: UnitState,
        callers: &[(&str, u32)],
        lines: Vec<String>,
    ) -> HelperContext {
        let paths = Paths::with_root(tmp.path());
        std::fs::create_dir_all(paths.etc_dir()).unwrap();
        if let Some(text) = config {
            std::fs::write(paths.boxpilot_toml(), text).unwrap();
        }
        let github = Arc::new(crate::core::github::testing::CannedGithubClient {
            latest: Ok("1.10.0".into()),
            sha256sums: Ok(None),
        });
        let downloader = Arc::new(crate::core::download::testing::FixedDownloader::new(Vec::new()));
        let journal = Arc::new(crate::systemd::testing::FixedJournal { lines });
        HelperContext::new(
            paths,
            Arc::new(FixedResolver::with(callers)),
            Arc::new(authority),
            Arc::new(crate::systemd::testing::FixedSystemd { answer: systemd_answer }),
            journal,
            Arc::new(PasswdLookup),
            github,
            downloader,
            Arc::new(PermissiveTestFs),
            Arc::new(crate::core::trust::version_testing::FixedVersionChecker::ok(
                "sing-box version 1.10.0",
            )),
        )
    }
```

- [ ] **Step 2: Update `main.rs`**

In `crates/boxpilotd/src/main.rs`, after the existing trait-object construction, add:

```rust
    let journal = Arc::new(crate::systemd::JournalctlProcess);
```

…and pass it to `HelperContext::new` between the `systemd` Arc and the `controller::PasswdLookup` Arc:

```rust
    let ctx = Arc::new(context::HelperContext::new(
        paths,
        Arc::new(credentials::DBusCallerResolver::new(conn.clone())),
        Arc::new(authority::DBusAuthority::new(conn.clone())),
        Arc::new(systemd::DBusSystemd::new(conn.clone())),
        journal,
        Arc::new(controller::PasswdLookup),
        github,
        downloader,
        fs_meta,
        version_checker,
    ));
```

- [ ] **Step 3: Build the workspace to flush type-name fallout**

Run: `cargo build --workspace`
Expected: clean build. `SystemdQuery` references in tests/iface.rs/other modules need to follow the `Systemd` rename (do them in the next step if the compiler lists them).

If the compiler flags any `Arc<dyn SystemdQuery>` remaining, replace with `Arc<dyn Systemd>` and re-run.

- [ ] **Step 4: Run all tests to verify nothing else regressed**

Run: `cargo test -p boxpilotd`
Expected: all tests still pass.

- [ ] **Step 5: Commit**

```bash
git add crates/boxpilotd/src/context.rs crates/boxpilotd/src/main.rs
git commit -m "refactor(context): rename systemd trait + add JournalReader dependency"
```

---

## Task 15: Wire `service.start` / `stop` / `restart` / `enable` / `disable` in `iface.rs`

**Files:**
- Modify: `crates/boxpilotd/src/iface.rs`

Replace the 5 stubs with real bodies that call `service::control::run`. The proxy method signature stays a JSON string (no request payload — plan-3 verbs take only the implicit unit name).

- [ ] **Step 1: Write the 5 failing tests**

Append to `crates/boxpilotd/src/iface.rs` `mod tests`. The tests use the `ctx_with_recording` helper added in Task 14 — it takes a pre-built `Arc<RecordingSystemd>` so the test can assert on calls *after* the method runs:

```rust
    use crate::context::testing::ctx_with_recording;
    use crate::systemd::testing::{RecordedCall, RecordingSystemd};

    #[tokio::test]
    async fn service_start_calls_systemd_start_unit() {
        let tmp = tempdir().unwrap();
        let rec = Arc::new(RecordingSystemd::new(UnitState::NotFound));
        let ctx = Arc::new(ctx_with_recording(
            &tmp,
            Some("schema_version = 1\ncontroller_uid = 1000\n"),
            CannedAuthority::allowing(&["app.boxpilot.helper.service.start"]),
            rec.clone(),
            &[(":1.42", 1000)],
        ));
        let h = Helper::new(ctx);
        h.do_service_start(":1.42").await.unwrap();
        assert!(rec.calls().iter().any(|c| matches!(c, RecordedCall::StartUnit(_))));
    }

    #[tokio::test]
    async fn service_stop_calls_systemd_stop_unit() {
        let tmp = tempdir().unwrap();
        let rec = Arc::new(RecordingSystemd::new(UnitState::NotFound));
        let ctx = Arc::new(ctx_with_recording(
            &tmp,
            Some("schema_version = 1\ncontroller_uid = 1000\n"),
            CannedAuthority::allowing(&["app.boxpilot.helper.service.stop"]),
            rec.clone(),
            &[(":1.42", 1000)],
        ));
        let h = Helper::new(ctx);
        h.do_service_stop(":1.42").await.unwrap();
        assert!(rec.calls().iter().any(|c| matches!(c, RecordedCall::StopUnit(_))));
    }

    #[tokio::test]
    async fn service_restart_calls_systemd_restart_unit() {
        let tmp = tempdir().unwrap();
        let rec = Arc::new(RecordingSystemd::new(UnitState::NotFound));
        let ctx = Arc::new(ctx_with_recording(
            &tmp,
            Some("schema_version = 1\ncontroller_uid = 1000\n"),
            CannedAuthority::allowing(&["app.boxpilot.helper.service.restart"]),
            rec.clone(),
            &[(":1.42", 1000)],
        ));
        let h = Helper::new(ctx);
        h.do_service_restart(":1.42").await.unwrap();
        assert!(rec.calls().iter().any(|c| matches!(c, RecordedCall::RestartUnit(_))));
    }

    #[tokio::test]
    async fn service_enable_calls_systemd_enable_unit_files() {
        let tmp = tempdir().unwrap();
        let rec = Arc::new(RecordingSystemd::new(UnitState::NotFound));
        let ctx = Arc::new(ctx_with_recording(
            &tmp,
            Some("schema_version = 1\ncontroller_uid = 1000\n"),
            CannedAuthority::allowing(&["app.boxpilot.helper.service.enable"]),
            rec.clone(),
            &[(":1.42", 1000)],
        ));
        let h = Helper::new(ctx);
        h.do_service_enable(":1.42").await.unwrap();
        assert!(rec.calls().iter().any(|c| matches!(c, RecordedCall::EnableUnitFiles(_))));
    }

    #[tokio::test]
    async fn service_disable_calls_systemd_disable_unit_files() {
        let tmp = tempdir().unwrap();
        let rec = Arc::new(RecordingSystemd::new(UnitState::NotFound));
        let ctx = Arc::new(ctx_with_recording(
            &tmp,
            Some("schema_version = 1\ncontroller_uid = 1000\n"),
            CannedAuthority::allowing(&["app.boxpilot.helper.service.disable"]),
            rec.clone(),
            &[(":1.42", 1000)],
        ));
        let h = Helper::new(ctx);
        h.do_service_disable(":1.42").await.unwrap();
        assert!(rec.calls().iter().any(|c| matches!(c, RecordedCall::DisableUnitFiles(_))));
    }
```

- [ ] **Step 2: Replace the 5 stubs and add the 5 do_* bodies**

In `crates/boxpilotd/src/iface.rs`, replace the 5 stub methods (`service_start`, `service_stop`, `service_restart`, `service_enable`, `service_disable`) inside the `#[interface(...)] impl Helper` block:

```rust
    async fn service_start(
        &self,
        #[zbus(header)] header: zbus::message::Header<'_>,
    ) -> zbus::fdo::Result<String> {
        let sender = extract_sender(&header)?;
        let resp = self.do_service_start(&sender).await.map_err(to_zbus_err)?;
        serde_json::to_string(&resp).map_err(|e| {
            zbus::fdo::Error::Failed(format!("app.boxpilot.Helper1.Ipc: serialize: {e}"))
        })
    }

    async fn service_stop(
        &self,
        #[zbus(header)] header: zbus::message::Header<'_>,
    ) -> zbus::fdo::Result<String> {
        let sender = extract_sender(&header)?;
        let resp = self.do_service_stop(&sender).await.map_err(to_zbus_err)?;
        serde_json::to_string(&resp).map_err(|e| {
            zbus::fdo::Error::Failed(format!("app.boxpilot.Helper1.Ipc: serialize: {e}"))
        })
    }

    async fn service_restart(
        &self,
        #[zbus(header)] header: zbus::message::Header<'_>,
    ) -> zbus::fdo::Result<String> {
        let sender = extract_sender(&header)?;
        let resp = self.do_service_restart(&sender).await.map_err(to_zbus_err)?;
        serde_json::to_string(&resp).map_err(|e| {
            zbus::fdo::Error::Failed(format!("app.boxpilot.Helper1.Ipc: serialize: {e}"))
        })
    }

    async fn service_enable(
        &self,
        #[zbus(header)] header: zbus::message::Header<'_>,
    ) -> zbus::fdo::Result<String> {
        let sender = extract_sender(&header)?;
        let resp = self.do_service_enable(&sender).await.map_err(to_zbus_err)?;
        serde_json::to_string(&resp).map_err(|e| {
            zbus::fdo::Error::Failed(format!("app.boxpilot.Helper1.Ipc: serialize: {e}"))
        })
    }

    async fn service_disable(
        &self,
        #[zbus(header)] header: zbus::message::Header<'_>,
    ) -> zbus::fdo::Result<String> {
        let sender = extract_sender(&header)?;
        let resp = self.do_service_disable(&sender).await.map_err(to_zbus_err)?;
        serde_json::to_string(&resp).map_err(|e| {
            zbus::fdo::Error::Failed(format!("app.boxpilot.Helper1.Ipc: serialize: {e}"))
        })
    }
```

In `impl Helper` (the inherent impl, not the `#[interface]` one), add the 5 inner functions. They are structurally near-identical; written out in full because plan-execution agents read tasks out of order and a `// same as above` comment fails them:

```rust
    async fn do_service_start(
        &self,
        sender: &str,
    ) -> Result<boxpilot_ipc::ServiceControlResponse, HelperError> {
        let _call = dispatch::authorize(&self.ctx, sender, HelperMethod::ServiceStart).await?;
        let cfg = self.ctx.load_config().await?;
        crate::service::control::run(
            crate::service::control::Verb::Start,
            &cfg.target_service,
            &*self.ctx.systemd,
        )
        .await
    }

    async fn do_service_stop(
        &self,
        sender: &str,
    ) -> Result<boxpilot_ipc::ServiceControlResponse, HelperError> {
        let _call = dispatch::authorize(&self.ctx, sender, HelperMethod::ServiceStop).await?;
        let cfg = self.ctx.load_config().await?;
        crate::service::control::run(
            crate::service::control::Verb::Stop,
            &cfg.target_service,
            &*self.ctx.systemd,
        )
        .await
    }

    async fn do_service_restart(
        &self,
        sender: &str,
    ) -> Result<boxpilot_ipc::ServiceControlResponse, HelperError> {
        let _call = dispatch::authorize(&self.ctx, sender, HelperMethod::ServiceRestart).await?;
        let cfg = self.ctx.load_config().await?;
        crate::service::control::run(
            crate::service::control::Verb::Restart,
            &cfg.target_service,
            &*self.ctx.systemd,
        )
        .await
    }

    async fn do_service_enable(
        &self,
        sender: &str,
    ) -> Result<boxpilot_ipc::ServiceControlResponse, HelperError> {
        let _call = dispatch::authorize(&self.ctx, sender, HelperMethod::ServiceEnable).await?;
        let cfg = self.ctx.load_config().await?;
        crate::service::control::run(
            crate::service::control::Verb::Enable,
            &cfg.target_service,
            &*self.ctx.systemd,
        )
        .await
    }

    async fn do_service_disable(
        &self,
        sender: &str,
    ) -> Result<boxpilot_ipc::ServiceControlResponse, HelperError> {
        let _call = dispatch::authorize(&self.ctx, sender, HelperMethod::ServiceDisable).await?;
        let cfg = self.ctx.load_config().await?;
        crate::service::control::run(
            crate::service::control::Verb::Disable,
            &cfg.target_service,
            &*self.ctx.systemd,
        )
        .await
    }
```

- [ ] **Step 3: Run tests**

Run: `cargo test -p boxpilotd iface::tests`
Expected: all existing iface tests + 5 new ones pass.

- [ ] **Step 4: Commit**

```bash
git add crates/boxpilotd/src/iface.rs
git commit -m "feat(iface): wire service.start/stop/restart/enable/disable"
```

---

## Task 16: Wire `service.install_managed` and `service.logs` in `iface.rs`

**Files:**
- Modify: `crates/boxpilotd/src/iface.rs`

`service.install_managed` is the only `service.*` action that may claim the controller (it's mutating, and a fresh box has no controller yet). Use `dispatch::maybe_claim_controller`.

`service.logs` takes a JSON request body (the `lines` count).

- [ ] **Step 1: Write the failing tests**

Append to `crates/boxpilotd/src/iface.rs` `mod tests`:

```rust
    use crate::context::testing::ctx_with_journal_lines;

    #[tokio::test]
    async fn service_install_managed_writes_unit_when_core_path_set() {
        let tmp = tempdir().unwrap();
        let rec = Arc::new(RecordingSystemd::new(UnitState::Known {
            active_state: "inactive".into(),
            sub_state: "dead".into(),
            load_state: "loaded".into(),
            n_restarts: 0,
            exec_main_status: 0,
        }));
        // /usr/bin/sing-box is in the §6.5 default-allowed prefix list and
        // PermissiveTestFs reports it as root-owned 0o755, so the trust
        // check passes inside the test.
        let ctx = Arc::new(ctx_with_recording(
            &tmp,
            Some("schema_version = 1\ncore_path = \"/usr/bin/sing-box\"\ncore_state = \"managed-installed\"\ncontroller_uid = 1000\n"),
            CannedAuthority::allowing(&["app.boxpilot.helper.service.install-managed"]),
            rec.clone(),
            &[(":1.42", 1000)],
        ));
        let h = Helper::new(ctx);
        let resp = h.do_service_install_managed(":1.42").await.unwrap();
        assert!(resp.generated_unit_path.ends_with("etc/systemd/system/boxpilot-sing-box.service"));
        assert!(rec.calls().iter().any(|c| matches!(c, RecordedCall::Reload)));
    }

    #[tokio::test]
    async fn service_logs_returns_journal_lines() {
        let tmp = tempdir().unwrap();
        let ctx = Arc::new(ctx_with_journal_lines(
            &tmp,
            Some("schema_version = 1\ncontroller_uid = 1000\n"),
            CannedAuthority::allowing(&["app.boxpilot.helper.service.logs"]),
            UnitState::NotFound,
            &[(":1.42", 1000)],
            vec!["entry1".into(), "entry2".into()],
        ));
        let h = Helper::new(ctx);
        let req = boxpilot_ipc::ServiceLogsRequest { lines: 5 };
        let resp = h.do_service_logs(":1.42", req).await.unwrap();
        assert_eq!(resp.lines, vec!["entry1".to_string(), "entry2".to_string()]);
    }
```

- [ ] **Step 2: Add the proxy method bodies**

In `crates/boxpilotd/src/iface.rs`, replace the `service_install_managed` stub:

```rust
    async fn service_install_managed(
        &self,
        #[zbus(header)] header: zbus::message::Header<'_>,
    ) -> zbus::fdo::Result<String> {
        let sender = extract_sender(&header)?;
        let resp = self
            .do_service_install_managed(&sender)
            .await
            .map_err(to_zbus_err)?;
        serde_json::to_string(&resp).map_err(|e| {
            zbus::fdo::Error::Failed(format!("app.boxpilot.Helper1.Ipc: serialize: {e}"))
        })
    }
```

…and the `service_logs` stub:

```rust
    async fn service_logs(
        &self,
        #[zbus(header)] header: zbus::message::Header<'_>,
        request_json: String,
    ) -> zbus::fdo::Result<String> {
        let sender = extract_sender(&header)?;
        let req: boxpilot_ipc::ServiceLogsRequest =
            serde_json::from_str(&request_json).map_err(|e| {
                zbus::fdo::Error::Failed(format!("app.boxpilot.Helper1.Ipc: parse: {e}"))
            })?;
        let resp = self
            .do_service_logs(&sender, req)
            .await
            .map_err(to_zbus_err)?;
        serde_json::to_string(&resp).map_err(|e| {
            zbus::fdo::Error::Failed(format!("app.boxpilot.Helper1.Ipc: serialize: {e}"))
        })
    }
```

In `impl Helper`:

```rust
    async fn do_service_install_managed(
        &self,
        sender: &str,
    ) -> Result<boxpilot_ipc::ServiceInstallManagedResponse, HelperError> {
        let call = dispatch::authorize(&self.ctx, sender, HelperMethod::ServiceInstallManaged).await?;
        let controller = dispatch::maybe_claim_controller(
            call.will_claim_controller,
            call.caller_uid,
            &*self.ctx.user_lookup,
        )?;
        let cfg = self.ctx.load_config().await?;
        let deps = crate::service::install::InstallDeps {
            paths: self.ctx.paths.clone(),
            systemd: &*self.ctx.systemd,
            fs: &*self.ctx.fs_meta,
        };
        let mut resp = crate::service::install::install_managed(&cfg, &deps, controller.clone()).await?;
        // The body returned `claimed_controller=false`; the iface layer is the
        // canonical place to fill it from the dispatch decision. (We purposely
        // do NOT call StateCommit here — install_managed does not change
        // boxpilot.toml's content; controller-name is written by the next
        // mutating call that *does* go through StateCommit. This matches
        // plan #2's `core.install_managed` flow which is the canonical
        // controller-claiming entry point on first install.)
        resp.claimed_controller = controller.is_some();

        // If `controller` is Some, we still need to persist controller-name +
        // boxpilot.toml + polkit drop-in *before* this method returns —
        // otherwise a `service.install_managed` on a fresh box would leave
        // controller-uid unwritten. Use a minimal StateCommit that touches
        // only those three files.
        if let Some(c) = controller {
            let commit = crate::core::commit::StateCommit {
                paths: self.ctx.paths.clone(),
                toml_updates: crate::core::commit::TomlUpdates::default(),
                controller: Some(c),
                install_state: boxpilot_ipc::InstallState::empty(),
                current_symlink_target: None,
            };
            // The empty InstallState rewrites install-state.json to its empty
            // form. That is acceptable — install-state.json is the cores
            // ledger and an empty one matches the no-managed-cores reality
            // of this fresh-box path. Plan #2's core.install_managed will
            // populate it on the first managed-core install.
            commit.apply().await?;
        }

        Ok(resp)
    }

    async fn do_service_logs(
        &self,
        sender: &str,
        req: boxpilot_ipc::ServiceLogsRequest,
    ) -> Result<boxpilot_ipc::ServiceLogsResponse, HelperError> {
        let _call = dispatch::authorize(&self.ctx, sender, HelperMethod::ServiceLogs).await?;
        let cfg = self.ctx.load_config().await?;
        crate::service::logs::read(&req, &cfg.target_service, &*self.ctx.journal).await
    }
```

**Note on the `StateCommit` corner case:** if a controller is being claimed *for the first time* via `service.install_managed` on a box that has no `install-state.json` yet, writing an empty ledger here would conflict with a later `core.install_managed` that writes a non-empty one. Add a guard inside `apply()` that *reads* the existing install-state.json (if any) and merges only the `current_managed_core` field into the staged write when we're not actively touching cores. The simplest correct guard is to **load** the on-disk state when the caller passes `InstallState::empty()` and re-emit it. Implement that in `StateCommit::apply()`:

```rust
        // 1a (revised). If the caller passed an empty install_state and a
        // non-empty one is already on disk, preserve it — we're not changing
        // the cores ledger here.
        let install_state_to_write = if self.install_state == InstallState::empty()
            && install_state_path.exists()
        {
            crate::core::state::read_state(&install_state_path).await?
        } else {
            self.install_state.clone()
        };
        // … then write `install_state_to_write.to_json()` to install_state_tmp.
```

This requires `InstallState: PartialEq` — already derived in plan #2.

- [ ] **Step 3: Run tests**

Run: `cargo test -p boxpilotd iface`
Expected: all existing tests + 2 new ones pass; the install_managed test verifies the unit file is written and `daemon-reload` fired.

- [ ] **Step 4: Commit**

```bash
git add crates/boxpilotd/src/iface.rs crates/boxpilotd/src/context.rs crates/boxpilotd/src/core/commit.rs
git commit -m "feat(iface): wire service.install_managed + service.logs"
```

---

## Task 17: Tauri proxy methods

**Files:**
- Modify: `crates/boxpilot-tauri/src/helper_client.rs`

- [ ] **Step 1: Add the 7 new proxy methods**

In the `#[proxy(...)] trait Helper { … }` block, after the `core_adopt` line, append:

```rust
    #[zbus(name = "ServiceStart")]
    fn service_start(&self) -> zbus::Result<String>;
    #[zbus(name = "ServiceStop")]
    fn service_stop(&self) -> zbus::Result<String>;
    #[zbus(name = "ServiceRestart")]
    fn service_restart(&self) -> zbus::Result<String>;
    #[zbus(name = "ServiceEnable")]
    fn service_enable(&self) -> zbus::Result<String>;
    #[zbus(name = "ServiceDisable")]
    fn service_disable(&self) -> zbus::Result<String>;
    #[zbus(name = "ServiceInstallManaged")]
    fn service_install_managed(&self) -> zbus::Result<String>;
    #[zbus(name = "ServiceLogs")]
    fn service_logs(&self, request_json: &str) -> zbus::Result<String>;
```

- [ ] **Step 2: Add the 7 client wrappers**

After `pub async fn core_adopt(...)`:

```rust
    pub async fn service_start(&self) -> Result<boxpilot_ipc::ServiceControlResponse, ClientError> {
        let proxy = HelperProxy::new(&self.conn).await?;
        let json = proxy.service_start().await?;
        serde_json::from_str(&json).map_err(|e| ClientError::Decode(e.to_string()))
    }

    pub async fn service_stop(&self) -> Result<boxpilot_ipc::ServiceControlResponse, ClientError> {
        let proxy = HelperProxy::new(&self.conn).await?;
        let json = proxy.service_stop().await?;
        serde_json::from_str(&json).map_err(|e| ClientError::Decode(e.to_string()))
    }

    pub async fn service_restart(&self) -> Result<boxpilot_ipc::ServiceControlResponse, ClientError> {
        let proxy = HelperProxy::new(&self.conn).await?;
        let json = proxy.service_restart().await?;
        serde_json::from_str(&json).map_err(|e| ClientError::Decode(e.to_string()))
    }

    pub async fn service_enable(&self) -> Result<boxpilot_ipc::ServiceControlResponse, ClientError> {
        let proxy = HelperProxy::new(&self.conn).await?;
        let json = proxy.service_enable().await?;
        serde_json::from_str(&json).map_err(|e| ClientError::Decode(e.to_string()))
    }

    pub async fn service_disable(&self) -> Result<boxpilot_ipc::ServiceControlResponse, ClientError> {
        let proxy = HelperProxy::new(&self.conn).await?;
        let json = proxy.service_disable().await?;
        serde_json::from_str(&json).map_err(|e| ClientError::Decode(e.to_string()))
    }

    pub async fn service_install_managed(
        &self,
    ) -> Result<boxpilot_ipc::ServiceInstallManagedResponse, ClientError> {
        let proxy = HelperProxy::new(&self.conn).await?;
        let json = proxy.service_install_managed().await?;
        serde_json::from_str(&json).map_err(|e| ClientError::Decode(e.to_string()))
    }

    pub async fn service_logs(
        &self,
        req: &boxpilot_ipc::ServiceLogsRequest,
    ) -> Result<boxpilot_ipc::ServiceLogsResponse, ClientError> {
        let proxy = HelperProxy::new(&self.conn).await?;
        let json = proxy
            .service_logs(&serde_json::to_string(req).unwrap())
            .await?;
        serde_json::from_str(&json).map_err(|e| ClientError::Decode(e.to_string()))
    }
```

- [ ] **Step 3: Build**

Run: `cargo build -p boxpilot`
Expected: clean build.

- [ ] **Step 4: Commit**

```bash
git add crates/boxpilot-tauri/src/helper_client.rs
git commit -m "feat(tauri): zbus proxy + client wrappers for service.* methods"
```

---

## Task 18: Tauri commands + invoke_handler registration

**Files:**
- Modify: `crates/boxpilot-tauri/src/commands.rs`
- Modify: `crates/boxpilot-tauri/src/lib.rs`

- [ ] **Step 1: Add the 7 commands**

Append to `crates/boxpilot-tauri/src/commands.rs`:

```rust
#[tauri::command]
pub async fn helper_service_start(
) -> Result<boxpilot_ipc::ServiceControlResponse, CommandError> {
    let c = HelperClient::connect().await?;
    Ok(c.service_start().await?)
}

#[tauri::command]
pub async fn helper_service_stop(
) -> Result<boxpilot_ipc::ServiceControlResponse, CommandError> {
    let c = HelperClient::connect().await?;
    Ok(c.service_stop().await?)
}

#[tauri::command]
pub async fn helper_service_restart(
) -> Result<boxpilot_ipc::ServiceControlResponse, CommandError> {
    let c = HelperClient::connect().await?;
    Ok(c.service_restart().await?)
}

#[tauri::command]
pub async fn helper_service_enable(
) -> Result<boxpilot_ipc::ServiceControlResponse, CommandError> {
    let c = HelperClient::connect().await?;
    Ok(c.service_enable().await?)
}

#[tauri::command]
pub async fn helper_service_disable(
) -> Result<boxpilot_ipc::ServiceControlResponse, CommandError> {
    let c = HelperClient::connect().await?;
    Ok(c.service_disable().await?)
}

#[tauri::command]
pub async fn helper_service_install_managed(
) -> Result<boxpilot_ipc::ServiceInstallManagedResponse, CommandError> {
    let c = HelperClient::connect().await?;
    Ok(c.service_install_managed().await?)
}

#[tauri::command]
pub async fn helper_service_logs(
    request: boxpilot_ipc::ServiceLogsRequest,
) -> Result<boxpilot_ipc::ServiceLogsResponse, CommandError> {
    let c = HelperClient::connect().await?;
    Ok(c.service_logs(&request).await?)
}
```

- [ ] **Step 2: Register in `lib.rs`**

Update `tauri::generate_handler!` in `crates/boxpilot-tauri/src/lib.rs`:

```rust
        .invoke_handler(tauri::generate_handler![
            commands::helper_service_status,
            commands::helper_ping,
            commands::helper_core_discover,
            commands::helper_core_install_managed,
            commands::helper_core_upgrade_managed,
            commands::helper_core_rollback_managed,
            commands::helper_core_adopt,
            commands::helper_service_start,
            commands::helper_service_stop,
            commands::helper_service_restart,
            commands::helper_service_enable,
            commands::helper_service_disable,
            commands::helper_service_install_managed,
            commands::helper_service_logs,
        ])
```

- [ ] **Step 3: Build**

Run: `cargo build -p boxpilot`
Expected: clean build.

- [ ] **Step 4: Commit**

```bash
git add crates/boxpilot-tauri/src/commands.rs crates/boxpilot-tauri/src/lib.rs
git commit -m "feat(tauri): expose service.* commands to the frontend"
```

---

## Task 19: Frontend TS types + api wrappers

**Files:**
- Modify: `frontend/src/api/types.ts`
- Modify: `frontend/src/api/helper.ts`

- [ ] **Step 1: Add the TS mirrors**

Append to `frontend/src/api/types.ts`:

```ts
export interface ServiceControlResponse { unit_state: UnitState; }

export interface ServiceInstallManagedResponse {
  unit_state: UnitState;
  generated_unit_path: string;
  claimed_controller: boolean;
}

export interface ServiceLogsRequest { lines: number; }
export interface ServiceLogsResponse {
  lines: string[];
  truncated: boolean;
}
```

- [ ] **Step 2: Add the invoke wrappers**

Append to `frontend/src/api/helper.ts`:

```ts
import type {
  ServiceControlResponse, ServiceInstallManagedResponse,
  ServiceLogsRequest, ServiceLogsResponse,
} from "./types";

export async function serviceStart(): Promise<ServiceControlResponse> {
  return await invoke<ServiceControlResponse>("helper_service_start");
}
export async function serviceStop(): Promise<ServiceControlResponse> {
  return await invoke<ServiceControlResponse>("helper_service_stop");
}
export async function serviceRestart(): Promise<ServiceControlResponse> {
  return await invoke<ServiceControlResponse>("helper_service_restart");
}
export async function serviceEnable(): Promise<ServiceControlResponse> {
  return await invoke<ServiceControlResponse>("helper_service_enable");
}
export async function serviceDisable(): Promise<ServiceControlResponse> {
  return await invoke<ServiceControlResponse>("helper_service_disable");
}
export async function serviceInstallManaged(): Promise<ServiceInstallManagedResponse> {
  return await invoke<ServiceInstallManagedResponse>("helper_service_install_managed");
}
export async function serviceLogs(req: ServiceLogsRequest): Promise<ServiceLogsResponse> {
  return await invoke<ServiceLogsResponse>("helper_service_logs", { request: req });
}
```

(The existing top-of-file `import` already covers the existing types; just merge the new symbols into one combined import statement when applying the diff.)

- [ ] **Step 3: Type-check**

Run: `cd frontend && npm install && npm run build`
Expected: clean build, no TS errors.

- [ ] **Step 4: Commit**

```bash
git add frontend/src/api/types.ts frontend/src/api/helper.ts
git commit -m "feat(frontend): TS mirrors + invoke wrappers for service.* commands"
```

---

## Task 20: `ServicePanel.vue` component

**Files:**
- Create: `frontend/src/components/ServicePanel.vue`

- [ ] **Step 1: Write the component**

Create `frontend/src/components/ServicePanel.vue`:

```vue
<script setup lang="ts">
import { onMounted, ref } from "vue";
import {
  serviceStatus, serviceStart, serviceStop, serviceRestart,
  serviceEnable, serviceDisable, serviceInstallManaged, serviceLogs,
} from "../api/helper";
import type { ServiceStatusResponse, ServiceLogsResponse } from "../api/types";

const status = ref<ServiceStatusResponse | null>(null);
const logs = ref<ServiceLogsResponse | null>(null);
const busy = ref(false);
const error = ref<string | null>(null);

async function refresh() {
  busy.value = true; error.value = null;
  try { status.value = await serviceStatus(); }
  catch (e: any) { error.value = e?.message ?? String(e); }
  finally { busy.value = false; }
}

async function run<T>(fn: () => Promise<T>) {
  busy.value = true; error.value = null;
  try { await fn(); await refresh(); }
  catch (e: any) { error.value = e?.message ?? String(e); }
  finally { busy.value = false; }
}

async function loadLogs() {
  busy.value = true; error.value = null;
  try { logs.value = await serviceLogs({ lines: 200 }); }
  catch (e: any) { error.value = e?.message ?? String(e); }
  finally { busy.value = false; }
}

onMounted(refresh);
</script>

<template>
  <section class="service-panel">
    <h2>Service</h2>
    <p v-if="error" class="err">{{ error }}</p>
    <pre v-if="status">{{ JSON.stringify(status, null, 2) }}</pre>
    <div class="actions">
      <button :disabled="busy" @click="refresh">Refresh</button>
      <button :disabled="busy" @click="run(serviceInstallManaged)">Install unit</button>
      <button :disabled="busy" @click="run(serviceEnable)">Enable</button>
      <button :disabled="busy" @click="run(serviceDisable)">Disable</button>
      <button :disabled="busy" @click="run(serviceStart)">Start</button>
      <button :disabled="busy" @click="run(serviceStop)">Stop</button>
      <button :disabled="busy" @click="run(serviceRestart)">Restart</button>
    </div>
    <h3>Logs</h3>
    <button :disabled="busy" @click="loadLogs">Tail last 200 lines</button>
    <pre v-if="logs" class="logs">{{ logs.lines.join("\n") }}</pre>
  </section>
</template>

<style scoped>
.service-panel { padding: 1rem; }
.actions { display: flex; flex-wrap: wrap; gap: 0.5rem; margin: 1rem 0; }
.actions button { padding: 0.5rem 1rem; }
.err { color: #c00; }
.logs { max-height: 24rem; overflow: auto; background: #111; color: #eee; padding: 0.5rem; font-size: 0.85em; }
</style>
```

- [ ] **Step 2: Type-check**

Run: `cd frontend && npm run build`
Expected: clean build.

- [ ] **Step 3: Commit**

```bash
git add frontend/src/components/ServicePanel.vue
git commit -m "feat(frontend): ServicePanel.vue with 6 verbs + log tail"
```

---

## Task 21: Mount ServicePanel under Home tab

**Files:**
- Modify: `frontend/src/App.vue`

- [ ] **Step 1: Replace the inline Home check with `<ServicePanel />`**

Edit `frontend/src/App.vue`:

```vue
<script setup lang="ts">
import { ref } from "vue";
import CoresPanel from "./components/CoresPanel.vue";
import ServicePanel from "./components/ServicePanel.vue";

type Tab = "home" | "cores";
const tab = ref<Tab>("home");
</script>

<template>
  <main>
    <h1>BoxPilot</h1>
    <nav>
      <button :class="{ active: tab === 'home' }" @click="tab = 'home'">Home</button>
      <button :class="{ active: tab === 'cores' }" @click="tab = 'cores'">Settings → Cores</button>
    </nav>
    <ServicePanel v-if="tab === 'home'" />
    <CoresPanel v-else-if="tab === 'cores'" />
  </main>
</template>

<style>
body { font-family: system-ui, sans-serif; padding: 2rem; max-width: 60rem; margin: auto; }
nav { display: flex; gap: 0.5rem; margin: 1rem 0; }
nav button { padding: 0.5rem 1rem; }
nav button.active { background: #333; color: #fff; }
</style>
```

The previous `serviceStatus`/`error`/`loading` refs and the inline `<button>Check service.status</button>` move into `ServicePanel.vue` (already done in Task 20). Drop the now-orphaned imports.

- [ ] **Step 2: Type-check**

Run: `cd frontend && npm run build`
Expected: clean build.

- [ ] **Step 3: Commit**

```bash
git add frontend/src/App.vue
git commit -m "feat(frontend): mount ServicePanel under Home"
```

---

## Task 22: Final workspace test sweep

**Files:** none (verification step).

- [ ] **Step 1: Run the full test suite**

Run: `cargo test --workspace`
Expected: all tests pass. Plan #2 had 71 tests; this plan adds roughly:
- 5 (Task 1: IPC types)
- 1 (Task 2: RecordingSystemd round-trip)
- 1 (Task 4: FixedJournal round-trip)
- 2 (Task 5: paths)
- 4 (Task 6: unit template)
- 3 (Task 8: StateCommit drop-in)
- 3 (Task 10: install_managed)
- 5 (Task 11: control)
- 3 (Task 12: logs)
- 4 (Task 13: verify)
- 5 (Task 15: iface control)
- 2 (Task 16: iface install + logs)

…for ~38 new tests, taking the total to ~109. Numbers may shift by ±5 depending on how the dispatch tests refactor.

- [ ] **Step 2: Frontend type-check + build**

Run: `cd frontend && npm run build`
Expected: clean build with no warnings.

- [ ] **Step 3: Run `cargo clippy` workspace-wide**

Run: `cargo clippy --workspace --all-targets -- -D warnings`
Expected: clean.

- [ ] **Step 4: Commit nothing (verification only); proceed to smoke procedure**

---

## Task 23: Manual smoke procedure doc

**Files:**
- Create: `docs/superpowers/plans/2026-04-29-managed-service-smoke-procedure.md`

- [ ] **Step 1: Write the smoke procedure**

Create `docs/superpowers/plans/2026-04-29-managed-service-smoke-procedure.md`:

```markdown
# Plan #3 manual smoke procedure

Run on a Debian/Ubuntu desktop after Task 22 passes, with a polkit
agent active and at least one managed core already installed (run the
plan #2 smoke procedure through step 2 first).

## Reinstall the helper from this branch

```bash
sudo make install-helper
```

## 1. Install the unit (claims controller if not yet claimed)

Run `make run-gui`, navigate to **Home**, click **Install unit**.

Expected:
- polkit prompt for admin auth (XML default `auth_admin_keep`).
- Panel updates to show `unit_state: { kind: "known", load_state: "loaded", … }`.
- `cat /etc/systemd/system/boxpilot-sing-box.service` matches the §7.1 template
  with `ExecStart=/var/lib/boxpilot/cores/current/sing-box run -c config.json`.
- `cat /etc/polkit-1/rules.d/48-boxpilot-controller.rules` shows
  `var BOXPILOT_CONTROLLER = "<your-username>";`.

## 2. Enable + Start (will fail to start until plan #5 adds a profile)

Click **Enable** → expect no prompt (controller cached after Task 1).
Click **Start** → expect the unit to enter `failed` because
`/etc/boxpilot/active/config.json` does not exist yet.

```bash
systemctl status boxpilot-sing-box.service
```

Expected: `Loaded`, but `Active: failed`. This confirms the unit is in
the right place and the sandbox loaded; it's just that activation (plan
#5) hasn't supplied a config yet.

## 3. Tail logs

Click **Tail last 200 lines**.

Expected: ~10–20 lines from journald showing the failed-start attempts
plus systemd's restart messages. No spawn errors.

## 4. Stop + Disable

Click **Stop** → unit goes to `inactive (dead)`.
Click **Disable** → `systemctl is-enabled boxpilot-sing-box.service`
prints `disabled`.

## 5. polkit perf check

```bash
time gdbus call --system --dest app.boxpilot.Helper \
  --object-path /app/boxpilot/Helper \
  --method app.boxpilot.Helper1.ServiceStatus
```

Expected: < 50 ms wall time (was ~150 ms with `polkit.spawn(cat)` per call).

## 6. Negative test: missing core_path

```bash
sudo cp /etc/boxpilot/boxpilot.toml /tmp/boxpilot.toml.bak
sudo sh -c 'sed -i "/^core_path/d; /^core_state/d" /etc/boxpilot/boxpilot.toml'
```

In the GUI click **Install unit** again. Expected: error toast naming
"no core configured — install or adopt a core first". `cat /etc/systemd/system/boxpilot-sing-box.service`
is unchanged (no half-write).

Restore:

```bash
sudo cp /tmp/boxpilot.toml.bak /etc/boxpilot/boxpilot.toml
```

## 7. Cleanup

```bash
sudo systemctl disable --now boxpilot-sing-box.service || true
sudo rm -f /etc/systemd/system/boxpilot-sing-box.service
sudo systemctl daemon-reload
```
```

- [ ] **Step 2: Commit**

```bash
git add docs/superpowers/plans/2026-04-29-managed-service-smoke-procedure.md
git commit -m "docs(plan-3): manual smoke procedure"
```

---

## Final checks before opening the PR

- [ ] All 23 tasks above are committed on the `managed-service` branch.
- [ ] `cargo test --workspace` passes locally.
- [ ] `cargo clippy --workspace --all-targets -- -D warnings` passes.
- [ ] `cd frontend && npm run build` passes.
- [ ] Manual smoke procedure (Task 23 doc) ran cleanly on a real systemd box.
- [ ] PR title: `feat: managed boxpilot-sing-box.service + service verbs (plan #3)`.
- [ ] PR body lists which §6.3 actions moved from stub → implemented (7 of them) and which spec sections changed status (§6.3 service.*, §7.1 unit content, §6.3 polkit JS rule).
