# BoxPilot Windows Port — Sub-project #2 Design

**Status:** Draft for review (brainstormed 2026-05-02).
**Cross-reference:** Sub-project #1 spec at `docs/superpowers/specs/2026-05-01-boxpilot-platform-abstraction-design.md`. Sub-project #1 plan at `docs/superpowers/plans/2026-05-01-boxpilot-platform-abstraction.md`. Sub-project #1 merged as commit `1547946` on `main` (PR #23).

---

## §1 Goal & Scope

Land the Windows port to **public-beta-ready** quality. Sub-project #1 delivered the trait surface, the AC5-critical Windows path (Authority pass-through, FileLock, IpcServer/Client, Windows Service entry), and Linux verbatim moves. Sub-project #2 fills in the rest: real Windows verb implementations, full-workspace Windows compile, real authorization, and platform-aware GUI copy. Linux behavior remains bit-for-bit unchanged.

**In scope (A–H from the brainstorm):**

- **A.** Real Windows implementations of all helper verbs currently returning `HelperError::NotImplemented` (service.{install_managed, start, stop, restart, enable, disable, status, logs}, core.{install_managed, upgrade_managed, rollback_managed, adopt, discover}, profile.{activate_bundle, rollback_release}, controller.transfer, legacy.{migrate_service, observe_service}, diagnostics.export_redacted, home.status).
- **B.** `boxpilotd` full-workspace Windows compile. CI gate widened to the entire crate set.
- **C.** Real `AclFsPermissions` Windows impl (`SetEntriesInAclW` + current-user SID + ACE for owner-only access).
- **D.** SCM-shape redesign of `ServiceManager` trait: keep method set, extend `UnitState` with platform-tagged extras.
- **E.** `boxpilot.toml` schema bump v1 → v2: `controller_uid: u32` → `controller_principal: String` (tagged "linux:1000" / "windows:S-1-5-...") + automatic forward migration.
- **F.** Windows Authority real impl: Group + controller-SID model with built-in per-verb authorization table; replaces the `AlwaysAllowAuthority` startup-warn placeholder.
- **G.** GUI text platform-awareness: helper exposes `platform.info` verb; Vue i18n keys gain platform variants for the few strings that name OS-specific subsystems.
- **H.** `build_tempfile_aux` Windows impl: `tempfile::NamedTempFile` under `%LocalAppData%\BoxPilot\aux\`, owner-only ACL, sha256 integrity replaces Linux F_SEAL_* semantics.

**Out of scope (Sub-project #3):**

- Windows installer (MSI / MSIX / NSIS).
- Wintun driver bundling, TUN configuration.
- macOS support.
- Microsoft Store packaging policy compliance work.

---

## §2 Locked Design Choices

These are settled and feed directly into the implementation plan.

### §2.1 Authority Model — Group + controller SID (Brainstorm option F.1)

- `boxpilotd.exe` runs under `LocalSystem` (SCM default).
- GUI runs as the logged-in user. Application manifest uses `asInvoker` (no automatic UAC). The GUI may relaunch elevated via `ShellExecute "runas"` for first-time setup, but day-to-day operation does not require it.
- Helper extracts the caller SID from the Named Pipe token chain (already implemented in PR #23: `GetNamedPipeClientProcessId` → `OpenProcessToken` → `GetTokenInformation`).
- Helper consults a hard-coded per-verb authorization table indexed on `HelperMethod::auth_class()`:
  | `auth_class` | Allowed when |
  |---|---|
  | `ReadOnly` | any logged-in user (caller SID resolves to a real user; no further check) |
  | `Mutating` | caller SID == `controller_principal` SID **OR** caller is member of `Administrators` (well-known SID `S-1-5-32-544`) |
  | `HighRisk` | caller is member of `Administrators` only |
- Group check via `CheckTokenMembership` against the pre-baked Administrators well-known SID.
- The `Authority` trait stays platform-agnostic: `check(action_id, &CallerPrincipal) -> bool`. Linux impl is the existing `DBusAuthority`. Windows impl is the new `WindowsLocalAuthority` with the table above.
- The `AlwaysAllowAuthority` stub from Sub-project #1 is deleted.

### §2.2 Schema Migration — v2 + auto forward-migration

- New top-level field `controller_principal: String` with tag-prefixed value:
  - Linux: `"linux:1000"` (uid as string)
  - Windows: `"windows:S-1-5-21-1234567890-1234567890-1234567890-1001"`
- `schema_version` bumps `1 → 2`.
- The `controller_uid: Option<u32>` field is **removed** from `BoxpilotConfig` (not deprecated-and-kept).
- Read-side migration in `BoxpilotConfig::parse`:
  - On `schema_version == 1`: read `controller_uid` if present, build `controller_principal = format!("linux:{uid}")`, set `schema_version = 2`, return the migrated struct.
  - On `schema_version == 2`: parse normally.
  - On `schema_version > 2`: error (existing `UnsupportedSchemaVersion { got }` path).
- Write-side: helper writes the migrated struct back through the existing `StateCommit::apply` atomic-rename path on the next mutating call. (No separate "migration write" — the migration is implicit; the v1 file just keeps being read as v2 until something else changes state.)
- `CallerPrincipal::to_toml_tag()` and `from_toml_tag()` helpers in `boxpilot-platform/src/traits/authority.rs` produce/parse the tagged form.
- `dispatch::ControllerWrites` becomes `{ principal: String /* tag-form */, username: String }`. Username on Windows is the SAM account name resolved via `LookupAccountSidW`.

---

## §3 SCM Trait Redesign

Goal: keep the existing `ServiceManager` (alias `Systemd`) method set, extend `UnitState` so SCM can populate it without lossy mapping. GUI continues to read `active_state` for primary state display; `platform_extra` carries platform-specific richness for diagnostics and logs.

### §3.1 New `UnitState` shape

The `UnitState` enum carried in `ServiceControlResponse.unit_state` and `service.status` responses gains a `platform_extra` field. The wire shape is not version-tagged (no explicit schema_version on this response), so consumers must accept the new field; the only consumer is the Vue GUI, updated in batch ④.

```rust
// crates/boxpilot-ipc/src/service.rs (or wherever UnitState lives)
pub enum UnitState {
    NotFound,
    Known {
        active_state: String,    // platform-neutral: "active" / "inactive" / "transitioning" / "paused"
        sub_state: String,       // Linux: existing sub_state semantics. Windows: SCM current_state lower-snake (running, stopped, start_pending, stop_pending, pause_pending, paused, continue_pending)
        load_state: String,      // Linux: loaded / not-found / .... Windows: always "loaded" once SCM acks the service exists.
        n_restarts: u32,         // Linux: NRestarts from systemd. Windows: best-effort from SERVICE_FAIL_ACTIONS or 0 if not configured.
        exec_main_status: i32,   // Linux: ExecMainStatus. Windows: win32_exit_code (or service_specific_exit_code if win32 == ERROR_SERVICE_SPECIFIC_ERROR).
        platform_extra: PlatformUnitExtra,  // NEW
    },
}

pub enum PlatformUnitExtra {
    Linux,
    Windows {
        check_point: u32,
        wait_hint_ms: u32,
        controls_accepted: u32,  // bitmask: SERVICE_ACCEPT_STOP, SERVICE_ACCEPT_PAUSE_CONTINUE, ...
    },
}
```

### §3.2 Windows SCM call mapping

| Trait method | SCM call(s) |
|---|---|
| `start_unit(name)` | `OpenSCManagerW` → `OpenServiceW(name, SERVICE_START)` → `StartServiceW` → poll `QueryServiceStatusEx` until state = RUNNING or wait_hint exceeded |
| `stop_unit(name)` | `OpenServiceW(name, SERVICE_STOP)` → `ControlService(SERVICE_CONTROL_STOP)` → poll until state = STOPPED |
| `restart_unit(name)` | stop_unit + start_unit (no atomic restart in SCM) |
| `enable_unit_files([name])` | `OpenServiceW(name, SERVICE_CHANGE_CONFIG)` → `ChangeServiceConfigW(start_type=SERVICE_AUTO_START)` |
| `disable_unit_files([name])` | `ChangeServiceConfigW(start_type=SERVICE_DISABLED)` |
| `unit_state(name)` | `QueryServiceStatusEx(SC_STATUS_PROCESS_INFO)` + `QueryServiceConfigW` for load_state/start_type. Maps SCM `dwCurrentState` to active_state: RUNNING → active, STOPPED → inactive, START_PENDING/STOP_PENDING/PAUSE_PENDING/CONTINUE_PENDING → transitioning, PAUSED → paused. |
| `fragment_path(name)` | `QueryServiceConfigW` → `lpBinaryPathName`. (Windows has no "fragment" concept; the service binary path is the closest analog.) |
| `enable_unit_files` (multiple) | iterate; SCM has no batch API |

Edge cases:
- All `OpenSCManagerW` calls use `SC_MANAGER_CONNECT` only (no `CREATE_SERVICE` rights — service install is via `service.install_managed` which uses `CreateServiceW` with elevated rights).
- Polling timeout: 30s default, `wait_hint_ms` from `SERVICE_STATUS_PROCESS` is the recommended overshoot. `HelperError::Systemd { message }` carries the timeout text.
- Win32 errors map: `ERROR_SERVICE_DOES_NOT_EXIST` → `UnitState::NotFound`; everything else → `HelperError::Systemd { message: format!("scm: {e}") }`.

### §3.3 Service install (`service.install_managed` on Windows)

`service.install_managed` on Windows:
1. `OpenSCManagerW(SC_MANAGER_CREATE_SERVICE)` (requires SCM-write privilege; `LocalSystem` has it).
2. `CreateServiceW(target_service, display_name, SERVICE_ALL_ACCESS, SERVICE_WIN32_OWN_PROCESS, SERVICE_DEMAND_START, SERVICE_ERROR_NORMAL, binPath, ...)`.
3. `ChangeServiceConfig2W(SERVICE_CONFIG_FAILURE_ACTIONS, ...)` to configure restart-on-fail equivalent to `Restart=on-failure` + `RestartSec=2s` from the Linux unit file.
4. `ChangeServiceConfig2W(SERVICE_CONFIG_DESCRIPTION, ...)` to set the human description.
5. The "binPath" = `boxpilotd.exe` install location + flags identifying which sing-box core to launch (resolved from `cores/current` junction at service start time).

The Linux-side `service::unit::render_*` template is replaced by a Windows-side `service::scm_config::build` builder that produces the equivalent SCM config struct. Shared GUI-visible behaviour (auto-restart, description) is preserved.

### §3.4 Service uninstall

Sub-project #2 does **not** add a `service.uninstall` verb (that's an installer concern, Sub-project #3). `service.install_managed` on a re-install path uses `DeleteService` + recreate when the existing service is misconfigured (mirrors current Linux idempotent-reinstall behavior).

---

## §4 Windows Authority Implementation

### §4.1 New impl: `WindowsLocalAuthority`

```rust
// crates/boxpilot-platform/src/windows/authority.rs
pub struct WindowsLocalAuthority;

impl Authority for WindowsLocalAuthority {
    async fn check(&self, action_id: &str, principal: &CallerPrincipal) -> HelperResult<bool> {
        let sid = match principal {
            CallerPrincipal::WindowsSid(s) => s.clone(),
            CallerPrincipal::LinuxUid(_) => return Ok(false),  // defensive; should never happen on Windows
        };
        let auth_class = HelperMethod::from_polkit_action_id(action_id).map(|m| m.auth_class());
        match auth_class {
            Some(AuthClass::ReadOnly) => Ok(true),
            Some(AuthClass::Mutating) => {
                let is_controller = ctx.is_controller_sid(&sid).await;
                let is_admin = check_administrators_membership(&sid)?;
                Ok(is_controller || is_admin)
            }
            Some(AuthClass::HighRisk) => check_administrators_membership(&sid),
            None => Ok(false),  // unknown action = deny
        }
    }
}
```

### §4.2 Helpers

- `check_administrators_membership(sid: &str) -> HelperResult<bool>`:
  1. Convert SID string → SID bytes via `ConvertStringSidToSidW`.
  2. Build the well-known Administrators SID `S-1-5-32-544` via `CreateWellKnownSid(WinBuiltinAdministratorsSid)`.
  3. `CheckTokenMembership(NULL, admin_sid, &mut result)` against the impersonation token of the caller. Note: `CheckTokenMembership` with `TokenHandle = NULL` checks the calling thread's effective token; we need to impersonate the caller first via `ImpersonateNamedPipeClient` before the call, then `RevertToSelf`.
  4. Free both SID buffers via `LocalFree`.
- `ctx.is_controller_sid(sid)`: read `boxpilot.toml::controller_principal`, parse tag-form, compare `windows:` prefix + SID equality.

### §4.3 Caller principal lookup (existing, no change)

Sub-project #1 already wired `GetNamedPipeClientProcessId` → `OpenProcessToken` → `GetTokenInformation(TokenUser)` → `ConvertSidToStringSidW`. This produces `CallerPrincipal::WindowsSid(String)`. Sub-project #2 reuses it as-is.

The acknowledged-leak `LocalFree` of the `ConvertSidToStringSidW` result (per round-1 review on PR #23, marked "fine for Sub-project #1") is **fixed in this sub-project** — small change, same hot path, addresses the long-running-helper memory growth concern.

---

## §5 GUI Text Platform-awareness

### §5.1 New verb: `platform.info`

```rust
// boxpilot-ipc/src/method.rs
HelperMethod::PlatformInfo => "platform.info"
auth_class = ReadOnly
```

Response:
```rust
pub struct PlatformInfoResponse {
    pub os: String,           // "linux" | "windows"
    pub os_version: String,   // free-form, e.g. "Windows 11 Pro 23H2 (build 22631)" or "Ubuntu 22.04.4 LTS"
}
```

Helper implementation: `cfg!(target_os = ...)` for `os`; OS-specific call for `os_version` (`/etc/os-release` parse on Linux, `RtlGetVersion` on Windows). 100% read-only and cheap.

### §5.2 GUI consumption

Vue side calls `platform.info` once at app boot, caches `platform.os` to a Pinia store. i18n key resolution:

```
zh.service.start_button → "启动服务"          (no platform variant; same on both)
zh.service.stop.linux   → "通过 systemd 停止"
zh.service.stop.windows → "通过 Windows 服务管理器停止"
zh.legacy.migrate.linux → "..."
```

Lookup: `t('service.stop')` first tries `t('service.stop.${platform}')`, falls back to `t('service.stop')`. Implemented as a small wrapper around `vue-i18n`. Most keys do not need platform variants; the audit will identify the few that do (likely 5-10 keys mentioning systemd / polkit / journalctl / dbus).

### §5.3 Help URLs

GUI "Help" buttons that today link to systemd man pages get a `platform`-aware URL chooser. Default to a placeholder MS Learn URL on Windows; a follow-up content task can refine.

---

## §6 `build_tempfile_aux` (Windows AuxStream backing)

```rust
// crates/boxpilot-platform/src/windows/bundle.rs
pub async fn build_tempfile_aux(staging_path: &Path) -> HelperResult<AuxStream> {
    use tempfile::Builder;
    let aux_dir = paths.local_appdata().join("aux");
    tokio::fs::create_dir_all(&aux_dir).await?;
    // OWNER-only ACL is the default on %LocalAppData% subdirs; no extra step needed,
    // but we cross-check via FsPermissions::ensure_owner_only(&aux_dir) defensively.
    let mut tmp = Builder::new()
        .prefix("boxpilot-aux-")
        .suffix(".tar")
        .tempfile_in(&aux_dir)
        .map_err(|e| HelperError::Ipc { message: format!("tempfile: {e}") })?;
    // tar the staging_path contents into tmp (tokio::task::spawn_blocking)
    spawn_blocking(move || {
        let mut builder = tar::Builder::new(tmp.as_file_mut());
        builder.append_dir_all(".", staging_path)?;
        builder.finish()?;
        tmp.as_file_mut().sync_all()?;
        Ok::<_, std::io::Error>(tmp)
    }).await??;
    let mut file: tokio::fs::File = tmp.reopen().map_err(...)?.into();
    file.seek(SeekFrom::Start(0)).await?;
    // The NamedTempFile is dropped when this fn returns, which deletes the file.
    // The reopened tokio::fs::File still has a handle; on Windows the file stays
    // until the last handle closes. AuxStream::AsyncRead owns the handle.
    Ok(AuxStream::from_async_read(file))
}
```

**Integrity:** No F_SEAL_* equivalent. The bundle's sha256 is computed by the Linux profile-prepare path and carried in the `ActivateBundleRequest`; Windows preparation does the same. The receiver re-hashes on unpack (existing logic). This makes seal-vs-no-seal observable only via "did the file get tampered between prepare and dispatch in the same process?" — a non-threat on a single-process flow.

**Cleanup:** Orphan tempfiles from crashed runs accumulate in `%LocalAppData%\BoxPilot\aux\`. Helper startup runs `aux::cleanup_orphans()`: any file older than 1 hour gets unlinked. This is enough; the aux dir is not size-bounded otherwise.

---

## §7 ActivePointer (Windows) — Junction-based

Linux uses `symlink + rename(2)` for the `releases/active` pointer. Windows symbolic links require either Developer Mode or admin privilege (which the helper has, but the GUI debugger paths may not). Junctions (`fsutil reparsepoint` / `CreateSymbolicLinkW(SYMBOLIC_LINK_FLAG_DIRECTORY)` won't work for a non-admin GUI; junctions don't have this restriction).

**Implementation:**

```rust
// crates/boxpilot-platform/src/windows/active.rs
use windows_sys::Win32::Storage::FileSystem::*;

impl ActivePointer for JunctionActive {
    fn read(&self) -> HelperResult<Option<PathBuf>> {
        // Read junction target via FSCTL_GET_REPARSE_POINT
    }
    fn set_atomic(&self, target: &Path) -> HelperResult<()> {
        // 1. Create junction at <pointer>.new -> target via DeviceIoControl(FSCTL_SET_REPARSE_POINT, ...)
        // 2. MoveFileExW(<pointer>.new, <pointer>, MOVEFILE_REPLACE_EXISTING) for atomic swap.
    }
    fn clear(&self) -> HelperResult<()> {
        // RemoveDirectoryW (junctions are directories)
    }
}
```

**Why junction over symlink:** SeCreateSymbolicLinkPrivilege is required for symlinks but not junctions. Junctions are limited to directories on the same volume — fine for our use case (releases all under `%ProgramData%\BoxPilot\releases\`). The "fragment" of a junction is opaque to the user; that's acceptable.

---

## §8 Components Affected (B — full-workspace Windows compile)

`boxpilotd` currently has unix-only modules blocking `cargo check --target x86_64-pc-windows-msvc -p boxpilotd`. The fix is per-file `#[cfg(target_os = "linux")]` gating where the file is Linux-only by design, and refactor to platform-neutral abstractions where the file should work cross-platform.

### §8.1 Files requiring `#[cfg(target_os = "linux")]` (Linux-only by design)

- `crates/boxpilotd/src/iface.rs` — entire D-Bus zbus interface. Windows uses `entry/windows.rs` driving `NamedPipeIpcServer` directly; there is no zbus equivalent. Gate the entire module.
- `crates/boxpilotd/src/credentials.rs` — zbus-based `CallerResolver`. Already implicit in §1 (Sub-project #1 PR #23) but the file currently has no cfg attribute. Add `#![cfg(target_os = "linux")]`.

### §8.2 Files requiring refactor (should be cross-platform)

- `crates/boxpilotd/src/legacy/backup.rs` — uses `std::os::unix::fs::PermissionsExt`. Refactor: replace `set_permissions(0o600)` with `boxpilot_platform::FsPermissions::ensure_owner_only()` (already in trait). Tests cfg-gate to Linux for the existing assertions; add Windows-specific tests using `RecordingFsPermissions`.
- `crates/boxpilotd/src/profile/release.rs` — uses `std::os::unix::fs::symlink` for the `active` pointer. Refactor: replace direct symlink calls with `ActivePointer::set_atomic()` (Sub-project #1 already did the trait extraction — verify all callers route through the trait, not raw `symlink`).
- `crates/boxpilotd/src/profile/{recovery, rollback, activate}.rs` and `core/commit.rs` — symlink usage in the **production path** (not just tests). Audit each call:
  - If the symlink is an `ActivePointer` operation: route through the trait.
  - If it's a different concept (e.g. a backup snapshot symlink): introduce a small ad-hoc trait or cfg-gate the operation per platform.
- `crates/boxpilotd/src/core/trust.rs` — Linux-specific trust checker (uid + mode bits + parent-dir walk + setuid check). Already extracted to `TrustChecker` trait in Sub-project #1. Verify the boxpilotd file wraps the trait and isn't doing direct unix calls.

### §8.3 Test code

Tests using `std::os::unix::fs::symlink` for fixture setup are cfg-gated to Linux. Windows tests use `JunctionActive` or in-memory fakes.

### §8.4 CI gate widening

`.github/workflows/windows-check.yml`:

```yaml
# was:  cargo check -p boxpilot-ipc -p boxpilot-platform
# now:  cargo check --workspace --all-targets
```

Plus `cargo test --workspace --all-targets` once all crates compile and Windows-specific tests are wired up. The Windows CI test-run reaches parity with the Linux runner over the course of batch ① → ② → ③.

---

## §9 Real `AclFsPermissions` (C)

Sub-project #1 ships a no-op stub (`tracing::debug! + Ok(())`). Sub-project #2 implements the real ACL story.

### §9.1 Required behavior

Match the Linux `chmod 0700` / `0600` semantics: only the file owner can read/write. Group and Everyone get nothing.

### §9.2 Win32 implementation

```rust
fn ensure_owner_only(&self, path: &Path) -> HelperResult<()> {
    // 1. Get the current process's user SID via OpenProcessToken + GetTokenInformation(TokenUser).
    // 2. Build EXPLICIT_ACCESS_W with one ACE granting GENERIC_ALL to the SID.
    // 3. SetEntriesInAclW(1, &ace, NULL, &mut new_dacl).  // null old_dacl = empty starting point
    // 4. SetNamedSecurityInfoW(path, SE_FILE_OBJECT,
    //      DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
    //      NULL, NULL, new_dacl, NULL)
    //    PROTECTED_DACL prevents inheritance from parent (which on %ProgramData% may grant Everyone read).
    // 5. LocalFree(new_dacl).
    Ok(())
}
```

The `PROTECTED_DACL_SECURITY_INFORMATION` flag is critical: without it, inherited ACEs from `%ProgramData%` (which normally grant Users:read) leak through and the file is not actually owner-only.

### §9.3 LocalFree pattern

The fixed-from-#1 `LocalFree`-after-`ConvertSidToStringSidW` pattern in `windows/ipc.rs` carries over here: every `SetEntriesInAclW` and every owner-SID `GetTokenInformation` allocation is paired with `LocalFree` (or `HeapFree`) on the same code path including error paths. Helper macro to avoid leaks:

```rust
struct LocalFreeOnDrop<T>(*mut T);
impl<T> Drop for LocalFreeOnDrop<T> {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { LocalFree(self.0 as _); }
        }
    }
}
```

---

## §10 Dependencies & Batching

```
B (compile gate) ────────────► all other work
  ├── D (SCM trait + UnitState v2) ──► A.service.* + service.install_managed
  ├── E (schema v2 + migration) ─────► F (Authority real)
  │                                      └── A 全部 (mutating verbs need real auth)
  │                                      └── C (AclFsPermissions real)
  ├── H (build_tempfile_aux) ────────► profile.activate_bundle subset of A
  └── G (GUI text) ──────────────────► independent; can land any time
```

### §10.1 Batch organization

Four PRs. Each batch leaves `main` in a state where Linux is fully working and Windows has progressively more verbs functioning correctly. Following the Sub-project #1 retrospective: 7 batches was too review-frequent; 1 was too large; 4 is the target.

| Batch | Scope | Size | Leaves main with… |
|---|---|---|---|
| ① | **Foundation**: D (SCM trait — extend `UnitState` with `platform_extra`) + B partial (cfg-gate `iface.rs` / `credentials.rs`; refactor symlink call sites that match the ActivePointer pattern to use the trait, cfg-gate or per-platform-rewrite the rest per §8.2; refactor `backup.rs` to use `FsPermissions::ensure_owner_only`). | M | Linux: bit-for-bit. Windows: full-workspace `cargo check` green (still NotImplemented for verbs). |
| ② | **Authorization platform**: E (schema v2 + auto-migration in `BoxpilotConfig::parse`) + F (`WindowsLocalAuthority` + helpers + `LocalFree` cleanup of #1's leak) + C (`AclFsPermissions` real impl). | M | Linux: bit-for-bit. Windows: real authorization story; ProfileStore files protected; controller-claim writes the new `controller_principal` field. |
| ③ | **Real Windows verbs**: ServiceManager Windows impl + `JunctionActive` ActivePointer + `WindowsCoreAssetNaming` + `ZipExtractor` + H (`build_tempfile_aux`). Wires up service.{install_managed,start,stop,restart,enable,disable,status,logs}, core.{install_managed,upgrade_managed,rollback_managed,adopt,discover}, profile.{activate_bundle,rollback_release} on Windows. | L | Windows: every primary-flow verb works end-to-end. |
| ④ | **Edge verbs + polish**: legacy.{migrate_service,observe_service} + diagnostics.export_redacted + controller.transfer + home.status + G (platform.info verb + Vue i18n platform-variant wrapper). | M | Windows: all 21 verbs functional; GUI strings aware of platform context. |

Per-batch workflow (mirrors Sub-project #1):
1. Implement on `feat/sub-project-2-windows-port` branch as a stack of fix-style commits per Sub-project #1 retrospective (no per-batch sub-branches; one PR per batch is opened from the same branch and merged forward).
2. Push, open one GH PR, wait for human review/merge.
3. `git pull --rebase origin main` before starting the next batch.

---

## §11 Testing Strategy

### §11.1 Unit tests

- **Schema migration**: v1 → v2 round-trip, v1 with no `controller_uid` (Unset state preserved), v2 with malformed `controller_principal` tag, schema_version > 2 (existing UnsupportedSchemaVersion path).
- **WindowsLocalAuthority**: matrix of (auth_class, caller is controller, caller is admin) → expected verdict. Uses a `MockTokenChecker` injected into the `WindowsLocalAuthority` (the real Win32 calls live behind a thin trait so tests aren't gated to actual Windows tokens).
- **AclFsPermissions**: smoke test on Windows runner — create a file, call `ensure_owner_only`, assert the returned DACL has exactly one ACE for the current user SID.
- **JunctionActive**: create, read-back, atomic swap, clear. All under `%TEMP%`.
- **Tempfile aux**: write a known byte stream, reopen, sha256 must match. Verify orphan cleanup deletes 1+ hour-old files in a fixture dir.
- **SCM ServiceManager**: integration tests gated to `windows-latest` runner — create a dummy service via `sc create` in test setup, start it, query state, stop it, delete it. Each subtest is hermetic (creates and removes its own service).

### §11.2 Cross-platform test runs

`cargo test --workspace --all-targets` runs on both `ubuntu-latest` and `windows-latest`. Tests using platform-specific syscalls are cfg-gated; cross-platform logic is exercised on both.

### §11.3 GUI

Vue unit test verifying the platform-variant i18n key resolution wrapper falls back to the platform-less key when no variant exists, and picks the right variant when both exist. Run under `npm test` on both runners.

### §11.4 Manual validation (smoke procedures)

A new `docs/superpowers/plans/<date>-windows-port-sub-project-2-smoke-procedure.md` document at the end of batch ④ describes a 30-minute manual smoke on a fresh Windows VM:
- Install boxpilotd.exe via `sc create` (developer flow; installer is Sub-project #3).
- Launch GUI, verify "Welcome" pane, install managed core, activate a profile, start service, check log pane, simulate failure (kill process), verify restart-on-fail.
- Run all four `legacy.migrate_service` paths against a dummy "old" service.
- Switch user accounts, verify Authority gates correctly (ReadOnly OK from any user, Mutating refused for non-controller non-admin).

---

## §12 Risks & Open Questions

These are flagged here so the implementation plan can pick them up rather than blocking the spec.

1. **SCM start_unit polling timeout** — 30s default may not suffice for sing-box with large config loads. Plan provides a knob in `BoxpilotConfig` (`scm_start_timeout_secs: Option<u64>`, default 30). To verify with realistic configs.
2. **`ChangeServiceConfig2W(SERVICE_CONFIG_FAILURE_ACTIONS)` granularity** — Linux `Restart=on-failure` + `RestartSec=2s` is hard to map exactly. SCM offers `SC_ACTION_RESTART` with delay-millis, no exit-code filtering. Plan: configure `SC_ACTION_RESTART(2000)` for first 3 actions then `SC_ACTION_NONE`. Document the divergence.
3. **`os_release` parsing on rare Linux distros** — `platform.info` falls back to `"linux unknown"` if `/etc/os-release` is missing or malformed; not a blocker.
4. **Vue i18n key audit** — exact list of strings needing platform variants is determined by grep at the start of batch ④. Estimate: 5–10 keys. If the real number is much higher, batch ④ may need split.
5. **GUI Help URL content** — the Windows Help URLs initially point to placeholders (MS Learn or empty). Real content authoring is out of scope.
6. **`controller.transfer` semantics on Windows** — the verb hands controller ownership from one principal to another. On Linux the new principal is a `LinuxUid`; on Windows it's a SID. The spec already supports this via `CallerPrincipal`. The interactive flow (how does the new controller "claim" — by being the one to call transfer? does the old controller need to authenticate?) inherits Linux behavior. Verify in batch ④.

---

## §13 Acceptance Criteria

- AC1: `cargo check --target x86_64-pc-windows-msvc --workspace --all-targets` is green on `windows-latest`.
- AC2: `cargo test --workspace --all-targets` is green on both `ubuntu-latest` and `windows-latest` runners.
- AC3: `cargo test --workspace --all-targets` count on Linux is no less than the post-Sub-project-#1 count (397) — i.e., no regressions.
- AC4: `boxpilotctl service.status` succeeds on a Windows host running `boxpilotd.exe` as a service, and returns a `UnitState::Known` with `platform_extra: Windows { … }`.
- AC5: `boxpilotctl profile.activate_bundle` end-to-end (helper-side bundle prepare not required; reuse existing flow with Windows `build_tempfile_aux`) succeeds on Windows for a 10MB sample bundle.
- AC6: `boxpilotctl service.start` from a non-admin, non-controller account is **denied** with `HelperError::NotAuthorized`. From the same account after first-time controller-claim through the GUI, it succeeds.
- AC7: A Linux box upgraded from Sub-project-#1 daemon to Sub-project-#2 daemon (with `boxpilot.toml::schema_version = 1` on disk) reads correctly, performs a mutating call, and writes the file back as `schema_version = 2` with `controller_principal = "linux:<uid>"`.
- AC8: Vue i18n platform-variant resolution: a key that exists only as `service.stop.linux` falls back when the Vue runtime is in Windows mode (logs warning, displays the key name); a key that exists as both `service.stop.linux` and `service.stop.windows` displays the right variant per `platform.info` response.
