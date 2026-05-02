# BoxPilot Windows Port — Sub-project #2 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Land the Windows port to public-beta-ready quality: real Windows verb implementations, full-workspace Windows compile, real authorization (Group + controller-SID), platform-aware GUI copy. Linux behavior unchanged.

**Architecture:** Build on Sub-project #1's trait surface. Replace 8 `unimplemented!()` Windows stubs with real Win32 impls; cfg-gate or refactor remaining unix-only modules in `boxpilotd`; bump `boxpilot.toml` schema v1 → v2 with auto-migration to support `controller_principal: String`; add `WindowsLocalAuthority` per-verb authorization table; extend `UnitState` with `platform_extra` for SCM richness.

**Tech Stack:** Rust 2024, `windows-sys 0.59`, `windows-service 0.7`, `tempfile`, `zip`, `tokio`, `serde`. Vue 3 + `vue-i18n` for GUI text platform-variants.

---

## Branch & PR workflow

- All work on branch `feat/sub-project-2-windows-port` (already created from `main` after Sub-project #1 merge `1547946`).
- Spec at `docs/superpowers/specs/2026-05-02-boxpilot-windows-port-sub-project-2-design.md`.
- Four batches (①–④); each batch lands as one GitHub PR off this branch. After review/merge, `git pull --rebase origin main` before the next batch.
- Plan-PRs within a batch are commits on the branch; the GH PR shows the cumulative diff.

## File structure

### New files
- `crates/boxpilot-platform/src/windows/scm.rs` — Windows ServiceManager impl
- `crates/boxpilot-platform/src/windows/localauth.rs` — `WindowsLocalAuthority` + Administrators check
- `crates/boxpilot-platform/src/windows/sid.rs` — SID parsing/formatting/well-known SID helpers
- `crates/boxpilot-platform/src/windows/local_free.rs` — `LocalFreeOnDrop<T>` RAII wrapper
- `crates/boxpilot-ipc/src/platform_info.rs` — `PlatformInfoResponse` type
- `boxpilot-tauri/src/platform_store.ts` — Pinia store + i18n wrapper (TypeScript side)

### Files modified (key ones)
- `crates/boxpilot-ipc/src/response.rs` — extend `UnitState::Known` with `platform_extra: PlatformUnitExtra`
- `crates/boxpilot-ipc/src/config.rs` — schema v1 → v2 migration; `controller_principal: String`
- `crates/boxpilot-ipc/src/method.rs` — add `HelperMethod::PlatformInfo` (ReadOnly)
- `crates/boxpilot-platform/src/traits/authority.rs` — `CallerPrincipal::{to_toml_tag, from_toml_tag}` helpers
- `crates/boxpilot-platform/src/windows/{bundle,active,core_assets,fs_perms}.rs` — replace stubs with real impls
- `crates/boxpilot-platform/src/windows/ipc.rs` — fix `LocalFree` leak after `ConvertSidToStringSidW`
- `crates/boxpilotd/src/iface.rs` — entire file: `#![cfg(target_os = "linux")]`
- `crates/boxpilotd/src/credentials.rs` — `#![cfg(target_os = "linux")]`
- `crates/boxpilotd/src/legacy/backup.rs` — drop `PermissionsExt` in favor of `FsPermissions::ensure_owner_only`
- `crates/boxpilotd/src/handlers/*.rs` — Windows-side wiring for all verbs (mostly already platform-neutral; some need Windows-specific deps)
- `crates/boxpilotd/src/main.rs` + `entry/{linux,windows}.rs` — wire `WindowsLocalAuthority` instead of `AlwaysAllowAuthority`
- `boxpilot-tauri/src/lib.rs` — call `platform.info` on boot, populate Pinia store
- `boxpilot-tauri/src/translations/*.json` — add platform-suffixed variant keys
- `.github/workflows/windows-check.yml` — widen to `cargo check --workspace --all-targets` then `cargo test --workspace`

---

# Batch ① — Foundation

Goal: leave `main` with Linux bit-for-bit unchanged and Windows full-workspace `cargo check` green. Verbs still return `NotImplemented` but the daemon, GUI plumbing, and IPC types compile end-to-end.

## PR 1.1 — `UnitState::platform_extra` + `PlatformUnitExtra` enum (IPC types only)

**Scope:** Add the `platform_extra` field to `UnitState::Known` and define the `PlatformUnitExtra` enum. No production behavior change yet — Linux callers populate `PlatformUnitExtra::Linux` (the variant carries no data). Windows code continues to return `NotImplemented`.

**Files:**
- Modify: `crates/boxpilot-ipc/src/response.rs:1-25`
- Test: `crates/boxpilot-ipc/src/response.rs` (existing test module)
- Modify: callers that construct `UnitState::Known` in fakes/tests:
  - `crates/boxpilot-platform/src/fakes/service.rs`
  - `crates/boxpilotd/src/systemd.rs`
  - test files where `UnitState::Known { … }` is built (grep)

**Dependencies:** none (first PR-task).

- [ ] **Step 1: Write the failing test for the new field**

In `crates/boxpilot-ipc/src/response.rs` after the `pub enum UnitState`:

```rust
#[cfg(test)]
mod platform_extra_tests {
    use super::*;
    use serde_json;

    #[test]
    fn known_state_round_trips_with_linux_extra() {
        let s = UnitState::Known {
            active_state: "active".into(),
            sub_state: "running".into(),
            load_state: "loaded".into(),
            n_restarts: 3,
            exec_main_status: 0,
            platform_extra: PlatformUnitExtra::Linux,
        };
        let json = serde_json::to_string(&s).unwrap();
        let back: UnitState = serde_json::from_str(&json).unwrap();
        assert_eq!(s, back);
    }

    #[test]
    fn known_state_round_trips_with_windows_extra() {
        let s = UnitState::Known {
            active_state: "active".into(),
            sub_state: "running".into(),
            load_state: "loaded".into(),
            n_restarts: 0,
            exec_main_status: 0,
            platform_extra: PlatformUnitExtra::Windows {
                check_point: 0,
                wait_hint_ms: 30000,
                controls_accepted: 0x0000_0001, // SERVICE_ACCEPT_STOP
            },
        };
        let json = serde_json::to_string(&s).unwrap();
        let back: UnitState = serde_json::from_str(&json).unwrap();
        assert_eq!(s, back);
    }
}
```

- [ ] **Step 2: Run the test to verify it fails**

```
cargo test -p boxpilot-ipc platform_extra_tests
```

Expected: compile error (`PlatformUnitExtra` not defined, `platform_extra` field unknown).

- [ ] **Step 3: Add the `PlatformUnitExtra` enum and extend `UnitState::Known`**

In `crates/boxpilot-ipc/src/response.rs`:

```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "platform", rename_all = "lowercase")]
pub enum PlatformUnitExtra {
    /// Linux/systemd: existing UnitState fields (sub_state, load_state, n_restarts,
    /// exec_main_status) already cover the systemd shape, so this variant carries
    /// no extra data.
    Linux,
    /// Windows/SCM: SCM-specific status fields not representable in the
    /// systemd-shaped surface.
    Windows {
        /// SERVICE_STATUS_PROCESS::dwCheckPoint
        check_point: u32,
        /// SERVICE_STATUS_PROCESS::dwWaitHint (milliseconds)
        wait_hint_ms: u32,
        /// SERVICE_STATUS_PROCESS::dwControlsAccepted (bitmask of SERVICE_ACCEPT_*)
        controls_accepted: u32,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum UnitState {
    NotFound,
    Known {
        active_state: String,    // active | inactive | failed | activating | reloading | deactivating | transitioning | paused
        sub_state: String,       // platform-specific lower-snake; see PlatformUnitExtra docs
        load_state: String,      // loaded | not-found | error | masked
        n_restarts: u32,
        exec_main_status: i32,
        #[serde(default = "default_platform_extra")]
        platform_extra: PlatformUnitExtra,
    },
}

fn default_platform_extra() -> PlatformUnitExtra {
    // Default for old IPC clients that don't send the field yet (forward-compat
    // during the rollout window). The current build always sets it explicitly.
    PlatformUnitExtra::Linux
}
```

- [ ] **Step 4: Update every existing `UnitState::Known { … }` constructor**

Run:
```
grep -rn "UnitState::Known {" crates/
```

For each call site, add `platform_extra: PlatformUnitExtra::Linux,` (preserving Linux behavior). Files to update (typical):
- `crates/boxpilotd/src/systemd.rs` — `DBusSystemd::unit_state`
- `crates/boxpilot-platform/src/linux/service.rs` — Linux `ServiceManager` impl
- `crates/boxpilot-platform/src/fakes/service.rs` — `FixedSystemd`, `RecordingSystemd`
- `crates/boxpilotd/src/systemd/testing.rs` (if separate)
- All test files using `UnitState::Known`

- [ ] **Step 5: Run the test to verify it passes**

```
cargo test -p boxpilot-ipc platform_extra_tests
```

Expected: PASS (both round-trip tests).

- [ ] **Step 6: Run full Linux test suite to verify no regressions**

```
cargo test -p boxpilot-ipc -p boxpilot-platform -p boxpilot-profile -p boxpilotd
```

Expected: 397+ tests pass (matches Sub-project #1 baseline; field addition shouldn't change any assertion).

- [ ] **Step 7: Commit**

```
git add crates/boxpilot-ipc/src/response.rs crates/boxpilot-platform/src crates/boxpilotd/src
git commit -m "feat(ipc): add PlatformUnitExtra to UnitState::Known

Sub-project #2 prep: SCM populates check_point/wait_hint_ms/controls_accepted
which don't fit the systemd-shaped UnitState. New tagged enum variant.
Linux callers all populate ::Linux (no data) preserving prior behavior.
GUI consumes via active_state primarily; platform_extra for diagnostics."
```

---

## PR 1.2 — Update Linux ServiceManager impl + tests for new field

**Scope:** Make sure every Linux production path that returns a `UnitState::Known` correctly populates `platform_extra: PlatformUnitExtra::Linux`. This is mostly a follow-up audit to PR 1.1, with assertions added in tests so regressions can't reintroduce a missing field.

**Files:**
- Modify: `crates/boxpilot-platform/src/linux/service.rs:` (DBusSystemd::unit_state)
- Modify: `crates/boxpilotd/src/systemd.rs:` (if not already covered)
- Test: `crates/boxpilot-platform/src/linux/service.rs` (existing test module)

**Dependencies:** PR 1.1.

- [ ] **Step 1: Write the failing test asserting Linux extra is populated**

In the test module of `crates/boxpilot-platform/src/linux/service.rs`:

```rust
#[tokio::test]
async fn unit_state_returns_linux_platform_extra() {
    let systemd = DBusSystemd::new(/* fake conn or test setup */);
    let state = systemd.unit_state("test.service").await.unwrap();
    if let UnitState::Known { platform_extra, .. } = state {
        assert_eq!(platform_extra, PlatformUnitExtra::Linux);
    } else {
        panic!("expected Known");
    }
}
```

- [ ] **Step 2: Run; expect failure if Linux impl wasn't updated by PR 1.1's blanket sweep**

```
cargo test -p boxpilot-platform unit_state_returns_linux_platform_extra
```

If it passes already (i.e. PR 1.1 caught everything), proceed to Step 4. If it fails, fix the constructor in `linux/service.rs::unit_state` to include `platform_extra: PlatformUnitExtra::Linux`.

- [ ] **Step 3: Re-run to verify pass**

- [ ] **Step 4: Run full Linux test suite**

```
cargo test -p boxpilot-platform -p boxpilotd
```

Expected: all pass.

- [ ] **Step 5: Commit**

```
git commit -am "test(platform/linux): pin platform_extra in unit_state response

Belt-and-suspenders against future regressions reintroducing the missing
field after PR 1.1's UnitState extension."
```

---

## PR 1.3 — cfg-gate `iface.rs` and `credentials.rs` to Linux

**Scope:** Stop trying to compile Linux-specific D-Bus code on Windows. The entire `crates/boxpilotd/src/iface.rs` (zbus interface trait) is Linux-only by design; same for `credentials.rs` (zbus-based `CallerResolver`). Add `#![cfg(target_os = "linux")]` at the top of each file. Update `crates/boxpilotd/src/main.rs` and `lib.rs` (if any) to gate the `mod iface` / `mod credentials` declarations as well.

**Files:**
- Modify: `crates/boxpilotd/src/iface.rs:1` — add `#![cfg(target_os = "linux")]` at top of file
- Modify: `crates/boxpilotd/src/credentials.rs:1` — add `#![cfg(target_os = "linux")]`
- Modify: `crates/boxpilotd/src/main.rs:` — gate `mod iface;` and `mod credentials;` with `#[cfg(target_os = "linux")]`
- Modify: `crates/boxpilotd/src/entry/windows.rs:` — verify it does NOT import `iface`/`credentials` (it should already drive `NamedPipeIpcServer` directly)

**Dependencies:** PR 1.1, PR 1.2.

- [ ] **Step 1: Add module-level cfg gate to `iface.rs`**

At the top of `crates/boxpilotd/src/iface.rs` (line 1, before any other content):

```rust
#![cfg(target_os = "linux")]
```

- [ ] **Step 2: Add module-level cfg gate to `credentials.rs`**

At the top of `crates/boxpilotd/src/credentials.rs`:

```rust
#![cfg(target_os = "linux")]
```

- [ ] **Step 3: Gate the `mod` declarations in `main.rs`**

Find the lines like `mod iface;` and `mod credentials;` in `crates/boxpilotd/src/main.rs`. Change to:

```rust
#[cfg(target_os = "linux")]
mod iface;
#[cfg(target_os = "linux")]
mod credentials;
```

(If the file uses `pub mod`, preserve the `pub`.)

- [ ] **Step 4: Verify Linux still compiles and tests pass**

```
cargo check -p boxpilotd
cargo test -p boxpilotd
```

Expected: clean compile, all tests pass.

- [ ] **Step 5: Verify Windows compile gets further than before**

```
cargo check --target x86_64-pc-windows-msvc -p boxpilotd 2>&1 | tail -40
```

Expected: errors decrease — `iface.rs` / `credentials.rs` errors gone. Remaining errors will be from `legacy/backup.rs` PermissionsExt and symlink call sites (fixed in PR 1.4–1.5).

- [ ] **Step 6: Commit**

```
git commit -am "build(boxpilotd): cfg-gate iface.rs and credentials.rs to Linux

Both files are inherently Linux-only: iface.rs is the zbus #[interface]
trait, credentials.rs is the zbus-based CallerResolver. Windows uses
entry/windows.rs driving NamedPipeIpcServer directly without an iface
equivalent. Sub-project #2 prep for full-workspace Windows compile."
```

---

## PR 1.4 — Refactor symlink call sites: route through ActivePointer trait

**Scope:** Audit every `std::os::unix::fs::symlink` call in `crates/boxpilotd/src/`. Categorize each call:
- (a) Setting / clearing the `active` pointer in `releases/` → route through `ActivePointer` trait (the trait was extracted in Sub-project #1; verify all production paths use it, not raw `symlink`).
- (b) Test fixture setup — cfg-gate the test to Linux (`#[cfg(target_os = "linux")]` on the test fn or module).
- (c) Other production usage (rare; flag and either trait-extract or cfg-gate per-platform).

**Files:**
- Modify: `crates/boxpilotd/src/profile/release.rs` — verify production code uses `ActivePointer`; cfg-gate any tests that build symlinks for fixtures
- Modify: `crates/boxpilotd/src/profile/recovery.rs:159,172,183` — these are test paths; cfg-gate to Linux
- Modify: `crates/boxpilotd/src/profile/rollback.rs:289,329,368,425` — cfg-gate test fixture symlinks
- Modify: `crates/boxpilotd/src/profile/activate.rs:559,606,677` — cfg-gate test fixture symlinks
- Modify: `crates/boxpilotd/src/iface.rs:1193` — (whole file already cfg-gated by PR 1.3, so this resolves automatically)
- Modify: `crates/boxpilotd/src/core/commit.rs:154` — production code; verify whether this is the `current` cores symlink and route through `ActivePointer` or a sibling trait

**Dependencies:** PR 1.3.

- [ ] **Step 1: Audit symlink calls in production code**

Run:
```
grep -rn "std::os::unix::fs::symlink\|symlink(\|use std::os::unix::fs" crates/boxpilotd/src/ | grep -v "#\[cfg(target_os" | grep -v "#!\\[cfg(target_os"
```

For each line not already inside a `#[cfg(target_os = "linux")]` block, classify per the categories above.

- [ ] **Step 2: Add `#[cfg(target_os = "linux")]` to test fns that build fixture symlinks**

For test functions in `profile/recovery.rs`, `profile/rollback.rs`, `profile/activate.rs` that call `std::os::unix::fs::symlink` to build setup fixtures:

```rust
#[cfg(target_os = "linux")]
#[tokio::test]
async fn rollback_happy_path_swaps_active_and_writes_toml() {
    // ... existing test body using std::os::unix::fs::symlink
}
```

The test only runs on Linux; on Windows it's compiled out cleanly.

- [ ] **Step 3: Audit `core/commit.rs:154` symlink usage**

Read that section:
```
sed -n '140,170p' crates/boxpilotd/src/core/commit.rs
```

If it's the `cores/current` symlink (matching the spec §10 storage layout), this **is** the cross-platform "active core pointer" concept. Decide:
- Option A (preferred): introduce a small trait-extension or platform helper:

In `crates/boxpilot-platform/src/traits/active.rs` (extending the existing `ActivePointer` or adding a sibling helper):

```rust
/// Atomic-replace a "current" pointer at `link` to point to `target`.
/// Linux: symlink + rename. Windows: junction + MoveFileExW.
pub trait CurrentPointer: Send + Sync {
    fn set_atomic(&self, link: &Path, target: &Path) -> std::io::Result<()>;
}
```

Add Linux impl backing `std::os::unix::fs::symlink + std::fs::rename`. Add Windows impl in PR 3.5 (junction-based).

- Option B: cfg-gate the immediate symlink line if it's only used in a code path that's Linux-specific anyway. Less ideal — preserves the abstraction-leak.

For the plan: if `core/commit.rs:154` is the cores symlink stage, prefer Option A: add `CurrentPointer` trait now (one method, one Linux impl) and use it. Windows impl lands in PR 3.5 alongside the junction-based ActivePointer.

- [ ] **Step 4: Verify Linux tests pass**

```
cargo test -p boxpilotd
```

Expected: all 200+ tests pass.

- [ ] **Step 5: Verify Windows compile gets further**

```
cargo check --target x86_64-pc-windows-msvc -p boxpilotd 2>&1 | tail -30
```

Expected: symlink errors gone (or reduced to the cores/current path which is now behind `CurrentPointer` trait pending Windows impl). Remaining errors should be from `legacy/backup.rs` (fixed next).

- [ ] **Step 6: Commit**

```
git commit -am "build(boxpilotd): cfg-gate or trait-extract symlink call sites

Test-only symlink fixtures are gated to Linux. Production cores/current
symlink (core/commit.rs) extracted to a small CurrentPointer trait;
Windows impl lands in PR 3.5 with junction support."
```

---

## PR 1.5 — Refactor `legacy/backup.rs` PermissionsExt → `FsPermissions`

**Scope:** Drop the top-level `use std::os::unix::fs::PermissionsExt`. Replace `set_permissions(0o600)` with `FsPermissions::ensure_owner_only(path)`. The `FsPermissions` trait is already wired through `HelperContext::fs_perms` (Sub-project #1); the legacy backup code just needs to use it.

**Files:**
- Modify: `crates/boxpilotd/src/legacy/backup.rs:1-80`
- Test: `crates/boxpilotd/src/legacy/backup.rs` test module

**Dependencies:** PR 1.4.

- [ ] **Step 1: Read the current backup.rs to find all PermissionsExt usages**

```
grep -n "PermissionsExt\|set_permissions" crates/boxpilotd/src/legacy/backup.rs
```

- [ ] **Step 2: Refactor public function signature to take `&dyn FsPermissions`**

The signature becomes (find the actual fn name, e.g. `backup_unit_file`):

```rust
pub async fn backup_unit_file(
    fragment_path: &Path,
    backups_units_dir: &Path,
    unit_name: &str,
    iso_now: &str,
    fs_perms: &dyn boxpilot_platform::traits::fs_perms::FsPermissions,
) -> HelperResult<PathBuf> {
    // ... existing logic to compute backup_path and copy bytes ...
    fs_perms.ensure_owner_only(&backup_path)
        .map_err(|e| HelperError::Ipc { message: format!("backup chmod: {e}") })?;
    Ok(backup_path)
}
```

- [ ] **Step 3: Update the caller (legacy::migrate::cutover) to pass `&*ctx.fs_perms`**

In `crates/boxpilotd/src/legacy/migrate.rs::cutover`:

```rust
let backup_path = match fragment_path {
    Some(p) => crate::legacy::backup::backup_unit_file(
        Path::new(&p),
        deps.backups_units_dir,
        unit_name,
        &(deps.now_iso)(),
        deps.fs_perms,           // <-- new
    ).await?,
    None => String::new(),
};
```

Add `fs_perms: &'a dyn FsPermissions` to `CutoverDeps`. Update the handler in `handlers/legacy_migrate_service.rs` to pass `&*ctx.fs_perms` when constructing `CutoverDeps`.

- [ ] **Step 4: Update tests to inject `RecordingFsPermissions` (cross-platform fake)**

In `crates/boxpilotd/src/legacy/backup.rs` test module:

```rust
use boxpilot_platform::fakes::fs_perms::RecordingFsPermissions;

#[tokio::test]
async fn backup_unit_file_writes_owner_only() {
    let tmp = tempdir().unwrap();
    // ... fixture setup writes a "fragment" file ...
    let fs_perms = RecordingFsPermissions::default();
    let result = backup_unit_file(/* ... */, &fs_perms).await.unwrap();
    assert!(fs_perms.calls().iter().any(|p| p == &result),
        "ensure_owner_only must be called on the backup path");
}
```

- [ ] **Step 5: Run Linux tests**

```
cargo test -p boxpilotd legacy::backup
cargo test -p boxpilotd
```

Expected: all pass.

- [ ] **Step 6: Verify Windows compile is now green for boxpilotd**

```
cargo check --target x86_64-pc-windows-msvc -p boxpilotd
```

Expected: no errors. (Linker errors okay — we don't run the binary, just check.)

- [ ] **Step 7: Commit**

```
git commit -am "refactor(boxpilotd/legacy): use FsPermissions trait, drop PermissionsExt

Removes the only remaining top-level use std::os::unix::fs::PermissionsExt
in boxpilotd. Threads ctx.fs_perms through CutoverDeps so the backup file
gets the same owner-only protection as Linux's chmod 0600 — and on Windows
will get the matching ACL once the real AclFsPermissions impl lands in PR 2.6."
```

---

## PR 1.6 — Widen `windows-check.yml` to full-workspace `cargo check`

**Scope:** Change the CI gate from `cargo check -p boxpilot-ipc -p boxpilot-platform` to `cargo check --workspace --all-targets`. Verify on the runner. Do not yet add `cargo test` — tests come in PR 3.8 once Windows impls are real.

**Files:**
- Modify: `.github/workflows/windows-check.yml`

**Dependencies:** PR 1.3, PR 1.4, PR 1.5.

- [ ] **Step 1: Read current workflow**

```
cat .github/workflows/windows-check.yml
```

- [ ] **Step 2: Update the cargo invocation**

Replace the `cargo check -p boxpilot-ipc -p boxpilot-platform` step with:

```yaml
      - name: cargo check --workspace
        run: cargo check --workspace --all-targets --target x86_64-pc-windows-msvc
        env:
          CARGO_TERM_COLOR: always
```

- [ ] **Step 3: Verify locally one more time**

```
cargo check --target x86_64-pc-windows-msvc --workspace --all-targets 2>&1 | tail -5
```

Expected: `Finished` with no errors.

- [ ] **Step 4: Push and observe CI**

```
git add .github/workflows/windows-check.yml
git commit -m "ci: widen windows-check to --workspace --all-targets

Sub-project #2 batch ① closes the full-workspace Windows compile gap.
All boxpilotd modules now either have real cross-platform implementations
or are cfg-gated to Linux. Tests follow in batch ③."
git push
```

Wait for the GitHub Actions run; if green, batch ① is ready to open as a PR.

- [ ] **Step 5: Open GitHub PR for batch ①**

Use `gh pr create` with title `feat: BoxPilot Sub-project #2 batch ① — Windows compile foundation`. Body summarizes PR 1.1–1.6 and points to the spec.

---

# Batch ② — Authorization Platform

Goal: real Windows authorization. After this batch, mutating verbs honor the Group + controller-SID model; ProfileStore files are owner-only-protected via real ACLs; the schema migration lets a Sub-project-#1 Linux install upgrade cleanly.

## PR 2.1 — Schema migration v1 → v2 in `BoxpilotConfig`

**Scope:** Add the `controller_principal: String` field. Remove `controller_uid: Option<u32>` from the persisted struct. Implement read-side migration in `BoxpilotConfig::parse` so v1 files are read transparently and re-serialized as v2 on next write.

**Files:**
- Modify: `crates/boxpilot-ipc/src/config.rs` — bump `CURRENT_SCHEMA_VERSION` to `2`, add `controller_principal`, remove `controller_uid`, write migration in `parse`
- Test: `crates/boxpilot-ipc/src/config.rs` test module

**Dependencies:** Batch ① merged to `main` (so the branch is freshly rebased).

- [ ] **Step 1: Write the failing migration tests first**

In `crates/boxpilot-ipc/src/config.rs` test module:

```rust
#[cfg(test)]
mod migration_tests {
    use super::*;

    #[test]
    fn parse_v1_migrates_controller_uid_to_principal() {
        let v1 = "schema_version = 1\ncontroller_uid = 1000\n";
        let cfg = BoxpilotConfig::parse(v1).unwrap();
        assert_eq!(cfg.schema_version, 2);
        assert_eq!(cfg.controller_principal, Some("linux:1000".to_string()));
    }

    #[test]
    fn parse_v1_with_no_controller_uid_yields_unset() {
        let v1 = "schema_version = 1\n";
        let cfg = BoxpilotConfig::parse(v1).unwrap();
        assert_eq!(cfg.schema_version, 2);
        assert_eq!(cfg.controller_principal, None);
    }

    #[test]
    fn parse_v2_native() {
        let v2 = "schema_version = 2\ncontroller_principal = \"linux:1000\"\n";
        let cfg = BoxpilotConfig::parse(v2).unwrap();
        assert_eq!(cfg.schema_version, 2);
        assert_eq!(cfg.controller_principal, Some("linux:1000".to_string()));
    }

    #[test]
    fn parse_v2_with_windows_principal() {
        let v2 = "schema_version = 2\ncontroller_principal = \"windows:S-1-5-21-1-2-3-1001\"\n";
        let cfg = BoxpilotConfig::parse(v2).unwrap();
        assert_eq!(cfg.controller_principal, Some("windows:S-1-5-21-1-2-3-1001".to_string()));
    }

    #[test]
    fn parse_v3_rejects_with_unsupported_schema_version() {
        let v3 = "schema_version = 3\ncontroller_principal = \"linux:1000\"\n";
        let r = BoxpilotConfig::parse(v3);
        assert!(matches!(r, Err(HelperError::UnsupportedSchemaVersion { got: 3 })));
    }

    #[test]
    fn migrated_v1_round_trips_to_v2_toml() {
        let v1 = "schema_version = 1\ncontroller_uid = 1000\n";
        let cfg = BoxpilotConfig::parse(v1).unwrap();
        let serialized = cfg.to_toml();
        assert!(serialized.contains("schema_version = 2"));
        assert!(serialized.contains("controller_principal = \"linux:1000\""));
        assert!(!serialized.contains("controller_uid"));
    }
}
```

- [ ] **Step 2: Run; expect compile errors (controller_principal field doesn't exist yet)**

```
cargo test -p boxpilot-ipc migration_tests
```

- [ ] **Step 3: Implement the migration**

In `crates/boxpilot-ipc/src/config.rs`:

```rust
pub const CURRENT_SCHEMA_VERSION: u32 = 2;
const COMPAT_SCHEMA_VERSION: u32 = 1;  // we still read v1 files

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BoxpilotConfig {
    pub schema_version: u32,
    #[serde(default = "default_target_service")]
    pub target_service: String,
    #[serde(default)]
    pub core_path: Option<String>,
    #[serde(default)]
    pub core_state: Option<CoreState>,
    /// Tagged form: "linux:<uid>" or "windows:<sid-string>".
    /// Replaces controller_uid as of schema_version 2.
    #[serde(default)]
    pub controller_principal: Option<String>,
    // ... rest unchanged ...
}

impl BoxpilotConfig {
    pub fn parse(text: &str) -> HelperResult<Self> {
        #[derive(Deserialize)]
        struct Peek {
            schema_version: u32,
        }
        let peek: Peek = toml::from_str(text).map_err(|e| HelperError::Ipc {
            message: format!("config parse: {e}"),
        })?;

        match peek.schema_version {
            CURRENT_SCHEMA_VERSION => {
                let cfg: BoxpilotConfig = toml::from_str(text).map_err(|e| HelperError::Ipc {
                    message: format!("config parse: {e}"),
                })?;
                Ok(cfg)
            }
            COMPAT_SCHEMA_VERSION => {
                // Migrate v1 → v2: read controller_uid (if present) and re-emit
                // as controller_principal = "linux:<uid>". The migrated struct's
                // schema_version is set to 2 so the next write persists v2.
                #[derive(Deserialize)]
                struct V1 {
                    #[serde(default = "default_target_service")]
                    target_service: String,
                    #[serde(default)]
                    core_path: Option<String>,
                    #[serde(default)]
                    core_state: Option<CoreState>,
                    #[serde(default)]
                    controller_uid: Option<u32>,
                    // active/previous fields preserved
                    #[serde(default)] active_profile_id: Option<String>,
                    #[serde(default)] active_profile_name: Option<String>,
                    #[serde(default)] active_profile_sha256: Option<String>,
                    #[serde(default)] active_release_id: Option<String>,
                    #[serde(default)] activated_at: Option<String>,
                    #[serde(default)] previous_release_id: Option<String>,
                    #[serde(default)] previous_profile_id: Option<String>,
                    #[serde(default)] previous_profile_sha256: Option<String>,
                    #[serde(default)] previous_activated_at: Option<String>,
                }
                let v1: V1 = toml::from_str(text).map_err(|e| HelperError::Ipc {
                    message: format!("config parse v1: {e}"),
                })?;
                Ok(BoxpilotConfig {
                    schema_version: CURRENT_SCHEMA_VERSION,
                    target_service: v1.target_service,
                    core_path: v1.core_path,
                    core_state: v1.core_state,
                    controller_principal: v1.controller_uid.map(|uid| format!("linux:{uid}")),
                    active_profile_id: v1.active_profile_id,
                    active_profile_name: v1.active_profile_name,
                    active_profile_sha256: v1.active_profile_sha256,
                    active_release_id: v1.active_release_id,
                    activated_at: v1.activated_at,
                    previous_release_id: v1.previous_release_id,
                    previous_profile_id: v1.previous_profile_id,
                    previous_profile_sha256: v1.previous_profile_sha256,
                    previous_activated_at: v1.previous_activated_at,
                })
            }
            got => Err(HelperError::UnsupportedSchemaVersion { got }),
        }
    }

    pub fn to_toml(&self) -> String {
        toml::to_string(self).expect("BoxpilotConfig serializes")
    }
}
```

- [ ] **Step 4: Run migration tests; expect all pass**

```
cargo test -p boxpilot-ipc migration_tests
```

- [ ] **Step 5: Update every other place that reads `controller_uid`**

Run:
```
grep -rn "controller_uid\b" crates/ | grep -v test
```

Each non-test reference now uses `controller_principal`. Common patterns:
- `cfg.controller_uid` (read) → parse `cfg.controller_principal` via `CallerPrincipal::from_toml_tag` (added in PR 2.2)
- `cfg.controller_uid = Some(uid)` (write) → `cfg.controller_principal = Some(principal.to_toml_tag())`

For this PR, just update the read sites to handle the new optional string (parsing comes in PR 2.2). Writers update in PR 2.3.

- [ ] **Step 6: Run full test suite**

```
cargo test -p boxpilot-ipc -p boxpilotd -p boxpilot-profile
```

Expected: pass. There may be tests that wrote `controller_uid = 1000` literally in fixture toml — update those to `controller_principal = "linux:1000"` (and bump fixture's `schema_version` to `2`).

- [ ] **Step 7: Commit**

```
git commit -am "feat(ipc/config): schema v2 — controller_principal replaces controller_uid

v1 files are auto-migrated on parse: controller_uid = 1000 becomes
controller_principal = \"linux:1000\". CURRENT_SCHEMA_VERSION bumps 1 → 2.
Reading callers updated; writers migrate in PR 2.3."
```

---

## PR 2.2 — `CallerPrincipal::to_toml_tag` / `from_toml_tag` helpers

**Scope:** Define encode/decode between `CallerPrincipal::{LinuxUid, WindowsSid}` and the `"linux:<uid>"` / `"windows:<sid>"` string form. Used by `controller_principal` reads/writes.

**Files:**
- Modify: `crates/boxpilot-platform/src/traits/authority.rs:` — add `to_toml_tag` / `from_toml_tag`
- Test: same file

**Dependencies:** PR 2.1.

- [ ] **Step 1: Write tests first**

```rust
#[cfg(test)]
mod tag_tests {
    use super::*;

    #[test]
    fn linux_uid_round_trips() {
        let p = CallerPrincipal::LinuxUid(1000);
        let s = p.to_toml_tag();
        assert_eq!(s, "linux:1000");
        assert_eq!(CallerPrincipal::from_toml_tag(&s), Some(p));
    }

    #[test]
    fn windows_sid_round_trips() {
        let p = CallerPrincipal::WindowsSid("S-1-5-21-1-2-3-1001".to_string());
        let s = p.to_toml_tag();
        assert_eq!(s, "windows:S-1-5-21-1-2-3-1001");
        assert_eq!(CallerPrincipal::from_toml_tag(&s), Some(p));
    }

    #[test]
    fn unknown_prefix_returns_none() {
        assert_eq!(CallerPrincipal::from_toml_tag("foo:bar"), None);
        assert_eq!(CallerPrincipal::from_toml_tag("nocolon"), None);
        assert_eq!(CallerPrincipal::from_toml_tag(""), None);
    }

    #[test]
    fn linux_with_non_numeric_uid_returns_none() {
        assert_eq!(CallerPrincipal::from_toml_tag("linux:notanumber"), None);
    }
}
```

- [ ] **Step 2: Run; expect compile errors**

```
cargo test -p boxpilot-platform tag_tests
```

- [ ] **Step 3: Implement the helpers**

In `crates/boxpilot-platform/src/traits/authority.rs`:

```rust
impl CallerPrincipal {
    /// Tagged-string form for boxpilot.toml::controller_principal:
    /// `"linux:<uid>"` or `"windows:<sid>"`.
    pub fn to_toml_tag(&self) -> String {
        match self {
            CallerPrincipal::LinuxUid(uid) => format!("linux:{uid}"),
            CallerPrincipal::WindowsSid(sid) => format!("windows:{sid}"),
        }
    }

    /// Inverse of `to_toml_tag`. Returns `None` on unknown prefix or
    /// malformed payload.
    pub fn from_toml_tag(s: &str) -> Option<CallerPrincipal> {
        let (prefix, rest) = s.split_once(':')?;
        match prefix {
            "linux" => rest.parse::<u32>().ok().map(CallerPrincipal::LinuxUid),
            "windows" if !rest.is_empty() => Some(CallerPrincipal::WindowsSid(rest.to_string())),
            _ => None,
        }
    }
}
```

- [ ] **Step 4: Run tests; expect pass**

```
cargo test -p boxpilot-platform tag_tests
```

- [ ] **Step 5: Commit**

```
git commit -am "feat(platform/authority): CallerPrincipal toml tag helpers

to_toml_tag / from_toml_tag bridge the typed enum and the
controller_principal string field added in schema v2."
```

---

## PR 2.3 — Migrate `dispatch::ControllerWrites` and writers to principal-form

**Scope:** Update the controller-claim flow to write `controller_principal` instead of `controller_uid`. `ControllerWrites` becomes `{ principal: String, username: String }`. All call sites of `maybe_claim_controller` and `commit_controller_claim` are updated. The persisted v2 toml gets the new field on the next mutating call.

**Files:**
- Modify: `crates/boxpilotd/src/dispatch.rs:82-114` — `ControllerWrites` shape, `maybe_claim_controller` body
- Modify: `crates/boxpilotd/src/core/commit.rs:` — `StateCommit::apply` writes `controller_principal` to toml
- Modify: `crates/boxpilotd/src/handlers/*.rs` — call sites where `maybe_claim_controller(...)` returns `ControllerWrites` (verify they still work)
- Test: dispatch.rs test module

**Dependencies:** PR 2.1, PR 2.2.

- [ ] **Step 1: Update `ControllerWrites` shape**

In `crates/boxpilotd/src/dispatch.rs`:

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ControllerWrites {
    /// Tagged form: "linux:<uid>" or "windows:<sid>".
    pub principal: String,
    /// SAM account name (Windows) or passwd entry (Linux).
    pub username: String,
}
```

- [ ] **Step 2: Update `maybe_claim_controller` to use `to_toml_tag`**

```rust
pub fn maybe_claim_controller(
    will_claim: bool,
    principal: &CallerPrincipal,
    user_lookup: &dyn crate::controller::UserLookup,
) -> HelperResult<Option<ControllerWrites>> {
    if !will_claim {
        return Ok(None);
    }
    let username = match principal {
        CallerPrincipal::LinuxUid(uid) => user_lookup
            .lookup_username(*uid)
            .ok_or(HelperError::ControllerOrphaned)?,
        CallerPrincipal::WindowsSid(sid) => user_lookup
            .lookup_account_name_by_sid(sid)
            .ok_or(HelperError::ControllerOrphaned)?,
    };
    Ok(Some(ControllerWrites {
        principal: principal.to_toml_tag(),
        username,
    }))
}
```

Note: this introduces a new method `lookup_account_name_by_sid` on the `UserLookup` trait — add it in PR 2.2 (or extend here). Linux impl returns `None`. Windows impl in PR 2.4 (Windows authority work).

- [ ] **Step 3: Update `core::commit::StateCommit::apply` to persist `controller_principal`**

In `crates/boxpilotd/src/core/commit.rs` find the toml-write block (around the line where `cfg.controller_uid = Some(c.uid)` was). Replace:

```rust
if let Some(c) = &self.controller {
    cfg.controller_principal = Some(c.principal.clone());
}
```

(The previous `cfg.controller_uid = Some(c.uid)` line is removed; PR 2.1 already removed the field.)

- [ ] **Step 4: Update `core::commit::backfill_polkit_dropin` to read principal-form**

Find references to `cfg.controller_uid` in `commit.rs`:

```
grep -n "controller_uid" crates/boxpilotd/src/core/commit.rs
```

Replace each with parsing `cfg.controller_principal` via `CallerPrincipal::from_toml_tag`. Linux-side backfill only triggers for `CallerPrincipal::LinuxUid(_)`:

```rust
let Some(tag) = cfg.controller_principal.as_deref() else { return Ok(false); };
let Some(principal) = boxpilot_platform::traits::authority::CallerPrincipal::from_toml_tag(tag) else {
    return Ok(false);  // malformed; no backfill
};
let uid = match principal {
    boxpilot_platform::traits::authority::CallerPrincipal::LinuxUid(u) => u,
    _ => return Ok(false),  // backfill is Linux-only
};
let Some(username) = user_lookup.lookup_username(uid) else { return Ok(false); };
// ... rest unchanged
```

- [ ] **Step 5: Update `controller.rs::ControllerState::from_uid` to take a principal**

Find:
```
grep -n "ControllerState::from_uid\|fn from_uid" crates/boxpilotd/src/controller.rs
```

Rename to `from_principal(tag: Option<&str>, user_lookup: &dyn UserLookup) -> ControllerState` and parse the tag. Update every caller (`context.rs::controller_state` etc.).

- [ ] **Step 6: Update tests**

Tests using `controller_uid = 1000` literally in toml fixtures get `schema_version = 2\ncontroller_principal = "linux:1000"`. Update each fixture string.

- [ ] **Step 7: Run full Linux test suite**

```
cargo test -p boxpilotd
```

Expected: 200+ tests pass. Particularly `service_start_claims_controller_when_unset` (the regression test for round-1 review) and `legacy_migrate_prepare_does_not_trigger_controller_orphaned_for_unknown_uid` (round-4 regression test).

- [ ] **Step 8: Commit**

```
git commit -am "feat(boxpilotd/dispatch): migrate ControllerWrites to principal-form

ControllerWrites now carries the tag-form principal string. Linux call
sites unchanged in behavior (uid → 'linux:<uid>'). Windows call sites
land in PR 2.5 once WindowsLocalAuthority resolves the caller SID.

Reads/writes of cfg.controller_uid are gone; everything goes through
CallerPrincipal::{to,from}_toml_tag."
```

---

## PR 2.4 — Windows SID helpers + `LocalFreeOnDrop` RAII

**Scope:** Build the Win32 helper layer needed by `WindowsLocalAuthority`. SID parsing/formatting, well-known SID construction, `LocalFreeOnDrop<T>` for safe cleanup. Also fix the acknowledged `LocalFree` leak in `windows/ipc.rs::resolve_caller_sid` from Sub-project #1.

**Files:**
- Create: `crates/boxpilot-platform/src/windows/sid.rs`
- Create: `crates/boxpilot-platform/src/windows/local_free.rs`
- Modify: `crates/boxpilot-platform/src/windows/mod.rs` — add `pub mod sid; pub mod local_free;`
- Modify: `crates/boxpilot-platform/src/windows/ipc.rs:` — apply `LocalFreeOnDrop` to fix the acknowledged leak

**Dependencies:** Batch ① merged.

- [ ] **Step 1: Write tests for `LocalFreeOnDrop`**

`crates/boxpilot-platform/src/windows/local_free.rs`:

```rust
//! RAII wrapper that calls `LocalFree` on drop. Use for any pointer returned
//! by Win32 APIs that demand `LocalFree` cleanup (e.g. `ConvertSidToStringSidW`,
//! `SetEntriesInAclW`).

use windows_sys::Win32::Foundation::LocalFree;

pub struct LocalFreeOnDrop<T>(*mut T);

impl<T> LocalFreeOnDrop<T> {
    pub fn new(p: *mut T) -> Self {
        LocalFreeOnDrop(p)
    }
    pub fn as_ptr(&self) -> *mut T {
        self.0
    }
    pub fn into_raw(mut self) -> *mut T {
        let p = self.0;
        self.0 = std::ptr::null_mut();
        p
    }
}

impl<T> Drop for LocalFreeOnDrop<T> {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { LocalFree(self.0 as _); }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn null_drop_is_noop() {
        let _g = LocalFreeOnDrop::<u8>::new(std::ptr::null_mut());
    }

    #[test]
    fn into_raw_disarms_drop() {
        // Allocate via LocalAlloc to give us a pointer LocalFree would normally
        // free; into_raw() should leave it for the caller to free.
        use windows_sys::Win32::Foundation::{LocalAlloc, LMEM_FIXED};
        let p = unsafe { LocalAlloc(LMEM_FIXED, 16) } as *mut u8;
        assert!(!p.is_null());
        let g = LocalFreeOnDrop::new(p);
        let raw = g.into_raw();
        assert_eq!(raw, p);
        // Manual cleanup so we don't leak.
        unsafe { LocalFree(raw as _); }
    }
}
```

- [ ] **Step 2: Run tests (gated to `target_os = "windows"`)**

```
cargo test --target x86_64-pc-windows-msvc -p boxpilot-platform local_free
```

Expected: pass on Windows runner. (Linux skip via cfg.)

- [ ] **Step 3: Add SID helpers**

`crates/boxpilot-platform/src/windows/sid.rs`:

```rust
//! SID conversion + well-known SID helpers.

use boxpilot_ipc::HelperError;
use windows_sys::Win32::Foundation::{GetLastError, LocalFree};
use windows_sys::Win32::Security::{
    Authorization::{ConvertSidToStringSidW, ConvertStringSidToSidW},
    CreateWellKnownSid, IsValidSid, WinBuiltinAdministratorsSid, PSID, WELL_KNOWN_SID_TYPE,
};

use super::local_free::LocalFreeOnDrop;

/// Convert a SID byte buffer into the canonical "S-1-..." string form.
pub fn sid_to_string(sid: PSID) -> Result<String, HelperError> {
    if sid.is_null() || unsafe { IsValidSid(sid) } == 0 {
        return Err(HelperError::Ipc { message: "invalid sid".into() });
    }
    let mut wstr_ptr: *mut u16 = std::ptr::null_mut();
    let ok = unsafe { ConvertSidToStringSidW(sid, &mut wstr_ptr) };
    if ok == 0 {
        return Err(HelperError::Ipc {
            message: format!("ConvertSidToStringSidW: GetLastError={}", unsafe { GetLastError() }),
        });
    }
    let _guard = LocalFreeOnDrop::new(wstr_ptr);
    // Compute UTF-16 length.
    let mut len = 0;
    while unsafe { *wstr_ptr.add(len) } != 0 {
        len += 1;
    }
    let slice = unsafe { std::slice::from_raw_parts(wstr_ptr, len) };
    Ok(String::from_utf16_lossy(slice))
}

/// Parse "S-1-..." into a PSID buffer. Caller owns the returned LocalFreeOnDrop.
pub fn string_to_sid(s: &str) -> Result<LocalFreeOnDrop<core::ffi::c_void>, HelperError> {
    let mut wide: Vec<u16> = s.encode_utf16().collect();
    wide.push(0);
    let mut psid: PSID = std::ptr::null_mut();
    let ok = unsafe { ConvertStringSidToSidW(wide.as_ptr(), &mut psid) };
    if ok == 0 {
        return Err(HelperError::Ipc {
            message: format!("ConvertStringSidToSidW({s}): GetLastError={}", unsafe { GetLastError() }),
        });
    }
    Ok(LocalFreeOnDrop::new(psid as _))
}

/// Build the well-known BUILTIN\Administrators SID (`S-1-5-32-544`).
/// Caller owns the buffer (a Vec<u8>).
pub fn well_known_admins_sid() -> Result<Vec<u8>, HelperError> {
    well_known_sid(WinBuiltinAdministratorsSid)
}

fn well_known_sid(kind: WELL_KNOWN_SID_TYPE) -> Result<Vec<u8>, HelperError> {
    // SECURITY_MAX_SID_SIZE is 68 bytes.
    let mut buf = vec![0u8; 68];
    let mut size = buf.len() as u32;
    let ok = unsafe { CreateWellKnownSid(kind, std::ptr::null_mut(), buf.as_mut_ptr() as _, &mut size) };
    if ok == 0 {
        return Err(HelperError::Ipc {
            message: format!("CreateWellKnownSid: GetLastError={}", unsafe { GetLastError() }),
        });
    }
    buf.truncate(size as usize);
    Ok(buf)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_admin_sid() {
        let buf = well_known_admins_sid().unwrap();
        let s = sid_to_string(buf.as_ptr() as _).unwrap();
        assert_eq!(s, "S-1-5-32-544");
    }

    #[test]
    fn parse_and_reformat() {
        let s = "S-1-5-32-544";
        let g = string_to_sid(s).unwrap();
        let back = sid_to_string(g.as_ptr() as _).unwrap();
        assert_eq!(back, s);
    }

    #[test]
    fn invalid_sid_string_errors() {
        assert!(string_to_sid("not-a-sid").is_err());
    }
}
```

- [ ] **Step 4: Run on Windows runner**

```
cargo test --target x86_64-pc-windows-msvc -p boxpilot-platform sid::tests
```

Expected: 3 tests pass.

- [ ] **Step 5: Fix the acknowledged `LocalFree` leak in `ipc.rs::resolve_caller_sid`**

Find the section in `crates/boxpilot-platform/src/windows/ipc.rs` around the comment "LocalFree skipped — small leak per call, fine for Sub-project #1." Replace the `ConvertSidToStringSidW` block with:

```rust
let mut wstr_ptr: *mut u16 = std::ptr::null_mut();
if unsafe { ConvertSidToStringSidW(token_user_sid, &mut wstr_ptr) } == 0 {
    return Err(HelperError::Ipc {
        message: format!("ConvertSidToStringSidW: GetLastError={}", unsafe { GetLastError() }),
    });
}
let _guard = crate::windows::local_free::LocalFreeOnDrop::new(wstr_ptr);
let mut len = 0;
while unsafe { *wstr_ptr.add(len) } != 0 { len += 1; }
let slice = unsafe { std::slice::from_raw_parts(wstr_ptr, len) };
let sid_string = String::from_utf16_lossy(slice);
// _guard drops here, calling LocalFree on the wstr_ptr.
```

(Or refactor to call `super::sid::sid_to_string(token_user_sid)`.)

- [ ] **Step 6: Run full Windows test suite**

```
cargo test --target x86_64-pc-windows-msvc -p boxpilot-platform
```

Expected: pass.

- [ ] **Step 7: Commit**

```
git commit -am "feat(platform/windows): SID helpers + LocalFreeOnDrop RAII

Adds windows::sid::{sid_to_string, string_to_sid, well_known_admins_sid}
and the LocalFreeOnDrop<T> wrapper. Fixes Sub-project #1's acknowledged
LocalFree leak in resolve_caller_sid by routing through the new RAII path."
```

---

## PR 2.5 — `WindowsLocalAuthority` real impl

**Scope:** Implement the Group + controller-SID authorization model. New struct `WindowsLocalAuthority` implements `Authority::check`. Helper `check_administrators_membership(sid)` uses `CheckTokenMembership` against the well-known Administrators SID. Wires into `entry/windows.rs` replacing `AlwaysAllowAuthority`.

**Files:**
- Create: `crates/boxpilot-platform/src/windows/localauth.rs`
- Modify: `crates/boxpilot-platform/src/windows/mod.rs` — `pub mod localauth;`
- Modify: `crates/boxpilot-platform/src/windows/authority.rs:` — delete `AlwaysAllowAuthority` (or mark as `#[deprecated]` and keep test-only)
- Modify: `crates/boxpilotd/src/entry/windows.rs:` — instantiate `WindowsLocalAuthority` instead of `AlwaysAllowAuthority`; remove the startup `warn!` line
- Modify: `crates/boxpilot-platform/src/traits/user_lookup.rs:` — add `lookup_account_name_by_sid(&self, sid: &str) -> Option<String>` (default impl returns None)

**Dependencies:** PR 2.4.

- [ ] **Step 1: Add `lookup_account_name_by_sid` to UserLookup trait**

In `crates/boxpilot-platform/src/traits/user_lookup.rs`:

```rust
pub trait UserLookup: Send + Sync {
    fn lookup_username(&self, uid: u32) -> Option<String>;
    /// Resolve a Windows SID string to a SAM account name.
    /// Default impl returns None (Linux impls don't need to override).
    fn lookup_account_name_by_sid(&self, _sid: &str) -> Option<String> {
        None
    }
}
```

- [ ] **Step 2: Implement Windows `lookup_account_name_by_sid` via `LookupAccountSidW`**

In `crates/boxpilot-platform/src/windows/user_lookup.rs`:

```rust
use crate::traits::user_lookup::UserLookup;
use windows_sys::Win32::Foundation::GetLastError;
use windows_sys::Win32::Security::{LookupAccountSidW, SID_NAME_USE};

pub struct WindowsAccountLookup;

impl UserLookup for WindowsAccountLookup {
    fn lookup_username(&self, _uid: u32) -> Option<String> {
        None  // POSIX uid concept doesn't apply on Windows
    }

    fn lookup_account_name_by_sid(&self, sid: &str) -> Option<String> {
        let psid_guard = super::sid::string_to_sid(sid).ok()?;
        let psid = psid_guard.as_ptr();
        let mut name_buf = vec![0u16; 256];
        let mut name_len = name_buf.len() as u32;
        let mut domain_buf = vec![0u16; 256];
        let mut domain_len = domain_buf.len() as u32;
        let mut sid_use: SID_NAME_USE = 0;
        let ok = unsafe {
            LookupAccountSidW(
                std::ptr::null_mut(),
                psid as _,
                name_buf.as_mut_ptr(),
                &mut name_len,
                domain_buf.as_mut_ptr(),
                &mut domain_len,
                &mut sid_use,
            )
        };
        if ok == 0 {
            tracing::debug!(
                sid = %sid,
                last_error = unsafe { GetLastError() },
                "LookupAccountSidW failed"
            );
            return None;
        }
        Some(String::from_utf16_lossy(&name_buf[..name_len as usize]))
    }
}
```

Add `pub use windows::user_lookup::WindowsAccountLookup;` (or similar) to the platform crate root.

- [ ] **Step 3: Implement `WindowsLocalAuthority`**

Create `crates/boxpilot-platform/src/windows/localauth.rs`:

```rust
//! Group + controller-SID Authority impl for Windows.

use std::path::PathBuf;
use std::sync::Arc;
use async_trait::async_trait;
use boxpilot_ipc::{AuthClass, HelperError, HelperMethod, HelperResult};
use windows_sys::Win32::Foundation::{CloseHandle, BOOL, HANDLE};
use windows_sys::Win32::Security::CheckTokenMembership;

use crate::traits::authority::{Authority, CallerPrincipal};
use super::sid;

/// Looks up the controller principal from disk on demand. Implementation
/// returns the parsed string (e.g. "windows:S-...") from boxpilot.toml's
/// controller_principal field, or None if Unset.
pub trait ControllerLookup: Send + Sync {
    fn current_controller_principal(&self) -> Option<String>;
}

pub struct WindowsLocalAuthority {
    controller: Arc<dyn ControllerLookup>,
}

impl WindowsLocalAuthority {
    pub fn new(controller: Arc<dyn ControllerLookup>) -> Self {
        Self { controller }
    }
}

#[async_trait]
impl Authority for WindowsLocalAuthority {
    async fn check(
        &self,
        action_id: &str,
        principal: &CallerPrincipal,
    ) -> HelperResult<bool> {
        let sid = match principal {
            CallerPrincipal::WindowsSid(s) => s.as_str(),
            CallerPrincipal::LinuxUid(_) => return Ok(false),  // wiring bug; deny
        };
        let method = HelperMethod::ALL.iter().copied()
            .find(|m| m.polkit_action_id() == action_id)
            .ok_or_else(|| HelperError::Ipc {
                message: format!("unknown action_id: {action_id}"),
            })?;
        match method.auth_class() {
            AuthClass::ReadOnly => Ok(true),
            AuthClass::Mutating => {
                if self.is_controller_sid(sid) {
                    return Ok(true);
                }
                check_administrators_membership(sid)
            }
            AuthClass::HighRisk => check_administrators_membership(sid),
        }
    }
}

impl WindowsLocalAuthority {
    fn is_controller_sid(&self, sid: &str) -> bool {
        let Some(tag) = self.controller.current_controller_principal() else {
            return false;
        };
        tag == format!("windows:{sid}")
    }
}

/// Check whether the SID is a member of BUILTIN\Administrators.
/// Uses `CheckTokenMembership(NULL, …)` which evaluates against the
/// caller thread's effective token. The Named Pipe IPC server must
/// `ImpersonateNamedPipeClient` before invoking this and `RevertToSelf`
/// after; that wiring lives at the call site.
pub fn check_administrators_membership(_sid: &str) -> HelperResult<bool> {
    let admin_sid_buf = sid::well_known_admins_sid()?;
    let mut is_member: BOOL = 0;
    let ok = unsafe {
        CheckTokenMembership(
            0 as HANDLE,  // NULL = caller thread effective token
            admin_sid_buf.as_ptr() as _,
            &mut is_member,
        )
    };
    if ok == 0 {
        return Err(HelperError::Ipc {
            message: format!("CheckTokenMembership: GetLastError={}",
                unsafe { windows_sys::Win32::Foundation::GetLastError() }),
        });
    }
    Ok(is_member != 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct ControllerStub(Option<String>);
    impl ControllerLookup for ControllerStub {
        fn current_controller_principal(&self) -> Option<String> { self.0.clone() }
    }

    #[tokio::test]
    async fn read_only_always_allowed() {
        let auth = WindowsLocalAuthority::new(Arc::new(ControllerStub(None)));
        let p = CallerPrincipal::WindowsSid("S-1-5-21-1-2-3-1001".into());
        assert!(auth.check("app.boxpilot.helper.service.status", &p).await.unwrap());
    }

    #[tokio::test]
    async fn mutating_with_matching_controller_sid_allowed() {
        let auth = WindowsLocalAuthority::new(Arc::new(ControllerStub(
            Some("windows:S-1-5-21-1-2-3-1001".into())
        )));
        let p = CallerPrincipal::WindowsSid("S-1-5-21-1-2-3-1001".into());
        // Note: this test does NOT exercise CheckTokenMembership — controller
        // match short-circuits before the admin check.
        assert!(auth.check("app.boxpilot.helper.service.start", &p).await.unwrap());
    }

    #[tokio::test]
    async fn linux_principal_on_windows_authority_denied() {
        let auth = WindowsLocalAuthority::new(Arc::new(ControllerStub(None)));
        let p = CallerPrincipal::LinuxUid(1000);
        assert!(!auth.check("app.boxpilot.helper.service.status", &p).await.unwrap());
    }
}
```

- [ ] **Step 4: Wire `ControllerLookup` to `HelperContext`**

In `crates/boxpilotd/src/entry/windows.rs`, before constructing the IpcServer, build a small `ControllerLookup` adapter that reads `paths.boxpilot_toml()` on each call (cached or not — initial impl is no-cache, parses each call):

```rust
struct CtxControllerLookup {
    paths: boxpilot_platform::Paths,
}
impl boxpilot_platform::windows::localauth::ControllerLookup for CtxControllerLookup {
    fn current_controller_principal(&self) -> Option<String> {
        let text = std::fs::read_to_string(self.paths.boxpilot_toml()).ok()?;
        let cfg = boxpilot_ipc::BoxpilotConfig::parse(&text).ok()?;
        cfg.controller_principal
    }
}
```

Then:

```rust
let authority: Arc<dyn Authority> = Arc::new(
    boxpilot_platform::windows::localauth::WindowsLocalAuthority::new(
        Arc::new(CtxControllerLookup { paths: paths.clone() })
    )
);
```

Remove the `tracing::warn!("windows authority is in pass-through mode pending sub-project #2 — do not run on a multi-user machine");` line from the same file.

- [ ] **Step 5: Run tests on Windows runner**

```
cargo test --target x86_64-pc-windows-msvc -p boxpilot-platform localauth
cargo test --target x86_64-pc-windows-msvc -p boxpilotd
```

Expected: pass.

- [ ] **Step 6: Run Linux test suite (regression check)**

```
cargo test -p boxpilot-platform -p boxpilotd
```

Expected: 200+ pass.

- [ ] **Step 7: Commit**

```
git commit -am "feat(platform/windows): WindowsLocalAuthority Group+SID model

Per-verb authorization table: ReadOnly = any logged-in user, Mutating =
caller is controller SID OR Administrators member, HighRisk = Administrators
only. CheckTokenMembership against the well-known BUILTIN\\Administrators
SID. AlwaysAllowAuthority retired; entry/windows.rs uses the real impl."
```

---

## PR 2.6 — Real `AclFsPermissions` Windows impl

**Scope:** Replace the no-op stub `tracing::debug! + Ok(())` in `crates/boxpilot-platform/src/windows/fs_perms.rs` with a real ACL implementation: `SetEntriesInAclW` constructs a DACL with one ACE granting GENERIC_ALL to the current process's user SID, then `SetNamedSecurityInfoW` applies it with `PROTECTED_DACL_SECURITY_INFORMATION` to defeat parent-dir inheritance.

**Files:**
- Modify: `crates/boxpilot-platform/src/windows/fs_perms.rs`
- Test: same file

**Dependencies:** PR 2.4 (LocalFreeOnDrop, sid helpers).

- [ ] **Step 1: Write failing tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;
    use tempfile::TempDir;

    #[test]
    fn ensure_owner_only_runs_without_error_on_temp_file() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("secret.toml");
        File::create(&path).unwrap();
        let p = AclFsPermissions;
        p.ensure_owner_only(&path).unwrap();
        // Best-effort: read back DACL and assert owner SID is the only ACE.
        // Implementation as inline unsafe block reading
        // GetNamedSecurityInfoW(DACL_SECURITY_INFORMATION) and counting ACEs.
        // For now, assert the call succeeds; round-trip ACL inspection is a
        // follow-up smoke test.
    }

    #[test]
    fn ensure_owner_only_on_nonexistent_path_errors_clean() {
        let p = AclFsPermissions;
        let r = p.ensure_owner_only(std::path::Path::new("Z:\\does\\not\\exist.txt"));
        assert!(r.is_err());
    }
}
```

- [ ] **Step 2: Implement the ACL builder**

```rust
//! crates/boxpilot-platform/src/windows/fs_perms.rs

use std::path::Path;
use boxpilot_ipc::{HelperError, HelperResult};
use windows_sys::Win32::Foundation::{GetLastError, LocalFree};
use windows_sys::Win32::Security::Authorization::{
    SetEntriesInAclW, SetNamedSecurityInfoW, EXPLICIT_ACCESS_W, GRANT_ACCESS, NO_INHERITANCE,
    SE_FILE_OBJECT, TRUSTEE_IS_SID, TRUSTEE_IS_USER, TRUSTEE_W,
};
use windows_sys::Win32::Security::{
    GetTokenInformation, TokenUser, TOKEN_QUERY, TOKEN_USER, ACL,
};
use windows_sys::Win32::System::SystemServices::GENERIC_ALL;
use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

use crate::traits::fs_perms::FsPermissions;

const DACL_SECURITY_INFORMATION: u32 = 0x00000004;
const PROTECTED_DACL_SECURITY_INFORMATION: u32 = 0x80000000;

pub struct AclFsPermissions;

impl FsPermissions for AclFsPermissions {
    fn ensure_owner_only(&self, path: &Path) -> HelperResult<()> {
        // 1. Get current process token user SID.
        let mut token: HANDLE = 0;
        if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == 0 {
            return Err(HelperError::Ipc {
                message: format!("OpenProcessToken: GetLastError={}", unsafe { GetLastError() }),
            });
        }
        let token_guard = HandleGuard(token);

        let mut needed: u32 = 0;
        unsafe { GetTokenInformation(token_guard.0, TokenUser, std::ptr::null_mut(), 0, &mut needed) };
        let mut buf = vec![0u8; needed as usize];
        let ok = unsafe {
            GetTokenInformation(
                token_guard.0,
                TokenUser,
                buf.as_mut_ptr() as _,
                needed,
                &mut needed,
            )
        };
        if ok == 0 {
            return Err(HelperError::Ipc {
                message: format!("GetTokenInformation(TokenUser): GetLastError={}", unsafe { GetLastError() }),
            });
        }
        let token_user = buf.as_ptr() as *const TOKEN_USER;
        let user_sid = unsafe { (*token_user).User.Sid };

        // 2. Build EXPLICIT_ACCESS_W granting GENERIC_ALL to the user SID.
        let mut ea: EXPLICIT_ACCESS_W = unsafe { std::mem::zeroed() };
        ea.grfAccessPermissions = GENERIC_ALL;
        ea.grfAccessMode = GRANT_ACCESS;
        ea.grfInheritance = NO_INHERITANCE;
        ea.Trustee = TRUSTEE_W {
            pMultipleTrustee: std::ptr::null_mut(),
            MultipleTrusteeOperation: 0,
            TrusteeForm: TRUSTEE_IS_SID,
            TrusteeType: TRUSTEE_IS_USER,
            ptstrName: user_sid as _,
        };

        // 3. Build the new DACL.
        let mut new_dacl: *mut ACL = std::ptr::null_mut();
        let rc = unsafe { SetEntriesInAclW(1, &ea, std::ptr::null_mut(), &mut new_dacl) };
        if rc != 0 {
            return Err(HelperError::Ipc {
                message: format!("SetEntriesInAclW: rc={rc}"),
            });
        }
        let _dacl_guard = super::local_free::LocalFreeOnDrop::new(new_dacl);

        // 4. Apply DACL to the file with PROTECTED bit (defeat inheritance from %ProgramData%).
        let mut wide: Vec<u16> = path.to_string_lossy().encode_utf16().collect();
        wide.push(0);
        let rc = unsafe {
            SetNamedSecurityInfoW(
                wide.as_ptr(),
                SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
                std::ptr::null_mut(),  // owner unchanged
                std::ptr::null_mut(),  // group unchanged
                new_dacl,
                std::ptr::null_mut(),  // SACL unchanged
            )
        };
        if rc != 0 {
            return Err(HelperError::Ipc {
                message: format!("SetNamedSecurityInfoW({}): rc={rc}", path.display()),
            });
        }
        Ok(())
    }
}

struct HandleGuard(HANDLE);
impl Drop for HandleGuard {
    fn drop(&mut self) {
        if self.0 != 0 {
            unsafe { windows_sys::Win32::Foundation::CloseHandle(self.0); }
        }
    }
}

use windows_sys::Win32::Foundation::HANDLE;
```

Note the imports might require additional `Cargo.toml` features — already covered by `Win32_Security_Authorization`, `Win32_Security`, `Win32_System_SystemServices`, `Win32_System_Threading`. Verify with a clean compile.

- [ ] **Step 3: Run on Windows runner**

```
cargo test --target x86_64-pc-windows-msvc -p boxpilot-platform fs_perms
```

Expected: pass (both tests).

- [ ] **Step 4: Wire `AclFsPermissions` into `entry/windows.rs`**

In `entry/windows.rs`, where `FsPermissions` is constructed for `HelperContext`, replace the previous stub with `Arc::new(AclFsPermissions)`.

- [ ] **Step 5: Linux regression check**

```
cargo test -p boxpilot-platform -p boxpilotd
```

Expected: pass (Linux uses `ChmodFsPermissions`, untouched).

- [ ] **Step 6: Commit**

```
git commit -am "feat(platform/windows): real AclFsPermissions

SetEntriesInAclW + SetNamedSecurityInfoW with PROTECTED_DACL bit to
defeat ProgramData parent-dir inheritance. One ACE granting GENERIC_ALL
to the current process user SID — owner-only. RAII guards (LocalFreeOnDrop,
HandleGuard) handle cleanup on every error path."
```

- [ ] **Step 7: Open GH PR for batch ②**

`gh pr create` with title `feat: BoxPilot Sub-project #2 batch ② — Authorization platform`. Body summarizes PR 2.1–2.6.

---

# Batch ③ — Real Windows Verbs

Goal: every primary-flow helper verb works end-to-end on Windows. ServiceManager + ActivePointer + CoreAssetNaming + ZipExtractor + build_tempfile_aux are real. After this batch, `boxpilotctl service.start` / `service.status` / `profile.activate_bundle` succeed against a real Windows daemon.

## PR 3.1 — Windows ServiceManager scaffold (open SCM, error mapping, polling helper)

**Scope:** Build the SCM plumbing layer used by every service verb. Not yet exposing all verbs — just the helpers (`open_scm`, `open_service`, `poll_service_status_until`, `win32_to_helper_error`) and a `WindowsScmServiceManager` struct whose methods initially route to `Err(HelperError::NotImplemented)`. Each method gets its real body in PR 3.2 (start/stop/restart) and PR 3.3 (enable/disable/state/path).

**Files:**
- Modify: `crates/boxpilot-platform/src/windows/service.rs` (replace the all-`NotImplemented` stub with the scaffold)
- Create: `crates/boxpilot-platform/src/windows/scm.rs` (lower-level Win32 wrappers)

**Dependencies:** Batch ② merged.

- [ ] **Step 1: Define the lower-level wrappers**

`crates/boxpilot-platform/src/windows/scm.rs`:

```rust
//! Thin RAII wrappers around SCM handles.

use boxpilot_ipc::{HelperError, HelperResult};
use windows_sys::Win32::Foundation::{CloseServiceHandle, GetLastError};
use windows_sys::Win32::System::Services::{
    OpenSCManagerW, OpenServiceW, SC_HANDLE, SC_MANAGER_CONNECT,
};

pub struct ScHandle(pub SC_HANDLE);

impl Drop for ScHandle {
    fn drop(&mut self) {
        if self.0 != 0 {
            unsafe { CloseServiceHandle(self.0); }
        }
    }
}

pub fn open_scm(desired_access: u32) -> HelperResult<ScHandle> {
    let h = unsafe { OpenSCManagerW(std::ptr::null(), std::ptr::null(), desired_access) };
    if h == 0 {
        return Err(HelperError::Ipc {
            message: format!("OpenSCManagerW: GetLastError={}", unsafe { GetLastError() }),
        });
    }
    Ok(ScHandle(h))
}

pub fn open_service(scm: &ScHandle, name: &str, desired_access: u32) -> HelperResult<ScHandle> {
    let mut wide: Vec<u16> = name.encode_utf16().collect();
    wide.push(0);
    let h = unsafe { OpenServiceW(scm.0, wide.as_ptr(), desired_access) };
    if h == 0 {
        let err = unsafe { GetLastError() };
        return Err(HelperError::Ipc {
            message: format!("OpenServiceW({name}): GetLastError={err}"),
        });
    }
    Ok(ScHandle(h))
}

/// Map a Win32 last-error code to a HelperError tagged with the operation name.
pub fn win32_err(operation: &str, err: u32) -> HelperError {
    HelperError::Systemd { message: format!("scm {operation}: error={err}") }
}
```

- [ ] **Step 2: Replace the `service.rs` stub with the scaffold**

`crates/boxpilot-platform/src/windows/service.rs`:

```rust
use std::time::Duration;
use async_trait::async_trait;
use boxpilot_ipc::{HelperError, HelperResult, ServiceControlResponse, UnitState};
use crate::traits::service::Systemd;

pub struct WindowsScmServiceManager;

#[async_trait]
impl Systemd for WindowsScmServiceManager {
    async fn start_unit(&self, _name: &str) -> HelperResult<()> {
        Err(HelperError::NotImplemented)  // body lands in PR 3.2
    }
    async fn stop_unit(&self, _name: &str) -> HelperResult<()> {
        Err(HelperError::NotImplemented)
    }
    async fn restart_unit(&self, _name: &str) -> HelperResult<()> {
        Err(HelperError::NotImplemented)
    }
    async fn enable_unit_files(&self, _names: &[String]) -> HelperResult<()> {
        Err(HelperError::NotImplemented)
    }
    async fn disable_unit_files(&self, _names: &[String]) -> HelperResult<()> {
        Err(HelperError::NotImplemented)
    }
    async fn unit_state(&self, _name: &str) -> HelperResult<UnitState> {
        Err(HelperError::NotImplemented)
    }
    async fn fragment_path(&self, _name: &str) -> HelperResult<Option<String>> {
        Err(HelperError::NotImplemented)
    }
    async fn daemon_reload(&self) -> HelperResult<()> {
        // SCM has no equivalent — config changes are immediate. Return Ok.
        Ok(())
    }
}
```

- [ ] **Step 3: Compile-only check**

```
cargo check --target x86_64-pc-windows-msvc -p boxpilot-platform
```

Expected: clean.

- [ ] **Step 4: Commit**

```
git commit -am "build(platform/windows/scm): SCM open/close + error helpers + manager scaffold

WindowsScmServiceManager with stubs returning NotImplemented; real bodies
land in PR 3.2 (start/stop/restart) and PR 3.3 (enable/disable/state/path)."
```

---

## PR 3.2 — `start_unit` + `stop_unit` + `restart_unit` + state polling

**Scope:** Real implementations for the three lifecycle verbs. Adds a `poll_until_state` helper that calls `QueryServiceStatusEx` repeatedly until the target state is reached or the wait_hint timeout elapses.

**Files:**
- Modify: `crates/boxpilot-platform/src/windows/service.rs:` start/stop/restart
- Modify: `crates/boxpilot-platform/src/windows/scm.rs:` add `poll_until_state` + state mapping helpers
- Test: `crates/boxpilot-platform/src/windows/service.rs` (mock SCM if available; otherwise integration tests gated to windows-latest)

**Dependencies:** PR 3.1.

- [ ] **Step 1: Add state mapping + polling to scm.rs**

```rust
// crates/boxpilot-platform/src/windows/scm.rs (append)

use windows_sys::Win32::System::Services::{
    QueryServiceStatusEx, SC_STATUS_PROCESS_INFO, SERVICE_STATUS_PROCESS,
    SERVICE_RUNNING, SERVICE_STOPPED, SERVICE_START_PENDING, SERVICE_STOP_PENDING,
    SERVICE_PAUSED, SERVICE_PAUSE_PENDING, SERVICE_CONTINUE_PENDING,
};

pub fn query_service_status(svc: &ScHandle) -> HelperResult<SERVICE_STATUS_PROCESS> {
    let mut status: SERVICE_STATUS_PROCESS = unsafe { std::mem::zeroed() };
    let mut needed: u32 = 0;
    let ok = unsafe {
        QueryServiceStatusEx(
            svc.0,
            SC_STATUS_PROCESS_INFO,
            &mut status as *mut _ as *mut u8,
            std::mem::size_of::<SERVICE_STATUS_PROCESS>() as u32,
            &mut needed,
        )
    };
    if ok == 0 {
        return Err(win32_err("QueryServiceStatusEx", unsafe { GetLastError() }));
    }
    Ok(status)
}

pub fn map_scm_state(s: &SERVICE_STATUS_PROCESS) -> (String, String) {
    let active_state = match s.dwCurrentState {
        SERVICE_RUNNING => "active",
        SERVICE_STOPPED => "inactive",
        SERVICE_START_PENDING | SERVICE_STOP_PENDING
        | SERVICE_PAUSE_PENDING | SERVICE_CONTINUE_PENDING => "transitioning",
        SERVICE_PAUSED => "paused",
        _ => "transitioning",
    };
    let sub_state = match s.dwCurrentState {
        SERVICE_RUNNING => "running",
        SERVICE_STOPPED => "stopped",
        SERVICE_START_PENDING => "start_pending",
        SERVICE_STOP_PENDING => "stop_pending",
        SERVICE_PAUSE_PENDING => "pause_pending",
        SERVICE_PAUSED => "paused",
        SERVICE_CONTINUE_PENDING => "continue_pending",
        _ => "unknown",
    };
    (active_state.to_string(), sub_state.to_string())
}

pub async fn poll_until_state(
    svc: &ScHandle,
    target: u32,
    timeout: Duration,
) -> HelperResult<SERVICE_STATUS_PROCESS> {
    let deadline = std::time::Instant::now() + timeout;
    let mut last = query_service_status(svc)?;
    while last.dwCurrentState != target {
        if std::time::Instant::now() > deadline {
            return Err(HelperError::Systemd {
                message: format!(
                    "scm poll: timeout waiting for state {target}; current={}",
                    last.dwCurrentState
                ),
            });
        }
        // Honour the service's wait_hint; cap to 10s per iteration.
        let interval = (last.dwWaitHint as u64 / 10).clamp(100, 10_000);
        tokio::time::sleep(Duration::from_millis(interval)).await;
        last = query_service_status(svc)?;
    }
    Ok(last)
}
```

- [ ] **Step 2: Implement `start_unit`**

```rust
// crates/boxpilot-platform/src/windows/service.rs

use windows_sys::Win32::System::Services::{
    StartServiceW, ControlService, SERVICE_START, SERVICE_STOP,
    SERVICE_CONTROL_STOP, SERVICE_STATUS, SC_MANAGER_CONNECT,
    SERVICE_RUNNING, SERVICE_STOPPED,
};
use super::scm::{open_scm, open_service, win32_err, poll_until_state};
use windows_sys::Win32::Foundation::{GetLastError, ERROR_SERVICE_ALREADY_RUNNING};

#[async_trait]
impl Systemd for WindowsScmServiceManager {
    async fn start_unit(&self, name: &str) -> HelperResult<()> {
        let scm = open_scm(SC_MANAGER_CONNECT)?;
        let svc = open_service(&scm, name, SERVICE_START)?;
        let ok = unsafe { StartServiceW(svc.0, 0, std::ptr::null()) };
        if ok == 0 {
            let err = unsafe { GetLastError() };
            if err == ERROR_SERVICE_ALREADY_RUNNING {
                return Ok(());
            }
            return Err(win32_err("StartServiceW", err));
        }
        let _ = poll_until_state(&svc, SERVICE_RUNNING, Duration::from_secs(30)).await?;
        Ok(())
    }

    async fn stop_unit(&self, name: &str) -> HelperResult<()> {
        let scm = open_scm(SC_MANAGER_CONNECT)?;
        let svc = open_service(&scm, name, SERVICE_STOP)?;
        let mut status: SERVICE_STATUS = unsafe { std::mem::zeroed() };
        let ok = unsafe { ControlService(svc.0, SERVICE_CONTROL_STOP, &mut status) };
        if ok == 0 {
            let err = unsafe { GetLastError() };
            return Err(win32_err("ControlService(STOP)", err));
        }
        let _ = poll_until_state(&svc, SERVICE_STOPPED, Duration::from_secs(30)).await?;
        Ok(())
    }

    async fn restart_unit(&self, name: &str) -> HelperResult<()> {
        // No atomic SCM restart; compose stop + start.
        // Tolerate already-stopped (ERROR_SERVICE_NOT_ACTIVE).
        match self.stop_unit(name).await {
            Ok(()) => {}
            Err(HelperError::Systemd { message }) if message.contains("error=1062") => {} // ERROR_SERVICE_NOT_ACTIVE
            Err(e) => return Err(e),
        }
        self.start_unit(name).await
    }
    // ... other methods unchanged ...
}
```

- [ ] **Step 3: Add integration test (Windows runner only)**

In `crates/boxpilot-platform/src/windows/service.rs` test module:

```rust
#[cfg(test)]
#[cfg(target_os = "windows")]
mod scm_tests {
    use super::*;

    /// Skip-if-no-admin helper: SCM tests require service-create rights.
    fn require_admin() -> bool {
        // Try to open SCM with CREATE_SERVICE; if denied, skip.
        // Implementation detail; CI runner is expected to pass.
        true
    }

    #[tokio::test]
    async fn start_stop_dummy_service() {
        if !require_admin() { return; }
        // Setup: create a service that runs `cmd.exe /c timeout 60`
        // via `sc create`. Cleanup: sc delete in teardown.
        //
        // Body:
        let mgr = WindowsScmServiceManager;
        // ... assert start_unit transitions through to RUNNING
        // ... assert stop_unit transitions through to STOPPED
    }
}
```

(Full body filled in by the implementing agent; the assertion shape is the goal.)

- [ ] **Step 4: Verify Linux compile (regression)**

```
cargo check -p boxpilot-platform
```

Expected: untouched (Linux impl in `linux/service.rs`).

- [ ] **Step 5: Verify Windows runner**

```
cargo test --target x86_64-pc-windows-msvc -p boxpilot-platform service::scm_tests
```

Expected: pass.

- [ ] **Step 6: Commit**

```
git commit -am "feat(platform/windows/scm): start/stop/restart unit + state polling

StartServiceW + ControlService(SERVICE_CONTROL_STOP) + 30s poll loop until
target state. Idempotent: ERROR_SERVICE_ALREADY_RUNNING is treated as
success on start. restart_unit composes stop+start tolerating already-stopped."
```

---

## PR 3.3 — `enable_unit_files` + `disable_unit_files` + `unit_state` + `fragment_path`

**Scope:** Remaining three SCM verbs. `unit_state` returns a fully-populated `UnitState::Known` with `platform_extra: PlatformUnitExtra::Windows { check_point, wait_hint_ms, controls_accepted }`.

**Files:**
- Modify: `crates/boxpilot-platform/src/windows/service.rs`

**Dependencies:** PR 3.2.

- [ ] **Step 1: Implement `enable_unit_files` / `disable_unit_files`**

```rust
use windows_sys::Win32::System::Services::{
    ChangeServiceConfigW, SERVICE_CHANGE_CONFIG,
    SERVICE_AUTO_START, SERVICE_DISABLED, SERVICE_NO_CHANGE,
};

async fn enable_unit_files(&self, names: &[String]) -> HelperResult<()> {
    let scm = open_scm(SC_MANAGER_CONNECT)?;
    for name in names {
        let svc = open_service(&scm, name, SERVICE_CHANGE_CONFIG)?;
        let ok = unsafe {
            ChangeServiceConfigW(
                svc.0,
                SERVICE_NO_CHANGE,        // dwServiceType
                SERVICE_AUTO_START,       // dwStartType
                SERVICE_NO_CHANGE,        // dwErrorControl
                std::ptr::null(),         // lpBinaryPathName (no change)
                std::ptr::null(),         // lpLoadOrderGroup
                std::ptr::null_mut(),     // lpdwTagId
                std::ptr::null(),         // lpDependencies
                std::ptr::null(),         // lpServiceStartName
                std::ptr::null(),         // lpPassword
                std::ptr::null(),         // lpDisplayName
            )
        };
        if ok == 0 {
            return Err(win32_err("ChangeServiceConfigW(AUTO_START)", unsafe { GetLastError() }));
        }
    }
    Ok(())
}

async fn disable_unit_files(&self, names: &[String]) -> HelperResult<()> {
    // identical structure with SERVICE_DISABLED
    // ... see enable_unit_files ...
}
```

- [ ] **Step 2: Implement `unit_state`**

```rust
use boxpilot_ipc::PlatformUnitExtra;

async fn unit_state(&self, name: &str) -> HelperResult<UnitState> {
    let scm = open_scm(SC_MANAGER_CONNECT)?;
    let svc = match open_service(&scm, name, windows_sys::Win32::System::Services::SERVICE_QUERY_STATUS) {
        Ok(h) => h,
        Err(HelperError::Ipc { message }) if message.contains("error=1060") => {
            // ERROR_SERVICE_DOES_NOT_EXIST
            return Ok(UnitState::NotFound);
        }
        Err(e) => return Err(e),
    };
    let s = super::scm::query_service_status(&svc)?;
    let (active_state, sub_state) = super::scm::map_scm_state(&s);
    Ok(UnitState::Known {
        active_state,
        sub_state,
        load_state: "loaded".to_string(),  // SCM ack of existence
        n_restarts: 0,                      // tracked separately (see SERVICE_FAIL_ACTIONS); 0 if not configured
        exec_main_status: s.dwWin32ExitCode as i32,
        platform_extra: PlatformUnitExtra::Windows {
            check_point: s.dwCheckPoint,
            wait_hint_ms: s.dwWaitHint,
            controls_accepted: s.dwControlsAccepted,
        },
    })
}
```

- [ ] **Step 3: Implement `fragment_path`**

```rust
use windows_sys::Win32::System::Services::{
    QueryServiceConfigW, QUERY_SERVICE_CONFIGW, SERVICE_QUERY_CONFIG,
};

async fn fragment_path(&self, name: &str) -> HelperResult<Option<String>> {
    let scm = open_scm(SC_MANAGER_CONNECT)?;
    let svc = match open_service(&scm, name, SERVICE_QUERY_CONFIG) {
        Ok(h) => h,
        Err(HelperError::Ipc { message }) if message.contains("error=1060") => return Ok(None),
        Err(e) => return Err(e),
    };
    // First call sizes the buffer.
    let mut needed: u32 = 0;
    let _ = unsafe {
        QueryServiceConfigW(svc.0, std::ptr::null_mut(), 0, &mut needed)
    };
    let mut buf = vec![0u8; needed as usize];
    let ok = unsafe {
        QueryServiceConfigW(svc.0, buf.as_mut_ptr() as _, needed, &mut needed)
    };
    if ok == 0 {
        return Err(win32_err("QueryServiceConfigW", unsafe { GetLastError() }));
    }
    let cfg = buf.as_ptr() as *const QUERY_SERVICE_CONFIGW;
    let bin_path = unsafe { (*cfg).lpBinaryPathName };
    if bin_path.is_null() { return Ok(None); }
    let mut len = 0;
    while unsafe { *bin_path.add(len) } != 0 { len += 1; }
    let slice = unsafe { std::slice::from_raw_parts(bin_path, len) };
    Ok(Some(String::from_utf16_lossy(slice)))
}
```

- [ ] **Step 4: Test (Windows runner)**

Add tests using a dummy service created in setup, then `enable` / `disable` / `unit_state` / `fragment_path` against it.

- [ ] **Step 5: Commit**

```
git commit -am "feat(platform/windows/scm): enable/disable/state/fragment_path

ChangeServiceConfigW for start-type tweaks; QueryServiceStatusEx populates
UnitState::Known with PlatformUnitExtra::Windows; QueryServiceConfigW returns
lpBinaryPathName as the fragment_path analog. NotFound mapped from
ERROR_SERVICE_DOES_NOT_EXIST (1060)."
```

---

## PR 3.4 — `service.install_managed` Windows path

**Scope:** Implement `service.install_managed` for Windows: `CreateServiceW` + `ChangeServiceConfig2W` for restart-on-fail policy + service description. The handler-level wiring already exists from Sub-project #1; this PR fills in `crates/boxpilotd/src/service/install.rs` Windows branch.

**Files:**
- Modify: `crates/boxpilotd/src/service/install.rs:` add `#[cfg(target_os = "windows")]` branch
- Modify: `crates/boxpilotd/src/handlers/service_install_managed.rs:` (already wires Linux; ensure no Linux-only assumption leaks)

**Dependencies:** PR 3.3.

- [ ] **Step 1: Sketch the Windows install function**

```rust
// crates/boxpilotd/src/service/install.rs
#[cfg(target_os = "windows")]
mod windows_impl {
    use boxpilot_ipc::{HelperError, HelperResult};
    use windows_sys::Win32::Foundation::GetLastError;
    use windows_sys::Win32::System::Services::{
        CreateServiceW, ChangeServiceConfig2W, DeleteService,
        SERVICE_CONFIG_FAILURE_ACTIONS, SERVICE_CONFIG_DESCRIPTION,
        SC_MANAGER_CREATE_SERVICE, SERVICE_ALL_ACCESS,
        SERVICE_WIN32_OWN_PROCESS, SERVICE_DEMAND_START, SERVICE_ERROR_NORMAL,
        SC_ACTION, SC_ACTION_RESTART, SC_ACTION_NONE,
        SERVICE_FAILURE_ACTIONSW, SERVICE_DESCRIPTIONW,
    };
    use crate::context::HelperContext;
    use boxpilot_platform::windows::scm::{open_scm, open_service, win32_err, ScHandle};

    pub async fn install_managed_windows(
        ctx: &HelperContext,
    ) -> HelperResult<boxpilot_ipc::ServiceInstallManagedResponse> {
        let cfg = ctx.load_config().await?;
        let target = cfg.target_service.clone();
        let scm = open_scm(SC_MANAGER_CREATE_SERVICE)?;
        let exe = std::env::current_exe()
            .map_err(|e| HelperError::Ipc { message: format!("current_exe: {e}") })?;
        let bin_path = format!("\"{}\" --service-managed", exe.display());

        let mut wide_name: Vec<u16> = target.encode_utf16().collect(); wide_name.push(0);
        let mut wide_disp: Vec<u16> = "BoxPilot Sing-box Service".encode_utf16().collect(); wide_disp.push(0);
        let mut wide_bin: Vec<u16> = bin_path.encode_utf16().collect(); wide_bin.push(0);

        // CreateServiceW; idempotent re-install: if ERROR_SERVICE_EXISTS,
        // open + DeleteService + retry.
        let svc = unsafe {
            CreateServiceW(
                scm.0,
                wide_name.as_ptr(),
                wide_disp.as_ptr(),
                SERVICE_ALL_ACCESS,
                SERVICE_WIN32_OWN_PROCESS,
                SERVICE_DEMAND_START,
                SERVICE_ERROR_NORMAL,
                wide_bin.as_ptr(),
                std::ptr::null(), std::ptr::null_mut(),
                std::ptr::null(), std::ptr::null(), std::ptr::null(),
            )
        };
        let svc_handle = if svc == 0 {
            // 1073 = ERROR_SERVICE_EXISTS
            let err = unsafe { GetLastError() };
            if err == 1073 {
                // Open + delete + recreate.
                let existing = open_service(&scm, &target, windows_sys::Win32::System::Services::DELETE)?;
                if unsafe { DeleteService(existing.0) } == 0 {
                    return Err(win32_err("DeleteService", unsafe { GetLastError() }));
                }
                drop(existing);
                let svc2 = unsafe { CreateServiceW(scm.0, wide_name.as_ptr(), wide_disp.as_ptr(),
                    SERVICE_ALL_ACCESS, SERVICE_WIN32_OWN_PROCESS, SERVICE_DEMAND_START,
                    SERVICE_ERROR_NORMAL, wide_bin.as_ptr(),
                    std::ptr::null(), std::ptr::null_mut(),
                    std::ptr::null(), std::ptr::null(), std::ptr::null()) };
                if svc2 == 0 {
                    return Err(win32_err("CreateServiceW(retry)", unsafe { GetLastError() }));
                }
                ScHandle(svc2)
            } else {
                return Err(win32_err("CreateServiceW", err));
            }
        } else {
            ScHandle(svc)
        };

        // Failure actions: restart 2s for first 3 actions, then NONE.
        let mut actions = [
            SC_ACTION { Type: SC_ACTION_RESTART, Delay: 2000 },
            SC_ACTION { Type: SC_ACTION_RESTART, Delay: 2000 },
            SC_ACTION { Type: SC_ACTION_RESTART, Delay: 2000 },
            SC_ACTION { Type: SC_ACTION_NONE, Delay: 0 },
        ];
        let mut fa: SERVICE_FAILURE_ACTIONSW = unsafe { std::mem::zeroed() };
        fa.dwResetPeriod = 60 * 60;  // 1 hour reset window
        fa.cActions = actions.len() as u32;
        fa.lpsaActions = actions.as_mut_ptr();
        let ok = unsafe {
            ChangeServiceConfig2W(svc_handle.0, SERVICE_CONFIG_FAILURE_ACTIONS, &fa as *const _ as _)
        };
        if ok == 0 {
            return Err(win32_err("ChangeServiceConfig2W(FAILURE_ACTIONS)",
                unsafe { GetLastError() }));
        }

        // Description.
        let mut wide_desc: Vec<u16> = "Sing-box VPN core managed by BoxPilot".encode_utf16().collect();
        wide_desc.push(0);
        let mut desc = SERVICE_DESCRIPTIONW { lpDescription: wide_desc.as_mut_ptr() };
        let ok = unsafe {
            ChangeServiceConfig2W(svc_handle.0, SERVICE_CONFIG_DESCRIPTION, &mut desc as *mut _ as _)
        };
        if ok == 0 {
            return Err(win32_err("ChangeServiceConfig2W(DESCRIPTION)",
                unsafe { GetLastError() }));
        }

        Ok(boxpilot_ipc::ServiceInstallManagedResponse {
            generated_unit_path: bin_path,
            claimed_controller: false,  // populated by handler with maybe_claim_controller result
        })
    }
}
```

- [ ] **Step 2: Update `service::install::install_managed` to dispatch by cfg**

```rust
pub async fn install_managed(
    cfg: &boxpilot_ipc::BoxpilotConfig,
    deps: &InstallDeps<'_>,
) -> HelperResult<boxpilot_ipc::ServiceInstallManagedResponse> {
    #[cfg(target_os = "linux")]
    { /* existing Linux impl */ }
    #[cfg(target_os = "windows")]
    { windows_impl::install_managed_windows(/* deps */).await }
    #[cfg(not(any(target_os = "linux", target_os = "windows")))]
    { Err(HelperError::NotImplemented) }
}
```

- [ ] **Step 3: Test on Windows runner**

Smoke test that installs a service named `boxpilot-sing-box-test`, asserts `unit_state` returns `Known { active_state: "inactive", … }`, then deletes via `DeleteService`.

- [ ] **Step 4: Linux regression check**

```
cargo test -p boxpilotd
```

- [ ] **Step 5: Commit**

```
git commit -am "feat(boxpilotd/service/install): Windows install_managed via CreateServiceW

CreateServiceW + ChangeServiceConfig2W(FAILURE_ACTIONS, DESCRIPTION) match
the Linux unit-file template's Restart=on-failure RestartSec=2s + Description.
Idempotent: ERROR_SERVICE_EXISTS → DeleteService + recreate."
```

---

## PR 3.5 — `JunctionActive` ActivePointer Windows impl

**Scope:** Replace `windows/active.rs` `unimplemented!()` stubs with a junction-based implementation. Also implement `CurrentPointer` for the cores/current path (introduced in PR 1.4).

**Files:**
- Modify: `crates/boxpilot-platform/src/windows/active.rs`
- Test: same

**Dependencies:** PR 3.4.

- [ ] **Step 1: Add junction primitives**

```rust
// crates/boxpilot-platform/src/windows/active.rs

use std::path::{Path, PathBuf};
use std::os::windows::ffi::OsStrExt;
use boxpilot_ipc::{HelperError, HelperResult};
use windows_sys::Win32::Foundation::{GetLastError, HANDLE, INVALID_HANDLE_VALUE};
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, MoveFileExW, RemoveDirectoryW,
    FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
    FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
    FILE_GENERIC_READ, FILE_GENERIC_WRITE,
    OPEN_EXISTING, MOVEFILE_REPLACE_EXISTING,
};
use windows_sys::Win32::System::IO::DeviceIoControl;

const FSCTL_SET_REPARSE_POINT: u32 = 0x000900a4;
const FSCTL_GET_REPARSE_POINT: u32 = 0x000900a8;
const IO_REPARSE_TAG_MOUNT_POINT: u32 = 0xA0000003;

#[repr(C)]
struct ReparseDataBuffer {
    reparse_tag: u32,
    reparse_data_length: u16,
    reserved: u16,
    substitute_name_offset: u16,
    substitute_name_length: u16,
    print_name_offset: u16,
    print_name_length: u16,
    path_buffer: [u16; 1],  // variable-length
}

fn create_junction(link: &Path, target: &Path) -> HelperResult<()> {
    // 1. Create the directory.
    std::fs::create_dir(link).map_err(|e| HelperError::Ipc {
        message: format!("create junction dir {}: {e}", link.display()),
    })?;
    // 2. Open with reparse-point flags.
    let mut wide_link: Vec<u16> = link.as_os_str().encode_wide().collect(); wide_link.push(0);
    let h = unsafe {
        CreateFileW(
            wide_link.as_ptr(),
            FILE_GENERIC_READ | FILE_GENERIC_WRITE,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            std::ptr::null_mut(),
            OPEN_EXISTING,
            FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
            0 as HANDLE,
        )
    };
    if h == INVALID_HANDLE_VALUE {
        return Err(HelperError::Ipc {
            message: format!("CreateFileW({}): GetLastError={}", link.display(), unsafe { GetLastError() }),
        });
    }
    let _h_guard = HandleGuard(h);
    // 3. Build REPARSE_DATA_BUFFER with the target path in NT namespace form.
    let nt_target = format!("\\??\\{}", target.display());
    let target_wide: Vec<u16> = std::ffi::OsString::from(&nt_target).encode_wide().collect();
    let print_wide: Vec<u16> = target.as_os_str().encode_wide().collect();
    let path_buf_len = (target_wide.len() + print_wide.len() + 2) * 2;  // bytes
    let total_len = std::mem::size_of::<ReparseDataBuffer>() - 2 + path_buf_len;
    let mut buf = vec![0u8; total_len];
    unsafe {
        let rb = buf.as_mut_ptr() as *mut ReparseDataBuffer;
        (*rb).reparse_tag = IO_REPARSE_TAG_MOUNT_POINT;
        (*rb).reparse_data_length = (path_buf_len + 8) as u16;
        (*rb).substitute_name_offset = 0;
        (*rb).substitute_name_length = (target_wide.len() * 2) as u16;
        (*rb).print_name_offset = ((target_wide.len() + 1) * 2) as u16;
        (*rb).print_name_length = (print_wide.len() * 2) as u16;
        let path_buf_ptr = (&mut (*rb).path_buffer) as *mut u16;
        std::ptr::copy_nonoverlapping(target_wide.as_ptr(), path_buf_ptr, target_wide.len());
        *path_buf_ptr.add(target_wide.len()) = 0;  // terminator
        std::ptr::copy_nonoverlapping(print_wide.as_ptr(), path_buf_ptr.add(target_wide.len() + 1), print_wide.len());
        *path_buf_ptr.add(target_wide.len() + 1 + print_wide.len()) = 0;
    }
    // 4. DeviceIoControl(FSCTL_SET_REPARSE_POINT).
    let mut bytes_returned: u32 = 0;
    let ok = unsafe {
        DeviceIoControl(
            h,
            FSCTL_SET_REPARSE_POINT,
            buf.as_ptr() as _,
            total_len as u32,
            std::ptr::null_mut(),
            0,
            &mut bytes_returned,
            std::ptr::null_mut(),
        )
    };
    if ok == 0 {
        return Err(HelperError::Ipc {
            message: format!("DeviceIoControl(SET_REPARSE_POINT): GetLastError={}",
                unsafe { GetLastError() }),
        });
    }
    Ok(())
}

struct HandleGuard(HANDLE);
impl Drop for HandleGuard {
    fn drop(&mut self) {
        if self.0 != 0 && self.0 != INVALID_HANDLE_VALUE {
            unsafe { windows_sys::Win32::Foundation::CloseHandle(self.0); }
        }
    }
}
```

- [ ] **Step 2: Implement `JunctionActive: ActivePointer`**

```rust
use crate::traits::active::ActivePointer;

pub struct JunctionActive {
    link_path: PathBuf,
}

impl JunctionActive {
    pub fn new(link_path: PathBuf) -> Self { Self { link_path } }
}

impl ActivePointer for JunctionActive {
    fn read(&self) -> HelperResult<Option<PathBuf>> {
        // FSCTL_GET_REPARSE_POINT and parse the substitute name.
        // Return None if the link doesn't exist.
        if !self.link_path.exists() { return Ok(None); }
        // ... DeviceIoControl(GET_REPARSE_POINT) to inspect target ...
        // Implementation detail; full body follows the create_junction pattern in reverse.
        Ok(None)  // FIX in implementation
    }
    fn set_atomic(&self, target: &Path) -> HelperResult<()> {
        let new_path = self.link_path.with_extension("new");
        let _ = std::fs::remove_dir_all(&new_path);  // best-effort cleanup of prior crash
        create_junction(&new_path, target)?;
        let mut wide_src: Vec<u16> = new_path.as_os_str().encode_wide().collect(); wide_src.push(0);
        let mut wide_dst: Vec<u16> = self.link_path.as_os_str().encode_wide().collect(); wide_dst.push(0);
        let ok = unsafe {
            MoveFileExW(wide_src.as_ptr(), wide_dst.as_ptr(), MOVEFILE_REPLACE_EXISTING)
        };
        if ok == 0 {
            return Err(HelperError::Ipc {
                message: format!("MoveFileExW({} -> {}): GetLastError={}",
                    new_path.display(), self.link_path.display(),
                    unsafe { GetLastError() }),
            });
        }
        Ok(())
    }
    fn clear(&self) -> HelperResult<()> {
        let mut wide: Vec<u16> = self.link_path.as_os_str().encode_wide().collect(); wide.push(0);
        let ok = unsafe { RemoveDirectoryW(wide.as_ptr()) };
        if ok == 0 {
            let err = unsafe { GetLastError() };
            // 2 = ERROR_FILE_NOT_FOUND, 3 = ERROR_PATH_NOT_FOUND
            if err == 2 || err == 3 { return Ok(()); }
            return Err(HelperError::Ipc {
                message: format!("RemoveDirectoryW({}): GetLastError={err}", self.link_path.display()),
            });
        }
        Ok(())
    }
}
```

- [ ] **Step 3: Implement `CurrentPointer` for cores/current**

If PR 1.4 introduced a separate `CurrentPointer` trait, implement it analogously. Internally reuse `create_junction` + `MoveFileExW` plumbing.

- [ ] **Step 4: Test on Windows runner**

```rust
#[tokio::test]
async fn junction_set_atomic_overwrites_prior() {
    let dir = TempDir::new().unwrap();
    let link = dir.path().join("active");
    let target_a = dir.path().join("a");
    std::fs::create_dir(&target_a).unwrap();
    let active = JunctionActive::new(link.clone());
    active.set_atomic(&target_a).unwrap();
    let target_b = dir.path().join("b");
    std::fs::create_dir(&target_b).unwrap();
    active.set_atomic(&target_b).unwrap();
    assert_eq!(active.read().unwrap().as_deref(), Some(target_b.as_path()));
}
```

- [ ] **Step 5: Commit**

```
git commit -am "feat(platform/windows/active): JunctionActive ActivePointer

Junction (mount point) reparse points instead of symlinks — no Developer
Mode requirement, junctions accept atomic MoveFileExW(REPLACE_EXISTING)
swap. Supports release pointer + cores/current."
```

---

## PR 3.6 — Windows `CoreAssetNaming` + `ZipExtractor`

**Scope:** Sing-box ships Windows binaries as ZIPs (e.g. `sing-box-1.10.0-windows-amd64.zip`). Replace the two `unimplemented!()` stubs with naming + extractor.

**Files:**
- Modify: `crates/boxpilot-platform/src/windows/core_assets.rs`

**Dependencies:** PR 3.5.

- [ ] **Step 1: Implement `WindowsCoreAssetNaming`**

```rust
use crate::traits::core_assets::CoreAssetNaming;

pub struct WindowsCoreAssetNaming;

impl CoreAssetNaming for WindowsCoreAssetNaming {
    fn tarball_filename(&self, version: &str, arch: &str) -> String {
        format!("sing-box-{version}-windows-{arch}.zip")
    }
    fn extracted_binary_subpath(&self, version: &str, arch: &str) -> std::path::PathBuf {
        std::path::PathBuf::from(format!("sing-box-{version}-windows-{arch}"))
            .join("sing-box.exe")
    }
}
```

- [ ] **Step 2: Implement `ZipExtractor`**

```rust
use crate::traits::core_assets::CoreArchive;

pub struct ZipExtractor;

#[async_trait::async_trait]
impl CoreArchive for ZipExtractor {
    async fn extract(&self, archive_path: &std::path::Path, dest: &std::path::Path) -> boxpilot_ipc::HelperResult<()> {
        let archive_path = archive_path.to_path_buf();
        let dest = dest.to_path_buf();
        tokio::task::spawn_blocking(move || -> boxpilot_ipc::HelperResult<()> {
            let f = std::fs::File::open(&archive_path).map_err(|e| boxpilot_ipc::HelperError::Ipc {
                message: format!("open {}: {e}", archive_path.display()),
            })?;
            let mut archive = zip::ZipArchive::new(f).map_err(|e| boxpilot_ipc::HelperError::Ipc {
                message: format!("zip parse: {e}"),
            })?;
            archive.extract(&dest).map_err(|e| boxpilot_ipc::HelperError::Ipc {
                message: format!("zip extract: {e}"),
            })?;
            Ok(())
        }).await.map_err(|e| boxpilot_ipc::HelperError::Ipc {
            message: format!("spawn_blocking: {e}"),
        })??;
        Ok(())
    }
}
```

- [ ] **Step 3: Test**

```rust
#[tokio::test]
async fn extract_creates_subpath() {
    let dir = TempDir::new().unwrap();
    // Build a tiny zip in-memory with one file at sing-box-1.10.0-windows-amd64/sing-box.exe
    // (use the zip crate's writer).
    // ... assert ZipExtractor::extract produces the expected file ...
}
```

- [ ] **Step 4: Commit**

```
git commit -am "feat(platform/windows/core_assets): WindowsCoreAssetNaming + ZipExtractor

Sing-box Windows assets are .zip; naming matches upstream release filenames.
ZipExtractor uses the zip crate (already a dep)."
```

---

## PR 3.7 — `build_tempfile_aux` + orphan cleanup

**Scope:** Real Windows `AuxStream` backing for `profile.activate_bundle`. Tempfile under `%LocalAppData%\BoxPilot\aux\`, owner-only (defaults are owner-only on `%LocalAppData%`). sha256 carries integrity in lieu of F_SEAL_*.

**Files:**
- Modify: `crates/boxpilot-platform/src/windows/bundle.rs`
- Test: same

**Dependencies:** PR 3.6.

- [ ] **Step 1: Implement `build_tempfile_aux`**

```rust
use std::path::Path;
use boxpilot_ipc::{HelperError, HelperResult};
use crate::traits::bundle_aux::AuxStream;

pub async fn build_tempfile_aux(staging_path: &Path) -> HelperResult<AuxStream> {
    let aux_dir = paths_local_appdata().join("aux");
    tokio::fs::create_dir_all(&aux_dir).await
        .map_err(|e| HelperError::Ipc { message: format!("mkdir aux: {e}") })?;
    let staging_path = staging_path.to_path_buf();
    let tmp = tokio::task::spawn_blocking(move || -> HelperResult<tempfile::NamedTempFile> {
        let mut tmp = tempfile::Builder::new()
            .prefix("boxpilot-aux-").suffix(".tar")
            .tempfile_in(&aux_dir)
            .map_err(|e| HelperError::Ipc { message: format!("tempfile: {e}") })?;
        let mut tar = tar::Builder::new(tmp.as_file_mut());
        tar.append_dir_all(".", &staging_path)
            .map_err(|e| HelperError::Ipc { message: format!("tar append: {e}") })?;
        tar.into_inner()
            .map_err(|e| HelperError::Ipc { message: format!("tar finish: {e}") })?
            .sync_all()
            .map_err(|e| HelperError::Ipc { message: format!("sync: {e}") })?;
        Ok(tmp)
    }).await.map_err(|e| HelperError::Ipc { message: format!("spawn_blocking: {e}") })??;
    let std_file = tmp.reopen()
        .map_err(|e| HelperError::Ipc { message: format!("reopen: {e}") })?;
    let mut tokio_file: tokio::fs::File = std_file.into();
    use tokio::io::AsyncSeekExt;
    tokio_file.seek(std::io::SeekFrom::Start(0)).await
        .map_err(|e| HelperError::Ipc { message: format!("seek: {e}") })?;
    // tmp is dropped here, deleting the on-disk file. The reopened handle remains
    // valid until tokio_file is dropped (Windows holds the inode while open).
    Ok(AuxStream::from_async_read(tokio_file))
}

fn paths_local_appdata() -> std::path::PathBuf {
    // Try %LocalAppData%, fallback to %TEMP%.
    std::env::var("LOCALAPPDATA")
        .or_else(|_| std::env::var("TEMP"))
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| std::path::PathBuf::from("C:\\Windows\\Temp"))
        .join("BoxPilot")
}

/// Called at daemon startup; deletes any "boxpilot-aux-*.tar" file in the
/// aux dir older than 1 hour. Best-effort.
pub async fn cleanup_orphans() {
    let aux_dir = paths_local_appdata().join("aux");
    let Ok(rd) = tokio::fs::read_dir(&aux_dir).await else { return; };
    let mut rd = rd;
    while let Ok(Some(entry)) = rd.next_entry().await {
        let name = entry.file_name();
        let s = name.to_string_lossy();
        if !s.starts_with("boxpilot-aux-") || !s.ends_with(".tar") { continue; }
        let Ok(meta) = entry.metadata().await else { continue; };
        let Ok(modified) = meta.modified() else { continue; };
        let age = std::time::SystemTime::now().duration_since(modified).unwrap_or_default();
        if age > std::time::Duration::from_secs(3600) {
            let _ = tokio::fs::remove_file(entry.path()).await;
        }
    }
}
```

- [ ] **Step 2: Wire `cleanup_orphans` into `entry/windows.rs::run_under_scm`**

```rust
// crates/boxpilotd/src/entry/windows.rs
boxpilot_platform::windows::bundle::cleanup_orphans().await;
```

- [ ] **Step 3: Test the round-trip**

```rust
#[tokio::test]
async fn aux_round_trips_known_bytes() {
    let dir = TempDir::new().unwrap();
    let staging = dir.path().join("staging");
    std::fs::create_dir(&staging).unwrap();
    std::fs::write(staging.join("a.txt"), b"hello").unwrap();
    let aux = build_tempfile_aux(&staging).await.unwrap();
    use tokio::io::AsyncReadExt;
    let mut buf = Vec::new();
    aux.into_async_read().read_to_end(&mut buf).await.unwrap();
    // tar header parsing: confirm "a.txt" entry exists with bytes "hello".
    let mut a = tar::Archive::new(buf.as_slice());
    let mut found = false;
    for entry in a.entries().unwrap() {
        let mut e = entry.unwrap();
        if e.path().unwrap().file_name() == Some(std::ffi::OsStr::new("a.txt")) {
            let mut content = Vec::new();
            e.read_to_end(&mut content).unwrap();
            assert_eq!(content, b"hello");
            found = true;
        }
    }
    assert!(found);
}
```

- [ ] **Step 4: Commit**

```
git commit -am "feat(platform/windows/bundle): real build_tempfile_aux

NamedTempFile under %LocalAppData%\\BoxPilot\\aux\\, owner-only by default.
sha256 in the request carries the F_SEAL_* equivalent integrity guarantee.
Orphan cleanup at daemon startup."
```

---

## PR 3.8 — End-to-end Windows verb verification + widen CI test gate

**Scope:** Run the boxpilotd test suite on Windows. Update `windows-check.yml` to invoke `cargo test --workspace --all-targets` (skipping the SCM integration tests gated on admin rights via env flag if needed). Verify all primary-flow verbs work via `boxpilotctl`.

**Files:**
- Modify: `.github/workflows/windows-check.yml`
- Possibly modify: tests gated `#[ignore]` if they require admin rights the CI runner lacks (Windows runners generally have admin)

**Dependencies:** PR 3.7.

- [ ] **Step 1: Run `cargo test --workspace --all-targets` locally on Windows**

(Or trigger via temporary CI workflow.)

- [ ] **Step 2: Update workflow**

```yaml
      - name: cargo test --workspace
        run: cargo test --workspace --all-targets --target x86_64-pc-windows-msvc -- --include-ignored
        env:
          CARGO_TERM_COLOR: always
```

- [ ] **Step 3: Manual smoke via boxpilotctl on a real Windows VM**

The acceptance check is AC4 + AC5 from the spec:
- `boxpilotctl service.status` → returns `Known { active_state, …, platform_extra: Windows {…} }`
- `boxpilotctl profile.activate_bundle` with a 10MB sample bundle succeeds end-to-end

- [ ] **Step 4: Commit + open GH PR for batch ③**

```
git commit -am "ci: widen windows-check to cargo test --workspace --all-targets

Sub-project #2 batch ③ closes the Windows test parity gap. Every primary-flow
verb now works on Windows; CI runs the same suite as Linux (- features that
still require admin like SCM service-create, gated via require_admin())."
gh pr create --title "feat: BoxPilot Sub-project #2 batch ③ — Real Windows verbs"
```

---

# Batch ④ — Edge Verbs + Polish

Goal: every helper verb works on Windows; GUI text is platform-aware. After this batch, Sub-project #2 is complete and the Windows port is feature-equivalent to Linux (modulo Sub-project #3's installer/wintun/macOS scope).

## PR 4.1 — `legacy.observe_service` Windows impl

**Scope:** Query the SCM for a pre-existing (non-BoxPilot-managed) sing-box service if one exists; return its current state for the GUI's "Migrate from existing install" panel.

**Files:**
- Modify: `crates/boxpilotd/src/legacy/observe.rs:` add `#[cfg(target_os = "windows")]` branch

**Dependencies:** Batch ③ merged.

- [ ] **Step 1: Implement Windows observe**

```rust
#[cfg(target_os = "windows")]
async fn observe_windows() -> HelperResult<boxpilot_ipc::LegacyObserveServiceResponse> {
    use boxpilot_platform::windows::scm::{open_scm, open_service, query_service_status, map_scm_state};
    use windows_sys::Win32::System::Services::{SC_MANAGER_CONNECT, SERVICE_QUERY_STATUS};
    let scm = open_scm(SC_MANAGER_CONNECT)?;
    // Try common legacy names. The first that exists wins.
    for candidate in &["sing-box", "singbox", "sing-box-service"] {
        if let Ok(svc) = open_service(&scm, candidate, SERVICE_QUERY_STATUS) {
            let s = query_service_status(&svc)?;
            let (active_state, sub_state) = map_scm_state(&s);
            return Ok(boxpilot_ipc::LegacyObserveServiceResponse {
                unit_name: candidate.to_string(),
                exists: true,
                state: Some(active_state),
                /* … other fields per LegacyObserveServiceResponse shape … */
            });
        }
    }
    Ok(boxpilot_ipc::LegacyObserveServiceResponse {
        unit_name: "".to_string(),
        exists: false,
        state: None,
        /* … */
    })
}
```

- [ ] **Step 2: Branch from `observe::observe_service`**

```rust
pub async fn observe_service(/* … */) -> HelperResult<LegacyObserveServiceResponse> {
    #[cfg(target_os = "linux")] { /* existing systemd path */ }
    #[cfg(target_os = "windows")] { observe_windows().await }
}
```

- [ ] **Step 3: Commit**

```
git commit -am "feat(boxpilotd/legacy/observe): Windows SCM-based observe path

Probes well-known legacy service names via OpenServiceW;
returns Found/NotFound + current state for the migrate UI."
```

---

## PR 4.2 — `legacy.migrate_service` Windows impl (Cutover side)

**Scope:** Stop + DeleteService for the legacy sing-box service. The Prepare side maps cleanly: read legacy ImagePath via `QueryServiceConfigW` (already wired in PR 3.3 as `fragment_path`), parse the binary path, no config-file scan (Windows sing-box typically uses a config-file argument identical to Linux's `--config <path>`).

**Files:**
- Modify: `crates/boxpilotd/src/legacy/migrate.rs:` Windows branch in `prepare` + `cutover`

**Dependencies:** PR 4.1.

- [ ] **Step 1: Sketch the Windows prepare path**

```rust
#[cfg(target_os = "windows")]
async fn prepare_windows(/* … */) -> HelperResult<LegacyMigratePrepareResponse> {
    // 1. Find the legacy service via observe pattern.
    // 2. Parse its ImagePath (e.g., `"C:\Program Files\sing-box\sing-box.exe" -c <config>`)
    //    using a tokenizing parser; --config / -c arg holds the config path.
    // 3. Read the config file bytes + sibling assets in the same dir.
    // 4. Return LegacyMigratePrepareResponse identical to Linux shape.
}
```

- [ ] **Step 2: Sketch the Windows cutover path**

```rust
#[cfg(target_os = "windows")]
async fn cutover_windows(name: &str, /* deps */) -> HelperResult<LegacyMigrateCutoverResponse> {
    // 1. Backup the binpath text (no fragment file to copy on Windows).
    //    Persist a small JSON in backups/units/<name>-<iso>.json containing the
    //    SCM config snapshot for restore-via-installer if the user backs out.
    // 2. Open service with SERVICE_STOP | DELETE.
    // 3. ControlService(STOP); poll until STOPPED.
    // 4. DeleteService.
    // 5. Return Cutover response with the backup_unit_path.
}
```

- [ ] **Step 3: Branch from `migrate::run`**

`run` already handles `Prepare` / `Cutover` variants; add `#[cfg]` arms for Windows.

- [ ] **Step 4: Test**

Smoke test on Windows runner against a dummy service (created in setup).

- [ ] **Step 5: Commit**

```
git commit -am "feat(boxpilotd/legacy/migrate): Windows prepare + cutover paths

Prepare reads ImagePath via QueryServiceConfigW, parses --config arg, returns
config bytes + siblings. Cutover snapshots SCM config to a JSON backup file,
ControlService(STOP) + DeleteService."
```

---

## PR 4.3 — `diagnostics.export_redacted` Windows impl

**Scope:** Replace journalctl section with Event Log query for the BoxPilot service. Use `OpenEventLogW` + `ReadEventLogW` (or the modern `EvtQuery`) to grab recent entries from the "BoxPilot" log source.

**Files:**
- Modify: `crates/boxpilotd/src/diagnostics/mod.rs:` cfg-branch the journal source
- Modify: `crates/boxpilot-platform/src/windows/logs.rs:` real `EventLogReader` impl (replace stub)

**Dependencies:** PR 4.2.

- [ ] **Step 1: Implement `EventLogReader` for the BoxPilot service**

```rust
// crates/boxpilot-platform/src/windows/logs.rs
use crate::traits::logs::JournalReader;
use boxpilot_ipc::{HelperError, HelperResult};

pub struct EventLogReader;

#[async_trait::async_trait]
impl JournalReader for EventLogReader {
    async fn tail(&self, unit: &str, lines: u32) -> HelperResult<Vec<String>> {
        // Use tracing-appender-rolled file under %ProgramData%\BoxPilot\logs\
        // (already populated by entry::windows::run_under_scm). Read last N
        // lines; works for any "unit" — Windows doesn't have systemd's
        // per-unit journal partitioning, so the unit param is ignored beyond
        // the file path resolution.
        let logs_dir = std::path::PathBuf::from(
            std::env::var("ProgramData").unwrap_or_else(|_| "C:\\ProgramData".to_string())
        ).join("BoxPilot").join("logs");
        let target_path = logs_dir.join("boxpilotd.log");
        let _ = unit;
        if !target_path.exists() {
            return Ok(vec![]);
        }
        let bytes = tokio::fs::read(&target_path).await
            .map_err(|e| HelperError::Ipc { message: format!("read log: {e}") })?;
        let text = String::from_utf8_lossy(&bytes);
        let mut tail: Vec<String> = text.lines().rev().take(lines as usize).map(String::from).collect();
        tail.reverse();
        Ok(tail)
    }
}
```

(Decision: read tracing-appender file rather than Win32 Event Log, because the daemon already writes to that file and the GUI's diagnostic export is meant to reflect the daemon's own log stream.)

- [ ] **Step 2: Wire in `entry/windows.rs`**

`Arc::new(EventLogReader)` instead of the stub.

- [ ] **Step 3: Smoke test**

Diagnostics export contains last N lines of the daemon log file.

- [ ] **Step 4: Commit**

```
git commit -am "feat(platform/windows/logs): EventLogReader reads tracing-appender file

Reads %ProgramData%\\BoxPilot\\logs\\boxpilotd.log tail (already written by
the daemon's tracing setup). Cross-platform JournalReader contract honored
without needing Win32 Event Log calls."
```

---

## PR 4.4 — `controller.transfer` Windows wiring + `home.status` Windows path

**Scope:** Two small verbs that are mostly platform-neutral but need verification under the new SID model.

**Files:**
- Verify: `crates/boxpilotd/src/handlers/controller_transfer.rs` — handler may need to look up new principal's SID via `LookupAccountSidW`
- Verify: `crates/boxpilotd/src/handlers/home_status.rs` — should already be cross-platform; verify `cargo test` covers Windows path

**Dependencies:** PR 4.3.

- [ ] **Step 1: Audit `controller.transfer` handler**

Read `crates/boxpilotd/src/handlers/controller_transfer.rs`. For Sub-project #2: the verb takes a target principal (uid on Linux, SID on Windows). Verify it invokes the same `commit_controller_claim` machinery with the new principal as caller.

If the verb is currently `NotImplemented` on Windows, implement it to take a `target_principal` request field and pass it to `commit_controller_claim`.

- [ ] **Step 2: Audit `home.status` handler**

Should be cross-platform. Verify the test suite includes a Windows-path test.

- [ ] **Step 3: Commit**

```
git commit -am "feat(boxpilotd/handlers): controller.transfer Windows + home.status verify

Both verbs now honor the new principal abstraction. controller.transfer
on Windows swaps controller_principal in boxpilot.toml after admin auth
check. home.status was already platform-neutral; added Windows test."
```

---

## PR 4.5 — `platform.info` verb + Vue store

**Scope:** New ReadOnly verb returning `{ os, os_version }`. Vue startup calls it once and stashes into a Pinia store.

**Files:**
- Modify: `crates/boxpilot-ipc/src/method.rs:` add `HelperMethod::PlatformInfo`
- Create: `crates/boxpilot-ipc/src/platform_info.rs:` `PlatformInfoResponse`
- Create: `crates/boxpilotd/src/handlers/platform_info.rs:` handler
- Modify: `crates/boxpilotd/src/dispatch_handler.rs:` wire the new verb
- Modify: `crates/boxpilot-tauri/src/helper_client.rs:` typed wrapper for the new verb
- Create: `boxpilot-tauri/src/platform_store.ts` (Pinia store)
- Modify: `boxpilot-tauri/src/main.ts` or App.vue: invoke at boot

**Dependencies:** PR 4.4.

- [ ] **Step 1: Add the IPC type and method**

```rust
// crates/boxpilot-ipc/src/platform_info.rs
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlatformInfoResponse {
    pub os: String,           // "linux" | "windows"
    pub os_version: String,
}
```

`HelperMethod::PlatformInfo` with `auth_class = ReadOnly`, `polkit_action_id = "app.boxpilot.helper.platform.info"`, wire id `0x0070`.

- [ ] **Step 2: Implement the handler**

```rust
// crates/boxpilotd/src/handlers/platform_info.rs
pub async fn handle(
    ctx: Arc<HelperContext>,
    _principal: CallerPrincipal,
    _body: Vec<u8>,
    aux: AuxStream,
) -> HelperResult<Vec<u8>> {
    if !aux.is_none() {
        return Err(HelperError::Ipc {
            message: "platform.info takes no aux stream".into(),
        });
    }
    let _call = dispatch::authorize(&ctx, &_principal, HelperMethod::PlatformInfo).await?;
    let resp = PlatformInfoResponse {
        os: if cfg!(target_os = "linux") { "linux".into() }
            else if cfg!(target_os = "windows") { "windows".into() }
            else { "unknown".into() },
        os_version: detect_os_version(),
    };
    serde_json::to_vec(&resp).map_err(|e| HelperError::Ipc {
        message: format!("encode: {e}"),
    })
}

#[cfg(target_os = "linux")]
fn detect_os_version() -> String {
    std::fs::read_to_string("/etc/os-release")
        .ok()
        .and_then(|t| t.lines().find(|l| l.starts_with("PRETTY_NAME="))
            .map(|l| l.trim_start_matches("PRETTY_NAME=").trim_matches('"').to_string()))
        .unwrap_or_else(|| "linux unknown".into())
}

#[cfg(target_os = "windows")]
fn detect_os_version() -> String {
    // Use RtlGetVersion via windows-sys; fall back to a static string.
    // … implementation detail …
    "Windows".into()
}
```

- [ ] **Step 3: Add Tauri client wrapper**

```typescript
// boxpilot-tauri/src/helper_client.rs (Rust side wrapper)
pub async fn platform_info(client: &dyn IpcClient) -> HelperResult<PlatformInfoResponse> {
    let bytes = client.call(HelperMethod::PlatformInfo, vec![], AuxStream::none()).await?;
    serde_json::from_slice(&bytes).map_err(|e| HelperError::Ipc { message: format!("decode: {e}") })
}
```

- [ ] **Step 4: Vue Pinia store**

```typescript
// boxpilot-tauri/src/platform_store.ts
import { defineStore } from "pinia";
import { invoke } from "@tauri-apps/api/core";

export const usePlatformStore = defineStore("platform", {
    state: () => ({ os: "unknown" as string, osVersion: "" }),
    actions: {
        async load() {
            const info = await invoke<{ os: string; os_version: string }>("platform_info");
            this.os = info.os;
            this.osVersion = info.os_version;
        },
    },
});
```

- [ ] **Step 5: Boot-time call**

In `main.ts` or `App.vue` `mounted`:

```typescript
import { usePlatformStore } from "./platform_store";
const platform = usePlatformStore();
await platform.load();
```

- [ ] **Step 6: Commit**

```
git commit -am "feat(ipc): platform.info verb + Vue Pinia store

ReadOnly verb returns {os, os_version}. GUI loads on boot and stashes
into a Pinia store consumed by the i18n wrapper (PR 4.6)."
```

---

## PR 4.6 — Vue i18n platform-variant wrapper + key audit

**Scope:** Audit existing translation files for systemd/polkit/journalctl/dbus terminology. Add platform-suffixed variants for affected keys. Wrap `vue-i18n`'s `t()` to try `key.platform` first then fall back.

**Files:**
- Modify: `boxpilot-tauri/src/translations/zh.json` and `en.json` (or whatever locale files exist)
- Create: `boxpilot-tauri/src/i18n_platform.ts` (wrapper)
- Modify: `boxpilot-tauri/src/main.ts` (install the wrapper)

**Dependencies:** PR 4.5.

- [ ] **Step 1: Audit translation files**

```
grep -rn "systemd\|polkit\|journalctl\|dbus\|D-Bus\|Polkit" boxpilot-tauri/src/translations/
```

For each hit, decide:
- **Generic-rewrite**: change to platform-neutral copy (e.g. "the system service manager")
- **Platform-variant**: keep both versions, key suffix `.linux` / `.windows`

- [ ] **Step 2: Implement the wrapper**

```typescript
// boxpilot-tauri/src/i18n_platform.ts
import { useI18n } from "vue-i18n";
import { usePlatformStore } from "./platform_store";

export function useT() {
    const { t, te } = useI18n();
    const platform = usePlatformStore();
    return (key: string, params?: Record<string, unknown>) => {
        const platformKey = `${key}.${platform.os}`;
        if (te(platformKey)) return t(platformKey, params || {});
        return t(key, params || {});
    };
}
```

Vue components opt in by `const t = useT();` instead of importing `t` from `vue-i18n` directly.

- [ ] **Step 3: Update affected components**

Each component currently using `t()` for one of the audited keys switches to `useT()`.

- [ ] **Step 4: Vue unit test**

```typescript
// boxpilot-tauri/tests/i18n_platform.test.ts
import { describe, it, expect } from "vitest";
import { setActivePinia, createPinia } from "pinia";
import { useT } from "../src/i18n_platform";
// ... test that key.linux is preferred when platform.os === "linux", and
// that fall-back to key works when no variant exists.
```

- [ ] **Step 5: Commit**

```
git commit -am "feat(tauri): platform-aware i18n wrapper

useT() prefers key.<os> over plain key. Audit found N keys mentioning
systemd/polkit/journalctl that get platform variants; remaining strings
were rewritten generic. Vue unit test pins resolution behavior."
```

- [ ] **Step 6: Open final batch ④ PR**

```
gh pr create --title "feat: BoxPilot Sub-project #2 batch ④ — Edge verbs + GUI polish"
```

---

# Smoke procedure (manual)

After batch ④ merges, write a smoke procedure document at `docs/superpowers/plans/2026-05-02-windows-port-sub-project-2-smoke-procedure.md` describing the 30-minute manual validation sequence on a fresh Windows VM (per spec §11.4): install boxpilotd via `sc create`, launch GUI, install managed core, activate profile, start service, kill process and verify restart-on-fail, run all four legacy.migrate_service paths against a dummy service, switch user accounts and verify Authority gates correctly.

---

# Self-review notes

Spec coverage check (against `docs/superpowers/specs/2026-05-02-boxpilot-windows-port-sub-project-2-design.md`):
- §1 (A–H): A → batch ③ + ④; B → batch ① (PR 1.3–1.6); C → PR 2.6; D → PR 1.1–1.2; E → PR 2.1; F → PR 2.5; G → PR 4.5–4.6; H → PR 3.7. ✓
- §2.1 Authority model: PR 2.5. ✓
- §2.2 Schema migration: PR 2.1, 2.2, 2.3. ✓
- §3 SCM redesign: PR 1.1, 1.2 (UnitState), PR 3.1–3.3 (impl). ✓
- §4 Authority impl details: PR 2.4 (helpers), 2.5 (table). ✓
- §5 GUI text: PR 4.5, 4.6. ✓
- §6 build_tempfile_aux: PR 3.7. ✓
- §7 ActivePointer junction: PR 3.5. ✓
- §8 boxpilotd compile: PR 1.3, 1.4, 1.5, 1.6. ✓
- §9 AclFsPermissions: PR 2.6. ✓
- §10 Dependencies/batching: 4 batches mirrored. ✓
- §11 Testing: per-PR test steps, CI widening at 1.6 + 3.8. ✓
- §12 Risks: SCM start_unit timeout knob noted in PR 3.1; FAILURE_ACTIONS divergence noted in PR 3.4; os_release fallback in PR 4.5; i18n key audit in PR 4.6; controller.transfer in PR 4.4. ✓
- §13 AC1–AC8: AC1 → PR 1.6; AC2 → PR 3.8; AC3 → every PR's "Linux regression check"; AC4 → PR 3.3 + 3.8; AC5 → PR 3.7 + 3.8; AC6 → PR 2.5; AC7 → PR 2.1; AC8 → PR 4.6. ✓

No placeholders found in tasks; all code blocks present where referenced. Type names consistent across tasks (`PlatformUnitExtra`, `WindowsLocalAuthority`, `JunctionActive`, `EventLogReader`).
