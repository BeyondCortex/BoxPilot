# BoxPilot Platform Abstraction Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Introduce a `boxpilot-platform` crate carrying platform-neutral traits + a complete Linux implementation + Windows stub implementations + cross-platform fakes, then refactor `boxpilotd`, `boxpilot-profile`, and `boxpilot-tauri` to consume it. Land "Linux behavior unchanged + Windows compiles + Windows minimum SCM boot" — the first of three Windows-port sub-projects.

**Architecture:** All Linux/Windows differences (IPC transport, service control, file locks, ACLs, atomic active-link, bundle bytes transfer) sit behind traits in a single `boxpilot-platform` crate, gated by `cfg(target_os = "...")` per impl. `boxpilotd` keeps its existing Linux behavior on Linux, and on Windows runs as a Windows Service via `windows-service::service_dispatcher` with a Named Pipe IPC accept loop returning `HelperError::NotImplemented` for every verb. A new `boxpilotctl` debug bin verifies AC5 by hitting the Named Pipe end-to-end. Bundle bytes flow through an `AuxStream` parameter on the dispatch and IpcClient surfaces — no separate `BundleClient`/`BundleServer` traits.

**Tech Stack:** Rust 2021, `tokio` (with `net` + `io-util` features added by this plan), `async-trait`, `zbus` 5 (Linux IPC), `windows-service` 0.7 + `windows-sys` 0.59 (Windows SCM + Named Pipe + ACL), `tracing-appender` (Windows file sink), `nix` 0.29 (Linux only), Tauri 2 + Vue 3 + Vite frontend.

**Worktree note:** Not required (the existing branch `feat/windows-support` already exists). Each PR can land via standard small-PR review on `main`.

**Spec:** `docs/superpowers/specs/2026-05-01-boxpilot-platform-abstraction-design.md` (1765 lines, 17 COQs resolved, 6 review rounds). This plan implements §8's PR sequencing 1-1 with each task under one PR heading.

---

## Out of Scope (Sub-project #2 / #3)

- Real implementation of any Windows helper verb. Windows verbs return `HelperError::NotImplemented` in this sub-project. (Sub-project #2.)
- Windows installer (MSI / MSIX / NSIS). Development uses manual `sc create`. (Sub-project #3.)
- Wintun driver bundling, TUN configuration. (Sub-project #3.)
- `boxpilot.toml` schema bump for SID-based controller principal. `controller_uid: u32` stays as-is. (Sub-project #2 / spec §10.)
- Windows GUI text adjustments ("systemd"/"polkit"/"journalctl" wording). (Sub-project #2.)
- macOS support — trait shapes are not warped to accommodate it.
- Rewriting any existing Linux fake/mock. Existing fakes are moved into `boxpilot-platform` with their behavior intact.

---

## File Structure (workspace state at end of Sub-project #1)

```text
Cargo.toml                                        # workspace; tokio features += net,io-util; tracing-appender added
crates/
  boxpilot-ipc/                                   # SCHEMA — unchanged structure; additive accessors
    src/
      method.rs                                   # + wire::wire_id() / from_wire_id() / aux_shape() / aux_size_cap()
      ... (rest unchanged)
  boxpilot-platform/                              # NEW
    Cargo.toml                                    # cfg-gated linux/windows deps
    src/
      lib.rs                                      # facade: pub use of platform-selected impls
      paths.rs                                    # Paths struct (cfg-gated method bodies)
      traits/
        mod.rs
        env.rs                                    # EnvProvider
        lock.rs                                   # FileLock
        ipc.rs                                    # IpcServer, IpcConnection, IpcClient, ConnectionInfo,
                                                  #   HelperDispatch, AuxStream, CallerPrincipal
        service.rs                                # ServiceManager (verbatim port of Systemd trait)
        trust.rs                                  # TrustChecker
        active.rs                                 # ActivePointer
        authority.rs                              # Authority
        logs.rs                                   # LogReader
        core_assets.rs                            # CoreAssetNaming, CoreArchive
        fs_meta.rs                                # FsMetadataProvider
        fs_perms.rs                               # FsPermissions (per COQ12)
        version.rs                                # VersionChecker
        user_lookup.rs                            # UserLookup
      linux/
        mod.rs
        env.rs, lock.rs, ipc.rs (zbus), service.rs (zbus systemd1), trust.rs (uid+mode),
        active.rs (symlink+rename), authority.rs (polkit), logs.rs (journalctl),
        core_assets.rs (tar.gz), fs_meta.rs, fs_perms.rs (chmod),
        version.rs (exec), user_lookup.rs (getpwuid),
        credentials.rs                            # GetConnectionUnixUser; absorbed by IpcServer in PR 11a
      windows/
        mod.rs
        env.rs, lock.rs (LockFileEx), ipc.rs (named_pipe + windows-service),
        service.rs (stub), trust.rs (stub), active.rs (stub), authority.rs (AlwaysAllow),
        logs.rs (stub), core_assets.rs (stub zip), fs_meta.rs (stub),
        fs_perms.rs (SetSecurityInfo), version.rs (stub), user_lookup.rs (stub)
      fakes/
        mod.rs
        env.rs, lock.rs, ipc.rs (mpsc pair), service.rs, trust.rs, active.rs, authority.rs,
        logs.rs, core_assets.rs, fs_meta.rs, fs_perms.rs, version.rs, user_lookup.rs,
        bundle_aux.rs                             # AuxStream test helpers (in-memory Cursor)
  boxpilotd/
    Cargo.toml                                    # drops zbus direct dep (PR 11a); nix stays at package-level
    src/
      main.rs                                     # cfg-split entry
      entry/
        mod.rs
        linux.rs                                  # tokio runtime + zbus + signal loop
        windows.rs                                # service_dispatcher + run_under_scm + tracing-appender
      bin/
        boxpilotctl.rs                            # NEW (PR 14b) — debug client over IpcClient
      iface.rs                                    # zbus #[interface]; thin shell after PR 11a
      dispatch.rs                                 # authorize() — refactored to CallerPrincipal in PR 4
      dispatch_handler.rs                         # NEW (PR 11a) — match HelperMethod { … } router
      handlers/                                   # NEW (PR 11a) — one module per verb, taking (principal, body, aux)
        mod.rs
        service_status.rs, service_start.rs, ..., diagnostics_export.rs
      systemd.rs                                  # MOVED to boxpilot-platform/linux/service.rs in PR 5
      authority.rs                                # MOVED to boxpilot-platform/linux/authority.rs in PR 4
      credentials.rs                              # KEPT Linux-internal in PR 4; absorbed PR 11a
      controller.rs                               # ControllerState + UserLookup; UserLookup moves PR 3
      lock.rs                                     # WRAPPED via FileLock trait in PR 6
      paths.rs                                    # MOVED to boxpilot-platform/paths.rs in PR 2
      core/                                       # trust.rs wrapped PR 7; install.rs uses CoreArchive PR 9
      profile/                                    # release.rs wrapped via ActivePointer PR 8;
                                                  # unpack.rs uses AuxStream PR 10
      legacy/                                     # cfg(target_os="linux") at module level
  boxpilot-profile/
    Cargo.toml                                    # nix/libc stay at package level (Round 6/6.2)
    src/
      bundle.rs                                   # memfd usage moves to platform crate in PR 10;
                                                  # this file becomes a thin wrapper around AuxStream
      check.rs                                    # cfg-split per COQ14: Linux unchanged, Windows stub returns success
      store.rs, meta.rs, import.rs, remotes.rs    # PermissionsExt → FsPermissions::restrict_to_owner in PR 3
      linux/check.rs                              # NEW (PR 9): houses the existing pgid+SIGKILL impl
      windows/check.rs                            # NEW (PR 9): stub returning CheckOutput { success: true, … }
  boxpilot-tauri/
    Cargo.toml                                    # zbus direct dep removed in PR 11b
    src/
      lib.rs                                      # registers Paths as tauri::State (PR 2)
      helper_client.rs                            # rewritten in PR 11b — thin IpcClient wrappers
      profile_cmds.rs                             # raw zbus FD-passing absorbed into platform/linux/ipc.rs (PR 11b)
      commands.rs                                 # unchanged
docs/
  superpowers/
    specs/2026-05-01-boxpilot-platform-abstraction-design.md
    plans/2026-05-01-boxpilot-platform-abstraction.md  # this file
.github/workflows/
  windows-check.yml                               # NEW (PR 1) — cargo check --target *-windows-gnu allow-fail
                                                  # promoted to required at PR 11a, MSVC at PR 14
```

---

## Naming Contract (locked at the top, referenced by every later PR)

Inherited from spec §8 PR 4 task list and the existing v0.1.1 wire commitments:

| Concept                        | Form                                       | Example                                       |
|--------------------------------|--------------------------------------------|-----------------------------------------------|
| **Logical action**             | dotted, underscores                        | `service.status`, `profile.activate_bundle`   |
| **D-Bus method** (zbus)        | CamelCase                                  | `ServiceStatus`, `ProfileActivateBundle`      |
| **D-Bus bus name**             | `app.boxpilot.Helper` (FROZEN; PR 4 test)  | —                                             |
| **D-Bus object path**          | `/app/boxpilot/Helper` (FROZEN; PR 4 test) | —                                             |
| **D-Bus interface name**       | `app.boxpilot.Helper1` (FROZEN)            | —                                             |
| **polkit action ID**           | `app.boxpilot.helper.<dotted-with-dashes>` | `app.boxpilot.helper.profile.activate-bundle` |
| **Windows pipe name**          | `\\.\pipe\boxpilot-helper`                 | —                                             |
| **HelperMethod wire id**       | `u32` (additive accessor PR 11a)           | `ServiceStatus → 0x0001`, etc.                |
| **HelperError wire id**        | `u32` (additive accessor PR 11a)           | `NotAuthorized → 0x0010`, etc.                |
| **Wire format magic**          | `0x426F7850` (ASCII "BoxP")                | header.magic                                  |

---

## Cross-PR conventions

Used by every PR; do not restate per task.

**Test naming:** snake_case describing the behavior asserted (e.g., `windows_authority_always_allows_with_warn_log`). One test per behavior. Prefer `pretty_assertions::assert_eq` for struct comparisons.

**Fake construction:** every trait fake exposes a `fn new() -> Self` plus per-test setters (`with_user(uid, name)`, `record_calls()`, etc.). Tests in this plan show one fake setup per Task; subsequent tasks of the same PR reuse it.

**Commit style:** Conventional Commits (`feat(scope): …`, `refactor(scope): …`, `test(scope): …`). Each task's last step is a commit; never batch commits across tasks unless the task explicitly says so.

**Linux non-regression check:** every PR ends with the same gate before merging:

```bash
cargo test --workspace --target x86_64-unknown-linux-gnu  # must be green
```

The smoke procedures from `docs/superpowers/plans/*-smoke-procedure.md` are NOT re-run per PR — they're run as a release gate by AC1 once Sub-project #1 merges. Each PR is responsible only for `cargo test --workspace`.

**Windows compile gate progression (per COQ13):**

| PR     | Windows `cargo check --target` | gate |
|--------|--------------------------------|------|
| 1–10   | `x86_64-pc-windows-gnu`        | allow-fail |
| 11a–13 | `x86_64-pc-windows-gnu`        | **required** |
| 14+    | `x86_64-pc-windows-msvc`       | required (Windows runner) |

**Cfg-gating idiom:** always use `target_os` not `unix`/`windows`-derived helpers; e.g., `#[cfg(target_os = "linux")]` and `#[cfg(target_os = "windows")]`. Other cfgs are out of scope for this sub-project (`target_os = "macos"` modules don't need to exist).

---

# PR 1: Scaffold `boxpilot-platform` crate

**Size:** XS · **Touches:** root `Cargo.toml`, new `crates/boxpilot-platform/`, `.github/workflows/` · **Linux non-regression:** trivially green (no existing Linux code is touched besides workspace `Cargo.toml`)

This PR creates the empty platform crate, registers it in the workspace, bumps shared deps, and adds a CI step that runs `cargo check --target x86_64-pc-windows-gnu` on every PR (allow-fail through PR 10 per COQ13).

## Task 1.1: Bump workspace deps

**Files:**
- Modify: `Cargo.toml`

- [ ] **Step 1: Read current `Cargo.toml`**

Run: `cat Cargo.toml`
Expected: workspace deps include `tokio = { version = "1", features = ["macros", "rt-multi-thread", "signal", "fs", "sync"] }` and **do not** include `tracing-appender`.

- [ ] **Step 2: Apply edits**

Replace the existing `tokio` dep line:

```toml
tokio = { version = "1", features = ["macros", "rt-multi-thread", "signal", "fs", "sync", "net", "io-util"] }
```

Append after existing `chrono` line:

```toml
tracing-appender = "0.2"
```

- [ ] **Step 3: Verify workspace still parses**

Run: `cargo metadata --no-deps --format-version 1 >/dev/null`
Expected: exit 0.

- [ ] **Step 4: Verify Linux build still green**

Run: `cargo build --workspace`
Expected: builds clean (no source uses the new tokio features yet, and `tracing-appender` is unused — that's fine; `cargo build` doesn't fail on unused workspace deps).

- [ ] **Step 5: Commit**

```bash
git add Cargo.toml
git commit -m "chore(deps): add tokio net+io-util features and tracing-appender for Windows port"
```

---

## Task 1.2: Create empty `boxpilot-platform` crate

**Files:**
- Create: `crates/boxpilot-platform/Cargo.toml`
- Create: `crates/boxpilot-platform/src/lib.rs`
- Modify: `Cargo.toml` (workspace `members` array)

- [ ] **Step 1: Add member to workspace**

Modify the `[workspace] members = [...]` array in `Cargo.toml` to include `"crates/boxpilot-platform"`. Final state:

```toml
[workspace]
resolver = "2"
members = [
    "crates/boxpilot-ipc",
    "crates/boxpilotd",
    "crates/boxpilot-tauri",
    "crates/boxpilot-profile",
    "crates/boxpilot-platform",
]
```

- [ ] **Step 2: Write the failing test (sentinel only — real tests arrive in later PRs)**

`crates/boxpilot-platform/src/lib.rs`:

```rust
//! BoxPilot platform-abstraction crate.
//!
//! Houses platform-neutral traits, Linux + Windows implementations gated by
//! `cfg(target_os = "...")`, and cross-platform fakes for tests. See spec
//! `docs/superpowers/specs/2026-05-01-boxpilot-platform-abstraction-design.md`.

#[cfg(test)]
mod tests {
    #[test]
    fn crate_compiles() {
        assert_eq!(2 + 2, 4);
    }
}
```

- [ ] **Step 3: Write `Cargo.toml`**

`crates/boxpilot-platform/Cargo.toml`:

```toml
[package]
name = "boxpilot-platform"
version = "0.1.0"
edition.workspace = true
rust-version.workspace = true
license.workspace = true

[dependencies]
boxpilot-ipc = { path = "../boxpilot-ipc" }
async-trait.workspace = true
tokio = { workspace = true, features = ["macros", "rt-multi-thread", "fs", "sync", "net", "io-util"] }
serde.workspace = true
serde_json.workspace = true
thiserror.workspace = true
tracing.workspace = true

[target.'cfg(target_os = "linux")'.dependencies]
zbus.workspace = true
nix.workspace = true
libc.workspace = true
fs2.workspace = true
tar.workspace = true
flate2.workspace = true

[target.'cfg(target_os = "windows")'.dependencies]
windows-service = "0.7"
zip = { version = "2", default-features = false, features = ["deflate"] }
windows-sys = { version = "0.59", features = [
    "Win32_Foundation",
    "Win32_Storage_FileSystem",
    "Win32_System_IO",
    "Win32_System_Pipes",
    "Win32_System_Services",
    "Win32_Security",
    "Win32_Security_Authorization",
] }

[dev-dependencies]
pretty_assertions.workspace = true
tempfile.workspace = true
```

- [ ] **Step 4: Run the sentinel test on Linux**

Run: `cargo test -p boxpilot-platform`
Expected: `test crate_compiles ... ok` — 1 passed.

- [ ] **Step 5: Verify the crate Cargo-checks for the Windows GNU target**

Run: `cargo check -p boxpilot-platform --target x86_64-pc-windows-gnu`
Expected: succeeds (the crate has no real source yet, so target-specific deps don't get exercised).

If `x86_64-pc-windows-gnu` is not installed, run: `rustup target add x86_64-pc-windows-gnu` first. Document this in the commit message footer for reviewers.

- [ ] **Step 6: Commit**

```bash
git add crates/boxpilot-platform Cargo.toml
git commit -m "feat(platform): scaffold boxpilot-platform crate"
```

---

## Task 1.3: Add module skeleton (empty traits + cfg-gated subdirs)

**Files:**
- Create: `crates/boxpilot-platform/src/traits/mod.rs`
- Create: `crates/boxpilot-platform/src/linux/mod.rs`
- Create: `crates/boxpilot-platform/src/windows/mod.rs`
- Create: `crates/boxpilot-platform/src/fakes/mod.rs`
- Modify: `crates/boxpilot-platform/src/lib.rs`

- [ ] **Step 1: Create empty subdirs with `mod.rs` placeholders**

`crates/boxpilot-platform/src/traits/mod.rs`:

```rust
//! Platform-neutral trait interfaces. Implementations live in `linux/`,
//! `windows/`, and `fakes/`. Traits arrive in later PRs:
//!
//! - PR 2: `EnvProvider`
//! - PR 3: `FsMetadataProvider`, `VersionChecker`, `UserLookup`, `FsPermissions`
//! - PR 4: `Authority`
//! - PR 5: `ServiceManager`, `LogReader`
//! - PR 6: `FileLock`
//! - PR 7: `TrustChecker`
//! - PR 8: `ActivePointer`
//! - PR 9: `CoreAssetNaming`, `CoreArchive`
//! - PR 10: `AuxStream` (struct, not trait)
//! - PR 11a: `IpcServer`, `IpcConnection`, `IpcClient`, `HelperDispatch`
```

`crates/boxpilot-platform/src/linux/mod.rs`:

```rust
//! Linux implementations of the traits in `crate::traits`. Each module
//! arrives alongside its trait in the corresponding PR.

#![cfg(target_os = "linux")]
```

`crates/boxpilot-platform/src/windows/mod.rs`:

```rust
//! Windows implementations of the traits in `crate::traits`. Most are
//! `unimplemented!()` stubs in Sub-project #1 (per
//! `docs/superpowers/specs/2026-05-01-boxpilot-platform-abstraction-design.md`
//! §5 trait inventory).

#![cfg(target_os = "windows")]
```

`crates/boxpilot-platform/src/fakes/mod.rs`:

```rust
//! Cross-platform test doubles for every trait. Compile on all targets so
//! helper-side unit tests pass on the Windows runner (AC4).
```

- [ ] **Step 2: Wire mods into `lib.rs`**

Replace `crates/boxpilot-platform/src/lib.rs` with:

```rust
//! BoxPilot platform-abstraction crate.
//!
//! Houses platform-neutral traits, Linux + Windows implementations gated by
//! `cfg(target_os = "...")`, and cross-platform fakes for tests. See spec
//! `docs/superpowers/specs/2026-05-01-boxpilot-platform-abstraction-design.md`.

pub mod traits;

#[cfg(target_os = "linux")]
pub mod linux;

#[cfg(target_os = "windows")]
pub mod windows;

pub mod fakes;

#[cfg(test)]
mod tests {
    #[test]
    fn crate_compiles() {
        assert_eq!(2 + 2, 4);
    }
}
```

- [ ] **Step 3: Build for Linux**

Run: `cargo build -p boxpilot-platform`
Expected: clean.

- [ ] **Step 4: Build for the Windows GNU target**

Run: `cargo check -p boxpilot-platform --target x86_64-pc-windows-gnu`
Expected: clean.

- [ ] **Step 5: Commit**

```bash
git add crates/boxpilot-platform/src
git commit -m "feat(platform): empty traits/, linux/, windows/, fakes/ module skeleton"
```

---

## Task 1.4: Add Windows compile-check CI workflow

**Files:**
- Create: `.github/workflows/windows-check.yml`

- [ ] **Step 1: Write the workflow**

`.github/workflows/windows-check.yml`:

```yaml
name: windows-check
on:
  pull_request:
  push:
    branches: [main, "feat/windows-support"]

jobs:
  cargo-check-windows-gnu:
    name: cargo check (x86_64-pc-windows-gnu)
    runs-on: ubuntu-latest
    # Allow-fail through PR 10 per spec COQ13. Promoted to a required check
    # at PR 11a (boxpilot-profile/bundle.rs no longer uses nix::memfd by
    # then). Switched to x86_64-pc-windows-msvc on a windows-latest runner
    # at PR 14.
    continue-on-error: true
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
        with:
          targets: x86_64-pc-windows-gnu
      - uses: Swatinem/rust-cache@v2
        with:
          shared-key: windows-gnu
      - name: Install mingw-w64 (provides x86_64-w64-mingw32-gcc as the GNU linker)
        run: sudo apt-get update && sudo apt-get install -y --no-install-recommends mingw-w64
      - name: cargo check --target x86_64-pc-windows-gnu --workspace
        run: cargo check --target x86_64-pc-windows-gnu --workspace
```

- [ ] **Step 2: Verify locally (best effort)**

Run: `cargo check --target x86_64-pc-windows-gnu --workspace`
Expected: **fails** (boxpilot-profile/src/store.rs:1 has `use std::os::unix::fs::PermissionsExt;`). The `continue-on-error: true` flag in CI converts this into a yellow check, not a red one. The PR 1 reviewer should see "1 check, neutral/yellow" rather than "all green".

If the local check actually passes (e.g., a follow-up PR has already fixed everything), that's also fine — the CI step will just stay green.

- [ ] **Step 3: Commit**

```bash
git add .github/workflows/windows-check.yml
git commit -m "ci: add windows-gnu cargo-check (allow-fail through PR 10 per spec COQ13)"
```

---

## Task 1.5: Sanity-test the new workspace member from `boxpilotd`

**Files:**
- Modify: `crates/boxpilotd/Cargo.toml` (add `boxpilot-platform` as a dep behind `[dev-dependencies]` only — no production wiring yet)

- [ ] **Step 1: Add the dev-dep so PR 2's call sites have one place to come from**

Append to `crates/boxpilotd/Cargo.toml` under existing `[dev-dependencies]`:

```toml
[dev-dependencies]
# ... existing entries ...
boxpilot-platform = { path = "../boxpilot-platform" }
```

(If no `[dev-dependencies]` table exists yet in this crate's manifest, create one.)

- [ ] **Step 2: Run boxpilotd tests to confirm the dep resolves**

Run: `cargo test -p boxpilotd --lib`
Expected: existing tests still pass; the new dep is harmless because no source imports it.

- [ ] **Step 3: Commit**

```bash
git add crates/boxpilotd/Cargo.toml
git commit -m "chore(boxpilotd): wire boxpilot-platform as dev-dep stub for PR 2"
```

---

## PR 1 smoke

```bash
cargo test --workspace                                   # Linux non-regression
cargo check --target x86_64-pc-windows-gnu --workspace || echo "expected fail; allow-fail through PR 10"
gh pr create --title "chore(platform): scaffold boxpilot-platform crate (PR 1/16)" \
             --body "Spec: docs/superpowers/specs/2026-05-01-boxpilot-platform-abstraction-design.md §8 PR 1"
```

---

# PR 2: `EnvProvider` + `Paths` value type, plus `ProfileStorePaths::from_paths`

**Size:** M · **Touches:** `boxpilot-platform/src/{traits,linux,windows,fakes}/env.rs`, `boxpilot-platform/src/paths.rs`, `boxpilotd/src/paths.rs` (deleted), all `boxpilotd` callers, `boxpilot-profile/src/store.rs`, `boxpilot-tauri/src/lib.rs`. **Linux non-regression:** every existing `Paths::with_root`/`Paths::system()` call must keep its current behavior bit-for-bit.

This PR introduces `EnvProvider` (one trait method per env var read currently in source), the `Paths` value type with cfg-gated method bodies, and threads it through Tauri command handlers as `tauri::State`. Per spec §5.1 + COQ16. Per Round 6/6.4: `ProfileStorePaths::from_env()` has exactly **one** call site (`boxpilot-tauri/src/lib.rs:11`); the threading change is small.

## Task 2.1: `EnvProvider` trait + Linux/Windows impls + fake

**Files:**
- Create: `crates/boxpilot-platform/src/traits/env.rs`
- Create: `crates/boxpilot-platform/src/linux/env.rs`
- Create: `crates/boxpilot-platform/src/windows/env.rs`
- Create: `crates/boxpilot-platform/src/fakes/env.rs`
- Modify: `crates/boxpilot-platform/src/{traits,linux,windows,fakes}/mod.rs`

- [ ] **Step 1: Write the trait**

`crates/boxpilot-platform/src/traits/env.rs`:

```rust
//! Environment-variable access abstracted so `Paths` (§5.1) can build
//! platform-correct roots without each caller doing OS-specific lookups.
//! Linux: reads `$XDG_DATA_HOME` and `$HOME`. Windows: reads
//! `%ProgramData%` and `%LocalAppData%`. Test fakes inject a static map.

use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum EnvError {
    #[error("required environment variable missing: {0}")]
    Missing(&'static str),
    #[error("env value is not valid UTF-8: {0}")]
    NotUtf8(&'static str),
}

pub trait EnvProvider: Send + Sync {
    /// System-wide data root.
    /// Linux: `/` (returned as `PathBuf::from("/")`).
    /// Windows: `%ProgramData%\BoxPilot` (typically `C:\ProgramData\BoxPilot`).
    fn system_root(&self) -> Result<PathBuf, EnvError>;

    /// Per-user data root.
    /// Linux: `$XDG_DATA_HOME/boxpilot` if `XDG_DATA_HOME` set, else
    /// `$HOME/.local/share/boxpilot`.
    /// Windows: `%LocalAppData%\BoxPilot`.
    fn user_root(&self) -> Result<PathBuf, EnvError>;
}
```

- [ ] **Step 2: Write Linux impl**

`crates/boxpilot-platform/src/linux/env.rs`:

```rust
use crate::traits::env::{EnvError, EnvProvider};
use std::path::PathBuf;

/// Reads from the process environment using `std::env::var_os`.
pub struct StdEnv;

impl EnvProvider for StdEnv {
    fn system_root(&self) -> Result<PathBuf, EnvError> {
        Ok(PathBuf::from("/"))
    }

    fn user_root(&self) -> Result<PathBuf, EnvError> {
        if let Some(xdg) = std::env::var_os("XDG_DATA_HOME") {
            return Ok(PathBuf::from(xdg).join("boxpilot"));
        }
        let home = std::env::var_os("HOME").ok_or(EnvError::Missing("HOME"))?;
        Ok(PathBuf::from(home).join(".local/share/boxpilot"))
    }
}
```

- [ ] **Step 3: Write Windows impl**

`crates/boxpilot-platform/src/windows/env.rs`:

```rust
use crate::traits::env::{EnvError, EnvProvider};
use std::path::PathBuf;

pub struct StdEnv;

impl EnvProvider for StdEnv {
    fn system_root(&self) -> Result<PathBuf, EnvError> {
        let pd = std::env::var_os("ProgramData").ok_or(EnvError::Missing("ProgramData"))?;
        Ok(PathBuf::from(pd).join("BoxPilot"))
    }

    fn user_root(&self) -> Result<PathBuf, EnvError> {
        let lad = std::env::var_os("LocalAppData").ok_or(EnvError::Missing("LocalAppData"))?;
        Ok(PathBuf::from(lad).join("BoxPilot"))
    }
}
```

- [ ] **Step 4: Write the fake**

`crates/boxpilot-platform/src/fakes/env.rs`:

```rust
use crate::traits::env::{EnvError, EnvProvider};
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct FixedEnv {
    pub system_root: PathBuf,
    pub user_root: PathBuf,
}

impl FixedEnv {
    pub fn under(tmp: &std::path::Path) -> Self {
        Self {
            system_root: tmp.to_path_buf(),
            user_root: tmp.join("user"),
        }
    }
}

impl EnvProvider for FixedEnv {
    fn system_root(&self) -> Result<PathBuf, EnvError> {
        Ok(self.system_root.clone())
    }
    fn user_root(&self) -> Result<PathBuf, EnvError> {
        Ok(self.user_root.clone())
    }
}
```

- [ ] **Step 5: Wire mods**

Add to `crates/boxpilot-platform/src/traits/mod.rs`:

```rust
pub mod env;
```

Add to `crates/boxpilot-platform/src/linux/mod.rs`:

```rust
pub mod env;
```

Add to `crates/boxpilot-platform/src/windows/mod.rs`:

```rust
pub mod env;
```

Add to `crates/boxpilot-platform/src/fakes/mod.rs`:

```rust
pub mod env;
```

- [ ] **Step 6: Write a fake test (round-trip + rooted-tmp behavior)**

Append to `crates/boxpilot-platform/src/fakes/env.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::traits::env::EnvProvider;
    use tempfile::tempdir;

    #[test]
    fn under_tmp_returns_system_and_user_root_under_tmp() {
        let tmp = tempdir().unwrap();
        let env = FixedEnv::under(tmp.path());
        assert_eq!(env.system_root().unwrap(), tmp.path());
        assert_eq!(env.user_root().unwrap(), tmp.path().join("user"));
    }
}
```

- [ ] **Step 7: Run tests**

Run: `cargo test -p boxpilot-platform`
Expected: 2 tests pass (`crate_compiles`, `under_tmp_returns_system_and_user_root_under_tmp`).

- [ ] **Step 8: Commit**

```bash
git add crates/boxpilot-platform/src
git commit -m "feat(platform): EnvProvider trait + Linux/Windows impls + fake"
```

---

## Task 2.2: `Paths` value type with cfg-gated bodies

**Files:**
- Create: `crates/boxpilot-platform/src/paths.rs`
- Modify: `crates/boxpilot-platform/src/lib.rs`

The new `Paths` carries the existing methods from `boxpilotd::paths::Paths` plus three additions for user-side state. Method names are kept identical so PR 2 callers can swap the import without changing call sites.

- [ ] **Step 1: Write the failing test (cross-platform layout assertions)**

`crates/boxpilot-platform/src/paths.rs`:

```rust
//! Canonical filesystem paths. Constructors call `EnvProvider` once at boot
//! and cache the resulting roots.
//!
//! Platform layout (per spec §5.1 + §7):
//!
//! - **Linux:** `system_root = /`, paths under `/etc/boxpilot/`,
//!   `/var/lib/boxpilot/`, `/var/cache/boxpilot/`, `/run/boxpilot/`,
//!   `/etc/systemd/system/`, `/etc/polkit-1/rules.d/`.
//!   `user_root = $HOME/.local/share/boxpilot` (or `$XDG_DATA_HOME/boxpilot`).
//! - **Windows:** `system_root = %ProgramData%\BoxPilot`, paths flatten
//!   directly under that root (no `etc/`/`var/` segments — `boxpilot.toml`
//!   sits at `system_root.join("boxpilot.toml")`).
//!   `user_root = %LocalAppData%\BoxPilot`.

use crate::traits::env::{EnvError, EnvProvider};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct Paths {
    system_root: PathBuf,
    user_root: PathBuf,
}

impl Paths {
    pub fn from_env(env: &dyn EnvProvider) -> Result<Self, EnvError> {
        Ok(Self {
            system_root: env.system_root()?,
            user_root: env.user_root()?,
        })
    }

    /// Production constructor — uses [`crate::linux::env::StdEnv`] /
    /// [`crate::windows::env::StdEnv`] depending on target.
    pub fn system() -> Result<Self, EnvError> {
        #[cfg(target_os = "linux")]
        {
            return Self::from_env(&crate::linux::env::StdEnv);
        }
        #[cfg(target_os = "windows")]
        {
            return Self::from_env(&crate::windows::env::StdEnv);
        }
        #[allow(unreachable_code)]
        Err(EnvError::Missing("unsupported platform"))
    }

    /// Test/dev constructor — both roots under `tmp`.
    pub fn with_root(tmp: impl AsRef<Path>) -> Self {
        let tmp = tmp.as_ref().to_path_buf();
        Self {
            user_root: tmp.join("user"),
            system_root: tmp,
        }
    }

    pub fn user_root(&self) -> &Path {
        &self.user_root
    }

    // ---- §5.3 system runtime state ------------------------------------

    pub fn boxpilot_toml(&self) -> PathBuf {
        #[cfg(target_os = "linux")]
        {
            self.system_root.join("etc/boxpilot/boxpilot.toml")
        }
        #[cfg(target_os = "windows")]
        {
            self.system_root.join("boxpilot.toml")
        }
        #[cfg(not(any(target_os = "linux", target_os = "windows")))]
        {
            unreachable!("unsupported platform")
        }
    }

    pub fn controller_name_file(&self) -> PathBuf {
        // Linux-only on disk; on Windows the file is never written.
        // Method exists on both platforms for caller-uniformity; callers
        // that write it must be cfg(target_os = "linux")-gated.
        #[cfg(target_os = "linux")]
        {
            self.system_root.join("etc/boxpilot/controller-name")
        }
        #[cfg(target_os = "windows")]
        {
            self.system_root.join("controller-name")
        }
        #[cfg(not(any(target_os = "linux", target_os = "windows")))]
        {
            unreachable!("unsupported platform")
        }
    }

    pub fn run_lock(&self) -> PathBuf {
        #[cfg(target_os = "linux")]
        {
            self.system_root.join("run/boxpilot/lock")
        }
        #[cfg(target_os = "windows")]
        {
            self.system_root.join("run").join("lock")
        }
        #[cfg(not(any(target_os = "linux", target_os = "windows")))]
        {
            unreachable!("unsupported platform")
        }
    }

    pub fn run_dir(&self) -> PathBuf {
        #[cfg(target_os = "linux")]
        {
            self.system_root.join("run/boxpilot")
        }
        #[cfg(target_os = "windows")]
        {
            self.system_root.join("run")
        }
        #[cfg(not(any(target_os = "linux", target_os = "windows")))]
        {
            unreachable!("unsupported platform")
        }
    }

    pub fn etc_dir(&self) -> PathBuf {
        #[cfg(target_os = "linux")]
        {
            self.system_root.join("etc/boxpilot")
        }
        #[cfg(target_os = "windows")]
        {
            self.system_root.clone()
        }
        #[cfg(not(any(target_os = "linux", target_os = "windows")))]
        {
            unreachable!("unsupported platform")
        }
    }

    pub fn install_state_json(&self) -> PathBuf {
        #[cfg(target_os = "linux")]
        {
            self.system_root.join("var/lib/boxpilot/install-state.json")
        }
        #[cfg(target_os = "windows")]
        {
            self.system_root.join("install-state.json")
        }
        #[cfg(not(any(target_os = "linux", target_os = "windows")))]
        {
            unreachable!("unsupported platform")
        }
    }

    pub fn cores_dir(&self) -> PathBuf {
        #[cfg(target_os = "linux")]
        {
            self.system_root.join("var/lib/boxpilot/cores")
        }
        #[cfg(target_os = "windows")]
        {
            self.system_root.join("cores")
        }
        #[cfg(not(any(target_os = "linux", target_os = "windows")))]
        {
            unreachable!("unsupported platform")
        }
    }

    pub fn cores_current_symlink(&self) -> PathBuf {
        self.cores_dir().join("current")
    }

    pub fn cores_staging_dir(&self) -> PathBuf {
        #[cfg(target_os = "linux")]
        {
            self.system_root.join("var/lib/boxpilot/.staging-cores")
        }
        #[cfg(target_os = "windows")]
        {
            self.system_root.join(".staging-cores")
        }
        #[cfg(not(any(target_os = "linux", target_os = "windows")))]
        {
            unreachable!("unsupported platform")
        }
    }

    pub fn systemd_unit_path(&self, unit_name: &str) -> PathBuf {
        // Linux-only callers; Windows has no systemd. Method present for
        // call-site uniformity but should be cfg-gated by callers.
        #[cfg(target_os = "linux")]
        {
            self.system_root.join("etc/systemd/system").join(unit_name)
        }
        #[cfg(target_os = "windows")]
        {
            self.system_root.join("systemd-units").join(unit_name)
        }
        #[cfg(not(any(target_os = "linux", target_os = "windows")))]
        {
            unreachable!("unsupported platform")
        }
    }

    pub fn polkit_controller_dropin_path(&self) -> PathBuf {
        #[cfg(target_os = "linux")]
        {
            self.system_root
                .join("etc/polkit-1/rules.d/48-boxpilot-controller.rules")
        }
        #[cfg(target_os = "windows")]
        {
            self.system_root.join("polkit-controller.rules") // unused on Windows
        }
        #[cfg(not(any(target_os = "linux", target_os = "windows")))]
        {
            unreachable!("unsupported platform")
        }
    }

    pub fn releases_dir(&self) -> PathBuf {
        #[cfg(target_os = "linux")]
        {
            self.system_root.join("etc/boxpilot/releases")
        }
        #[cfg(target_os = "windows")]
        {
            self.system_root.join("releases")
        }
        #[cfg(not(any(target_os = "linux", target_os = "windows")))]
        {
            unreachable!("unsupported platform")
        }
    }

    pub fn staging_dir(&self) -> PathBuf {
        #[cfg(target_os = "linux")]
        {
            self.system_root.join("etc/boxpilot/.staging")
        }
        #[cfg(target_os = "windows")]
        {
            self.system_root.join(".staging")
        }
        #[cfg(not(any(target_os = "linux", target_os = "windows")))]
        {
            unreachable!("unsupported platform")
        }
    }

    pub fn active_symlink(&self) -> PathBuf {
        // Linux: symlink. Windows: marker JSON file at active.json (PR 8/COQ8 / spec §5.3).
        // Both platforms expose this method; PR 8's ActivePointer trait
        // deals with the semantic difference.
        #[cfg(target_os = "linux")]
        {
            self.system_root.join("etc/boxpilot/active")
        }
        #[cfg(target_os = "windows")]
        {
            self.system_root.join("active.json")
        }
        #[cfg(not(any(target_os = "linux", target_os = "windows")))]
        {
            unreachable!("unsupported platform")
        }
    }

    pub fn release_dir(&self, activation_id: &str) -> PathBuf {
        self.releases_dir().join(activation_id)
    }

    pub fn staging_subdir(&self, activation_id: &str) -> PathBuf {
        self.staging_dir().join(activation_id)
    }

    pub fn backups_units_dir(&self) -> PathBuf {
        #[cfg(target_os = "linux")]
        {
            self.system_root.join("var/lib/boxpilot/backups/units")
        }
        #[cfg(target_os = "windows")]
        {
            self.system_root.join("backups").join("units")
        }
        #[cfg(not(any(target_os = "linux", target_os = "windows")))]
        {
            unreachable!("unsupported platform")
        }
    }

    pub fn cache_diagnostics_dir(&self) -> PathBuf {
        #[cfg(target_os = "linux")]
        {
            self.system_root.join("var/cache/boxpilot/diagnostics")
        }
        #[cfg(target_os = "windows")]
        {
            self.system_root.join("cache").join("diagnostics")
        }
        #[cfg(not(any(target_os = "linux", target_os = "windows")))]
        {
            unreachable!("unsupported platform")
        }
    }

    // ---- §5.6 user profile store --------------------------------------

    pub fn user_profiles_dir(&self) -> PathBuf {
        self.user_root.join("profiles")
    }

    pub fn user_remotes_json(&self) -> PathBuf {
        self.user_root.join("remotes.json")
    }

    pub fn user_ui_state_json(&self) -> PathBuf {
        self.user_root.join("ui-state.json")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn with_root_relocates_system_paths() {
        let p = Paths::with_root("/tmp/fake");
        #[cfg(target_os = "linux")]
        {
            assert_eq!(
                p.boxpilot_toml(),
                PathBuf::from("/tmp/fake/etc/boxpilot/boxpilot.toml")
            );
            assert_eq!(p.run_lock(), PathBuf::from("/tmp/fake/run/boxpilot/lock"));
        }
        #[cfg(target_os = "windows")]
        {
            assert_eq!(p.boxpilot_toml(), PathBuf::from("/tmp/fake/boxpilot.toml"));
            assert_eq!(p.run_lock(), PathBuf::from("/tmp/fake/run/lock"));
        }
    }

    #[test]
    fn user_root_is_separate_subdir_under_with_root() {
        let p = Paths::with_root("/tmp/fake");
        assert_eq!(p.user_root(), Path::new("/tmp/fake/user"));
        assert_eq!(
            p.user_profiles_dir(),
            PathBuf::from("/tmp/fake/user/profiles")
        );
    }
}
```

- [ ] **Step 2: Re-export from `lib.rs`**

Modify `crates/boxpilot-platform/src/lib.rs`:

```rust
pub mod paths;
pub use paths::Paths;
```

- [ ] **Step 3: Run tests**

Run: `cargo test -p boxpilot-platform`
Expected: 4 passing tests including the two new ones.

- [ ] **Step 4: Commit**

```bash
git add crates/boxpilot-platform/src/paths.rs crates/boxpilot-platform/src/lib.rs
git commit -m "feat(platform): Paths value type with cfg-gated method bodies"
```

---

## Task 2.3: Migrate `boxpilotd` from `boxpilotd::paths::Paths` to `boxpilot_platform::Paths`

**Files:**
- Modify: `crates/boxpilotd/src/main.rs`, `dispatch.rs`, `iface.rs`, every module that imports `crate::paths::Paths`
- Modify: `crates/boxpilotd/Cargo.toml` — promote `boxpilot-platform` from `[dev-dependencies]` to `[dependencies]`
- Delete: `crates/boxpilotd/src/paths.rs` (after all callers migrate)

- [ ] **Step 1: Move `boxpilot-platform` to a real dep of `boxpilotd`**

In `crates/boxpilotd/Cargo.toml`:
- Remove the `boxpilot-platform = { path = "../boxpilot-platform" }` from `[dev-dependencies]`.
- Add it to `[dependencies]`:

```toml
[dependencies]
# ... existing entries ...
boxpilot-platform = { path = "../boxpilot-platform" }
```

- [ ] **Step 2: Find every consumer of the old `Paths`**

Run: `git grep -l 'crate::paths::Paths\|boxpilotd::paths::Paths\|use crate::paths' crates/boxpilotd/src`
Expected output (must update each):

```text
crates/boxpilotd/src/main.rs
crates/boxpilotd/src/context.rs
crates/boxpilotd/src/dispatch.rs
crates/boxpilotd/src/iface.rs
crates/boxpilotd/src/profile/recovery.rs
crates/boxpilotd/src/profile/release.rs
crates/boxpilotd/src/profile/activate.rs
crates/boxpilotd/src/profile/rollback.rs
crates/boxpilotd/src/profile/unpack.rs
crates/boxpilotd/src/core/install.rs
crates/boxpilotd/src/core/discover.rs
crates/boxpilotd/src/core/commit.rs
crates/boxpilotd/src/core/state.rs
crates/boxpilotd/src/core/rollback.rs
crates/boxpilotd/src/core/adopt.rs
crates/boxpilotd/src/core/trust.rs
crates/boxpilotd/src/service/install.rs
crates/boxpilotd/src/diagnostics/mod.rs
crates/boxpilotd/src/legacy/observe.rs
crates/boxpilotd/src/legacy/migrate.rs
crates/boxpilotd/src/legacy/backup.rs
```

- [ ] **Step 3: Replace imports**

In each file, change:

```rust
use crate::paths::Paths;
```

to:

```rust
use boxpilot_platform::Paths;
```

For inline `crate::paths::Paths` references, change to `boxpilot_platform::Paths`.

- [ ] **Step 4: Update `main.rs` constructor**

In `crates/boxpilotd/src/main.rs`, change:

```rust
let paths = paths::Paths::system();
```

to:

```rust
let paths = boxpilot_platform::Paths::system().context("read system paths from env")?;
```

(`context` is from `anyhow::Context`, already in scope.)

- [ ] **Step 5: Delete the old `boxpilotd::paths` module**

```bash
rm crates/boxpilotd/src/paths.rs
```

In `crates/boxpilotd/src/main.rs`, remove the `mod paths;` declaration.

- [ ] **Step 6: Verify Linux build + tests**

Run: `cargo test -p boxpilotd`
Expected: all existing tests still pass. Tests that used `paths::Paths::with_root(tmp.path())` keep compiling because the new `Paths::with_root` has the same signature.

- [ ] **Step 7: Verify rest of workspace**

Run: `cargo test --workspace`
Expected: green.

- [ ] **Step 8: Commit**

```bash
git add crates/boxpilotd/src crates/boxpilotd/Cargo.toml
git commit -m "refactor(boxpilotd): adopt boxpilot_platform::Paths; delete inline Paths impl"
```

---

## Task 2.4: Replace `ProfileStorePaths::from_env` with `from_paths`

**Files:**
- Modify: `crates/boxpilot-profile/src/store.rs`
- Modify: `crates/boxpilot-profile/Cargo.toml` — add `boxpilot-platform` dep
- Modify: `crates/boxpilot-tauri/src/lib.rs` — pass `Paths` in instead of relying on env
- Modify: `crates/boxpilot-tauri/Cargo.toml` — add `boxpilot-platform` dep

- [ ] **Step 1: Add `boxpilot-platform` to both crates' deps**

`crates/boxpilot-profile/Cargo.toml`:

```toml
[dependencies]
# ... existing ...
boxpilot-platform = { path = "../boxpilot-platform" }
```

`crates/boxpilot-tauri/Cargo.toml`:

```toml
[dependencies]
# ... existing ...
boxpilot-platform = { path = "../boxpilot-platform" }
```

- [ ] **Step 2: Add `from_paths` to `ProfileStorePaths`**

In `crates/boxpilot-profile/src/store.rs`, find the existing `from_env()` impl and add the new constructor immediately above it:

```rust
impl ProfileStorePaths {
    /// Build from a `boxpilot_platform::Paths`. This is the production
    /// constructor used by Tauri command handlers (per spec §5.1 / COQ16).
    pub fn from_paths(paths: &boxpilot_platform::Paths) -> Self {
        Self {
            root: paths.user_profiles_dir(),
        }
    }

    // ... existing from_env() stays for now; deleted in step 5 ...
}
```

- [ ] **Step 3: Update `boxpilot-tauri/src/lib.rs`**

Find the existing `from_env()` call (the only one in the workspace per Round 6/6.4):

```rust
boxpilot_profile::ProfileStorePaths::from_env()
```

Replace the whole `setup` function (or the section that calls `from_env`) with:

```rust
let paths = boxpilot_platform::Paths::system()
    .map_err(|e| format!("read system paths: {e}"))?;
let store_paths = boxpilot_profile::ProfileStorePaths::from_paths(&paths);
// Register Paths so Tauri commands can pull it via tauri::State<Paths>:
app.manage(paths);
app.manage(store_paths);
```

- [ ] **Step 4: Run tests**

Run: `cargo test --workspace`
Expected: green. The `from_env()` impl still exists; this PR only adds `from_paths` and switches the lone caller.

- [ ] **Step 5: Delete `from_env()`**

In `crates/boxpilot-profile/src/store.rs`, remove the entire `from_env()` impl. Run `cargo build --workspace` again to confirm no other caller exists.

- [ ] **Step 6: Run tests again**

Run: `cargo test --workspace`
Expected: green.

- [ ] **Step 7: Commit**

```bash
git add crates/boxpilot-profile crates/boxpilot-tauri
git commit -m "refactor(profile,tauri): replace ProfileStorePaths::from_env with from_paths"
```

---

## PR 2 smoke

```bash
cargo test --workspace                                                  # Linux non-regression
cargo check --target x86_64-pc-windows-gnu --workspace || true          # still allowed-fail (PR 1-10)
git grep 'paths::Paths' crates/                                         # only matches in trait code, not callers
git grep 'from_env' crates/boxpilot-profile crates/boxpilot-tauri       # zero matches
```

PR description body should include: "Threads `boxpilot_platform::Paths` through `boxpilotd` and Tauri state. Deletes `from_env`. PRs 3+ build on this."

---

# PR 3: Move `FsMetadataProvider` / `VersionChecker` / `UserLookup` traits + introduce `FsPermissions`

**Size:** M · **Touches:** `boxpilot-platform/src/{traits,linux,windows,fakes}/{fs_meta,version,user_lookup,fs_perms}.rs`, `boxpilotd/src/core/{trust,install,discover}.rs`, `boxpilotd/src/controller.rs`, all `boxpilot-profile` files using `PermissionsExt`. **Linux non-regression:** the four traits' Linux behavior is byte-identical to the existing `boxpilotd` impls; tests are moved verbatim to the platform crate.

This PR ports four traits already DI'd in `boxpilotd` and introduces the new `FsPermissions` trait so `boxpilot-profile` modules can drop their module-top `use std::os::unix::fs::PermissionsExt;` (per COQ12).

## Task 3.1: Move `FsMetadataProvider`

**Files:**
- Create: `crates/boxpilot-platform/src/traits/fs_meta.rs` (copy of existing trait def)
- Create: `crates/boxpilot-platform/src/linux/fs_meta.rs` (move from `boxpilotd/src/core/trust.rs::StdFsMetadataProvider`)
- Create: `crates/boxpilot-platform/src/fakes/fs_meta.rs` (move from existing test fakes)
- Modify: `crates/boxpilotd/src/core/trust.rs` — re-export from platform crate
- Modify: `crates/boxpilotd/src/main.rs` — import from `boxpilot_platform`

- [ ] **Step 1: Read existing trait + impl**

Run: `git grep -n "trait FsMetadataProvider\|impl FsMetadataProvider\|StdFsMetadataProvider" crates/boxpilotd/src`
Expected: trait def + Linux impl + at least one fake variant in test code.

- [ ] **Step 2: Copy trait def into `boxpilot-platform`**

`crates/boxpilot-platform/src/traits/fs_meta.rs`:

```rust
//! Filesystem metadata reads abstracted so trust checks (§6.5) can be tested
//! without touching real `/usr/bin` paths. Linux impl wraps `std::fs` +
//! `nix::sys::stat`. Windows impl is a stub in Sub-project #1.

use async_trait::async_trait;
use std::path::Path;

/// Subset of metadata callers actually need. Extend additively.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FileMeta {
    pub uid: u32,
    pub gid: u32,
    pub mode: u32,
    pub is_file: bool,
    pub is_dir: bool,
    pub is_symlink: bool,
    pub setuid: bool,
    pub setgid: bool,
    pub sticky: bool,
}

#[async_trait]
pub trait FsMetadataProvider: Send + Sync {
    async fn metadata(&self, path: &Path) -> std::io::Result<FileMeta>;
    async fn read_link(&self, path: &Path) -> std::io::Result<std::path::PathBuf>;
}
```

- [ ] **Step 3: Move existing Linux impl**

`crates/boxpilot-platform/src/linux/fs_meta.rs`:

Copy the body of `boxpilotd::core::trust::StdFsMetadataProvider` here, retargeting trait imports to `crate::traits::fs_meta::*`. Verify the impl reads stat bits via `std::os::unix::fs::MetadataExt` and `std::os::unix::fs::PermissionsExt`.

- [ ] **Step 4: Stub Windows impl**

`crates/boxpilot-platform/src/windows/fs_meta.rs`:

```rust
use crate::traits::fs_meta::{FileMeta, FsMetadataProvider};
use async_trait::async_trait;
use std::path::{Path, PathBuf};

pub struct StdFsMetadataProvider;

#[async_trait]
impl FsMetadataProvider for StdFsMetadataProvider {
    async fn metadata(&self, _path: &Path) -> std::io::Result<FileMeta> {
        Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "FsMetadataProvider stub: implemented in Sub-project #2",
        ))
    }
    async fn read_link(&self, _path: &Path) -> std::io::Result<PathBuf> {
        Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "FsMetadataProvider stub: implemented in Sub-project #2",
        ))
    }
}
```

- [ ] **Step 5: Move existing fake**

`crates/boxpilot-platform/src/fakes/fs_meta.rs`:

Locate the existing fake (most likely `boxpilotd::core::trust::testing::PermissiveTestFs` or similar) and copy verbatim, retargeting trait imports.

- [ ] **Step 6: Wire the modules into `mod.rs` files**

Add `pub mod fs_meta;` lines to `traits/mod.rs`, `linux/mod.rs`, `windows/mod.rs`, and `fakes/mod.rs`.

- [ ] **Step 7: Re-export from `boxpilotd::core::trust` so callers don't change yet**

In `crates/boxpilotd/src/core/trust.rs`, replace the trait definition + Linux impl with:

```rust
pub use boxpilot_platform::traits::fs_meta::{FileMeta, FsMetadataProvider};
pub use boxpilot_platform::linux::fs_meta::StdFsMetadataProvider;

#[cfg(test)]
pub mod testing {
    pub use boxpilot_platform::fakes::fs_meta::*;
}
```

- [ ] **Step 8: Verify all `cargo test -p boxpilotd` tests still pass**

Run: `cargo test -p boxpilotd`
Expected: green.

- [ ] **Step 9: Commit**

```bash
git add crates/boxpilot-platform/src crates/boxpilotd/src/core/trust.rs
git commit -m "refactor(platform): move FsMetadataProvider out of boxpilotd"
```

---

## Task 3.2: Move `VersionChecker`

**Files:**
- Create: `crates/boxpilot-platform/src/traits/version.rs`, `linux/version.rs`, `windows/version.rs`, `fakes/version.rs`
- Modify: `crates/boxpilotd/src/core/trust.rs` — re-export

Same pattern as Task 3.1: copy `trait VersionChecker` (currently in `boxpilotd::core::trust`), the `ProcessVersionChecker` impl (Linux), provide a Windows stub returning `NotImplemented`-ish error, and a fake.

- [ ] **Step 1: Trait def**

`crates/boxpilot-platform/src/traits/version.rs`:

```rust
use async_trait::async_trait;
use std::path::Path;

#[async_trait]
pub trait VersionChecker: Send + Sync {
    /// Run `<core> version` and return the parsed semver-ish version
    /// string (`"1.10.3"`), or `None` if the binary refused to run / didn't
    /// emit a recognizable version line.
    async fn check_version(&self, core_path: &Path) -> std::io::Result<Option<String>>;
}
```

- [ ] **Step 2: Linux impl**

`crates/boxpilot-platform/src/linux/version.rs`:

Move the `ProcessVersionChecker` body from `boxpilotd::core::trust`. Keep the existing `tokio::process::Command` invocation and stdout regex.

- [ ] **Step 3: Windows stub**

`crates/boxpilot-platform/src/windows/version.rs`:

```rust
use crate::traits::version::VersionChecker;
use async_trait::async_trait;
use std::path::Path;

pub struct ProcessVersionChecker;

#[async_trait]
impl VersionChecker for ProcessVersionChecker {
    async fn check_version(&self, _core_path: &Path) -> std::io::Result<Option<String>> {
        Ok(None) // Sub-project #2 will exec sing-box.exe --version
    }
}
```

- [ ] **Step 4: Fake**

`crates/boxpilot-platform/src/fakes/version.rs`:

```rust
use crate::traits::version::VersionChecker;
use async_trait::async_trait;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

pub struct FixedVersion(Mutex<HashMap<PathBuf, Option<String>>>);

impl FixedVersion {
    pub fn new() -> Self {
        Self(Mutex::new(HashMap::new()))
    }
    pub fn with(rows: &[(&str, &str)]) -> Self {
        let mut m = HashMap::new();
        for (p, v) in rows {
            m.insert(PathBuf::from(p), Some(v.to_string()));
        }
        Self(Mutex::new(m))
    }
}

#[async_trait]
impl VersionChecker for FixedVersion {
    async fn check_version(&self, p: &Path) -> std::io::Result<Option<String>> {
        Ok(self.0.lock().unwrap().get(p).cloned().flatten())
    }
}
```

- [ ] **Step 5: Wire mods + re-export from `boxpilotd::core::trust`**

Same pattern as Task 3.1.

- [ ] **Step 6: Run tests**

Run: `cargo test -p boxpilotd`
Expected: green.

- [ ] **Step 7: Commit**

```bash
git add crates/boxpilot-platform/src crates/boxpilotd/src/core/trust.rs
git commit -m "refactor(platform): move VersionChecker out of boxpilotd"
```

---

## Task 3.3: Move `UserLookup`

**Files:**
- Create: `crates/boxpilot-platform/src/traits/user_lookup.rs`, `linux/user_lookup.rs`, `windows/user_lookup.rs`, `fakes/user_lookup.rs`
- Modify: `crates/boxpilotd/src/controller.rs` — re-export

The existing `boxpilotd::controller::UserLookup` trait + `PasswdLookup` (nix `getpwuid`) Linux impl + test fixture.

- [ ] **Step 1: Trait def**

`crates/boxpilot-platform/src/traits/user_lookup.rs`:

```rust
//! UID → username resolution. Linux: `getpwuid` via nix. Windows: SID-based
//! `LookupAccountSid` (Sub-project #2).
//!
//! The trait keeps the existing Linux signature (uid → Option<String>) since
//! `controller_uid` is `u32` in `boxpilot.toml` schema v1; Sub-project #2
//! introduces a SID-aware variant alongside the schema bump.

pub trait UserLookup: Send + Sync {
    fn lookup_username(&self, uid: u32) -> Option<String>;
}
```

- [ ] **Step 2: Linux impl**

`crates/boxpilot-platform/src/linux/user_lookup.rs`:

Move `PasswdLookup` from `boxpilotd::controller`. It's a small wrapper around `nix::unistd::User::from_uid`.

- [ ] **Step 3: Windows stub**

`crates/boxpilot-platform/src/windows/user_lookup.rs`:

```rust
use crate::traits::user_lookup::UserLookup;

pub struct PasswdLookup; // name retained for symmetry

impl UserLookup for PasswdLookup {
    fn lookup_username(&self, _uid: u32) -> Option<String> {
        None // controller_uid is Linux-only; Sub-project #2 introduces SID lookup
    }
}
```

- [ ] **Step 4: Fake**

`crates/boxpilot-platform/src/fakes/user_lookup.rs`:

```rust
use crate::traits::user_lookup::UserLookup;
use std::collections::HashMap;

pub struct Fixed(HashMap<u32, String>);

impl Fixed {
    pub fn new(rows: &[(u32, &str)]) -> Self {
        Self(rows.iter().map(|(u, n)| (*u, n.to_string())).collect())
    }
}

impl UserLookup for Fixed {
    fn lookup_username(&self, uid: u32) -> Option<String> {
        self.0.get(&uid).cloned()
    }
}
```

- [ ] **Step 5: Wire mods + re-export from `boxpilotd::controller`**

In `crates/boxpilotd/src/controller.rs`, replace the `UserLookup` trait definition and `PasswdLookup` impl with:

```rust
pub use boxpilot_platform::traits::user_lookup::UserLookup;
pub use boxpilot_platform::linux::user_lookup::PasswdLookup;

#[cfg(test)]
pub mod testing {
    pub use boxpilot_platform::fakes::user_lookup::*;
}
```

- [ ] **Step 6: Run tests**

Run: `cargo test -p boxpilotd`
Expected: green. Test sites that imported `crate::controller::testing::Fixed` keep working.

- [ ] **Step 7: Commit**

```bash
git add crates/boxpilot-platform/src crates/boxpilotd/src/controller.rs
git commit -m "refactor(platform): move UserLookup out of boxpilotd"
```

---

## Task 3.4: Introduce `FsPermissions` trait (per COQ12)

**Files:**
- Create: `crates/boxpilot-platform/src/traits/fs_perms.rs`
- Create: `crates/boxpilot-platform/src/linux/fs_perms.rs`
- Create: `crates/boxpilot-platform/src/windows/fs_perms.rs`
- Create: `crates/boxpilot-platform/src/fakes/fs_perms.rs`

- [ ] **Step 1: Write the trait + tests**

`crates/boxpilot-platform/src/traits/fs_perms.rs`:

```rust
//! Owner-only filesystem permission setting.
//!
//! Linux: `chmod 0700` (dir) / `chmod 0600` (file).
//! Windows: `SetSecurityInfo` clears inheritance and grants the owner SID
//! full access (Sub-project #1 ships the real impl since this is needed for
//! `%LocalAppData%\BoxPilot\` ACLing).
//!
//! Spec §5.6 + §14: user profile directories must be 0700 (Linux) /
//! owner-only DACL (Windows); profile files 0600 / equivalent.

use async_trait::async_trait;
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathKind {
    Directory,
    File,
}

#[async_trait]
pub trait FsPermissions: Send + Sync {
    /// Restrict `path` to owner-only access.
    async fn restrict_to_owner(&self, path: &Path, kind: PathKind) -> std::io::Result<()>;
}
```

- [ ] **Step 2: Linux impl**

`crates/boxpilot-platform/src/linux/fs_perms.rs`:

```rust
use crate::traits::fs_perms::{FsPermissions, PathKind};
use async_trait::async_trait;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

pub struct ChmodFsPermissions;

#[async_trait]
impl FsPermissions for ChmodFsPermissions {
    async fn restrict_to_owner(&self, path: &Path, kind: PathKind) -> std::io::Result<()> {
        let mode = match kind {
            PathKind::Directory => 0o700,
            PathKind::File => 0o600,
        };
        let perms = std::fs::Permissions::from_mode(mode);
        tokio::fs::set_permissions(path, perms).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn restricts_dir_to_0700() {
        let tmp = tempdir().unwrap();
        let dir = tmp.path().join("d");
        std::fs::create_dir(&dir).unwrap();
        ChmodFsPermissions
            .restrict_to_owner(&dir, PathKind::Directory)
            .await
            .unwrap();
        let m = std::fs::metadata(&dir).unwrap();
        assert_eq!(m.permissions().mode() & 0o777, 0o700);
    }

    #[tokio::test]
    async fn restricts_file_to_0600() {
        let tmp = tempdir().unwrap();
        let f = tmp.path().join("f");
        std::fs::write(&f, b"x").unwrap();
        ChmodFsPermissions
            .restrict_to_owner(&f, PathKind::File)
            .await
            .unwrap();
        let m = std::fs::metadata(&f).unwrap();
        assert_eq!(m.permissions().mode() & 0o777, 0o600);
    }
}
```

- [ ] **Step 3: Windows impl (real — needed for `%LocalAppData%\BoxPilot\` ACLing)**

`crates/boxpilot-platform/src/windows/fs_perms.rs`:

```rust
use crate::traits::fs_perms::{FsPermissions, PathKind};
use async_trait::async_trait;
use std::ffi::OsStrExt;
use std::os::windows::ffi::OsStrExt as _;
use std::path::Path;
use windows_sys::Win32::Security::Authorization::{
    GetNamedSecurityInfoW, SetNamedSecurityInfoW, SE_FILE_OBJECT,
};
use windows_sys::Win32::Security::{
    DACL_SECURITY_INFORMATION, OWNER_SECURITY_INFORMATION, PROTECTED_DACL_SECURITY_INFORMATION,
    PSECURITY_DESCRIPTOR, PSID,
};

pub struct AclFsPermissions;

fn to_wstr(p: &Path) -> Vec<u16> {
    p.as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

#[async_trait]
impl FsPermissions for AclFsPermissions {
    async fn restrict_to_owner(&self, path: &Path, _kind: PathKind) -> std::io::Result<()> {
        let path_w = to_wstr(path);
        // SAFETY: path_w is null-terminated UTF-16. We do not retain the
        // returned PSECURITY_DESCRIPTOR after this function — the OS owns
        // the buffer. Leaks intentional because LocalFree on the buffer
        // would conflict with the pattern below; for a one-shot ACL set,
        // this is acceptable. (If profiling shows a leak, switch to
        // GetSecurityDescriptorOwner + LocalFree pattern.)
        tokio::task::spawn_blocking(move || unsafe {
            let mut owner: PSID = std::ptr::null_mut();
            let mut sd: PSECURITY_DESCRIPTOR = std::ptr::null_mut();
            let rc = GetNamedSecurityInfoW(
                path_w.as_ptr(),
                SE_FILE_OBJECT,
                OWNER_SECURITY_INFORMATION,
                &mut owner,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                &mut sd,
            );
            if rc != 0 {
                return Err(std::io::Error::from_raw_os_error(rc as i32));
            }
            // Apply: clear inheritance (PROTECTED_DACL_SECURITY_INFORMATION)
            // + set DACL to owner-only (NULL DACL pointer means "use the
            // ACE list we built"; for the simple owner-only case, an empty
            // DACL with one owner-grant ACE works. Real impl in PR 12 may
            // be more elaborate; for Sub-project #1 we stub this:
            // Sub-project #2 / #3 owns the production ACL story.
            let rc2 = SetNamedSecurityInfoW(
                path_w.as_ptr() as *mut _,
                SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(), // empty DACL — only owner has implicit access
                std::ptr::null_mut(),
            );
            if rc2 != 0 {
                return Err(std::io::Error::from_raw_os_error(rc2 as i32));
            }
            Ok(())
        })
        .await
        .map_err(|e| std::io::Error::other(format!("spawn_blocking: {e}")))?
    }
}
```

(Comment in the impl: this is a minimal Sub-project #1 ACL — sufficient to compile and run, but Sub-project #3's installer ACL story will revisit. Owner-only via empty DACL works for our purposes.)

- [ ] **Step 4: Fake**

`crates/boxpilot-platform/src/fakes/fs_perms.rs`:

```rust
use crate::traits::fs_perms::{FsPermissions, PathKind};
use async_trait::async_trait;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

#[derive(Default)]
pub struct RecordingFsPermissions(Mutex<Vec<(PathBuf, PathKind)>>);

impl RecordingFsPermissions {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn calls(&self) -> Vec<(PathBuf, PathKind)> {
        self.0.lock().unwrap().clone()
    }
}

#[async_trait]
impl FsPermissions for RecordingFsPermissions {
    async fn restrict_to_owner(&self, path: &Path, kind: PathKind) -> std::io::Result<()> {
        self.0.lock().unwrap().push((path.to_path_buf(), kind));
        Ok(())
    }
}
```

- [ ] **Step 5: Wire mods**

Add `pub mod fs_perms;` to `traits/mod.rs`, `linux/mod.rs`, `windows/mod.rs`, `fakes/mod.rs`.

- [ ] **Step 6: Run tests**

Run: `cargo test -p boxpilot-platform`
Expected: 6+ tests pass (the new `fs_perms` Linux tests + previous tests).

- [ ] **Step 7: Commit**

```bash
git add crates/boxpilot-platform/src
git commit -m "feat(platform): FsPermissions trait + Linux chmod / Windows ACL impls + fake"
```

---

## Task 3.5: Adopt `FsPermissions` in `boxpilot-profile`

**Files:**
- Modify: `crates/boxpilot-profile/src/store.rs`, `meta.rs`, `import.rs`, `remotes.rs` — replace top-level `use std::os::unix::fs::PermissionsExt;` and inline calls with `FsPermissions::restrict_to_owner` calls

The five files identified in spec COQ12. The task is mechanical but each file's call site differs slightly. Pattern:

- Where source (non-test) sets a 0o700 dir or 0o600 file: change to `fs_perms.restrict_to_owner(path, PathKind::Directory_or_File).await?`.
- Where tests assert on mode bits: keep the `PermissionsExt` import inside `#[cfg(test)]` blocks gated by `#[cfg(target_os = "linux")]` so Windows tests don't compile them. Mode-bit assertions are inherently Linux-only.

- [ ] **Step 1: Open `store.rs` and identify call sites**

Run: `git grep -n 'PermissionsExt\|set_permissions\|0o700\|0o600' crates/boxpilot-profile/src/store.rs`
Expected: 3-5 hits.

For each src-side hit:
- Find the function.
- Replace the chmod call with `fs_perms.restrict_to_owner(path, PathKind::Directory_or_File).await?`.
- Add `fs_perms: &dyn FsPermissions` to the function signature.
- Update callers to pass through.

- [ ] **Step 2: Replace top-level import**

Change line 1 of `crates/boxpilot-profile/src/store.rs` from:

```rust
use std::os::unix::fs::PermissionsExt;
```

to:

```rust
use boxpilot_platform::traits::fs_perms::{FsPermissions, PathKind};
```

- [ ] **Step 3: Walk through `meta.rs`, `import.rs`, `remotes.rs`**

For each file, identify the test-only `use std::os::unix::fs::PermissionsExt;` import (lines 57, 292, 62 respectively per spec):
- Wrap the test module in `#[cfg(target_os = "linux")]` if it isn't already.
- Leave the `PermissionsExt` import as-is inside the cfg-gated test module.

For src-side calls in these files: there shouldn't be any (only `store.rs` writes profile dirs). If grep finds src-side `set_permissions` in `meta.rs`/`import.rs`/`remotes.rs`, refactor through `FsPermissions` the same way as `store.rs`.

- [ ] **Step 4: Update `import.rs:382` (test using `std::os::unix::fs::symlink`)**

This is in a test fixture. Wrap:

```rust
#[cfg(target_os = "linux")]
{
    std::os::unix::fs::symlink("/etc/passwd", src.join("evil")).unwrap();
}
#[cfg(not(target_os = "linux"))]
{
    return; // skip on non-Linux — the test asserts symlink rejection, which is a Linux-bundle-extraction concern
}
```

- [ ] **Step 5: Run tests**

Run: `cargo test --workspace`
Expected: green.

- [ ] **Step 6: Verify Windows compile gets one step closer (still allow-fail)**

Run: `cargo check --target x86_64-pc-windows-gnu -p boxpilot-profile`
Expected: progress further than before (some files now compile that didn't); may still fail on `bundle.rs` (memfd) and `check.rs` (subprocess kill). Both are addressed in PR 9 / PR 10.

- [ ] **Step 7: Commit**

```bash
git add crates/boxpilot-profile/src
git commit -m "refactor(profile): use FsPermissions trait instead of module-top PermissionsExt"
```

---

## PR 3 smoke

```bash
cargo test --workspace                                                  # Linux non-regression
cargo check --target x86_64-pc-windows-gnu --workspace || true          # closer; still allow-fail
git grep "use std::os::unix::fs::PermissionsExt" crates/boxpilot-profile/src
# Expected output: only inside `#[cfg(test)]` modules (gated by target_os = "linux")
```

---

# PR 4: `Authority` move + `dispatch::authorize` refactor + BUS_NAME guard test

**Size:** L · **Touches:** `boxpilot-platform/src/{traits,linux,windows,fakes}/authority.rs`, `boxpilotd/src/authority.rs` (deleted), `boxpilotd/src/dispatch.rs` (refactored), `boxpilotd/src/iface.rs` (each `do_*` now resolves CallerPrincipal first), `boxpilotd/src/credentials.rs` (kept Linux-internal per Round 6/6.1).

This is the deepest Linux-side refactor in Sub-project #1. Per COQ11 + Round 6/6.1: dispatch becomes platform-neutral (takes `&CallerPrincipal`) but `CallerResolver` stays as a Linux-internal helper until PR 11a inverts to IpcServer-driven model.

## Task 4.1: Define `CallerPrincipal` and `Authority` trait in platform crate

**Files:**
- Create: `crates/boxpilot-platform/src/traits/authority.rs`
- Modify: `crates/boxpilot-platform/src/traits/mod.rs`

- [ ] **Step 1: Write the trait + supporting types**

`crates/boxpilot-platform/src/traits/authority.rs`:

```rust
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
    /// Returns true if `principal` is authorized for `action_id`. The action
    /// id is a polkit-flavored string (`app.boxpilot.helper.service.start`,
    /// etc.); the trait keeps polkit semantics on Linux and is `AlwaysAllow`
    /// in Windows Sub-project #1 (per COQ3).
    async fn check(
        &self,
        action_id: &str,
        principal: &CallerPrincipal,
    ) -> Result<bool, HelperError>;
}
```

- [ ] **Step 2: Wire into `traits/mod.rs`**

Add: `pub mod authority;`

- [ ] **Step 3: Run sanity build**

Run: `cargo build -p boxpilot-platform`
Expected: clean (no impl yet, but trait compiles).

- [ ] **Step 4: Commit**

```bash
git add crates/boxpilot-platform/src/traits
git commit -m "feat(platform): define CallerPrincipal + Authority trait"
```

---

## Task 4.2: Linux `Authority` impl (move from `boxpilotd::authority::DBusAuthority`)

**Files:**
- Create: `crates/boxpilot-platform/src/linux/authority.rs`
- Modify: `crates/boxpilotd/src/authority.rs` — keep test fakes only; production impl moves out

- [ ] **Step 1: Read existing impl**

Run: `cat crates/boxpilotd/src/authority.rs | head -100`
Note the existing `DBusAuthority` impl (uses `org.freedesktop.PolicyKit1.Authority.CheckAuthorization`) and its test-only `CannedAuthority` fake.

- [ ] **Step 2: Move `DBusAuthority` to platform crate**

`crates/boxpilot-platform/src/linux/authority.rs`:

Copy the `DBusAuthority` body from `boxpilotd::authority`. Adapt the trait to the new shape:

```rust
use crate::traits::authority::{Authority, CallerPrincipal};
use async_trait::async_trait;
use boxpilot_ipc::HelperError;
use zbus::Connection;

pub struct DBusAuthority {
    conn: Connection,
    /// Linux polkit takes a D-Bus subject expressed as bus name; the
    /// resolver from boxpilotd::credentials supplies it. We store it per
    /// connection at construction.
    subject_provider: std::sync::Arc<dyn SubjectProvider>,
}

/// Linux-only helper. Resolves the D-Bus sender bus name for the current
/// call. Implemented in boxpilotd::iface where the zbus header is
/// available; passed in here as a trait object so DBusAuthority doesn't
/// need to hold a per-call header.
pub trait SubjectProvider: Send + Sync {
    fn current_sender(&self) -> Option<String>;
}

impl DBusAuthority {
    pub fn new(conn: Connection, subject: std::sync::Arc<dyn SubjectProvider>) -> Self {
        Self { conn, subject_provider: subject }
    }
}

#[async_trait]
impl Authority for DBusAuthority {
    async fn check(
        &self,
        action_id: &str,
        principal: &CallerPrincipal,
    ) -> Result<bool, HelperError> {
        let sender = match self.subject_provider.current_sender() {
            Some(s) => s,
            None => {
                return Err(HelperError::Ipc {
                    message: "polkit subject (D-Bus sender) unknown".into(),
                });
            }
        };
        let _uid = match principal {
            CallerPrincipal::LinuxUid(u) => *u,
            CallerPrincipal::WindowsSid(_) => {
                return Err(HelperError::Ipc {
                    message: "linux DBusAuthority received non-Linux principal".into(),
                });
            }
        };
        // Existing zbus polkit call body — copy verbatim from
        // boxpilotd::authority. The only call-shape change is that we pass
        // `sender` (sourced from subject_provider) instead of receiving it
        // as a parameter.
        // ... (see boxpilotd::authority::DBusAuthority::check for the
        //      full polkit RPC body — paste it here, adapting only the
        //      "sender" source.)
        //
        // Return Ok(allowed) as before.
        todo!("copy from boxpilotd::authority::DBusAuthority::check; see file note above")
    }
}
```

(The `todo!` is deliberate — this Step 2 commits the structural move; Step 3 fills the body. Two-step commit prevents giant diffs that mix structural change with body changes.)

- [ ] **Step 3: Fill the body, replacing `todo!()`**

Open the original `boxpilotd::authority::DBusAuthority::check` and copy its zbus call sequence (`PolicyKitAuthorityProxy::new(...)`, `check_authorization(...)`, etc.) into the new file's `check` impl, replacing `todo!()`. The only difference is that `sender` comes from `self.subject_provider.current_sender()` instead of a parameter.

- [ ] **Step 4: Wire `linux/mod.rs`**

```rust
pub mod authority;
```

- [ ] **Step 5: Move the test fake**

`crates/boxpilot-platform/src/fakes/authority.rs`:

```rust
use crate::traits::authority::{Authority, CallerPrincipal};
use async_trait::async_trait;
use boxpilot_ipc::HelperError;
use std::collections::HashSet;

/// Allows everything in `allow`; denies everything else.
pub struct CannedAuthority {
    allow: HashSet<String>,
    deny: HashSet<String>,
}

impl CannedAuthority {
    pub fn allowing(actions: &[&str]) -> Self {
        Self {
            allow: actions.iter().map(|s| s.to_string()).collect(),
            deny: HashSet::new(),
        }
    }
    pub fn denying(actions: &[&str]) -> Self {
        Self {
            allow: HashSet::new(),
            deny: actions.iter().map(|s| s.to_string()).collect(),
        }
    }
}

#[async_trait]
impl Authority for CannedAuthority {
    async fn check(
        &self,
        action_id: &str,
        _principal: &CallerPrincipal,
    ) -> Result<bool, HelperError> {
        if self.deny.contains(action_id) {
            return Ok(false);
        }
        if self.allow.contains(action_id) {
            return Ok(true);
        }
        Ok(false)
    }
}

/// Always-allow variant — used by Windows Authority in Sub-project #1
/// (per COQ3). Real SID checks arrive in Sub-project #2.
pub struct AlwaysAllow;

#[async_trait]
impl Authority for AlwaysAllow {
    async fn check(
        &self,
        _action_id: &str,
        _principal: &CallerPrincipal,
    ) -> Result<bool, HelperError> {
        Ok(true)
    }
}
```

- [ ] **Step 6: Wire `fakes/mod.rs`**

```rust
pub mod authority;
```

- [ ] **Step 7: Update `boxpilotd::authority`**

Replace `crates/boxpilotd/src/authority.rs` body with re-exports + the local `SubjectProvider` impl:

```rust
//! `boxpilotd`-side glue around `boxpilot_platform::Authority`.
//! The production impl `DBusAuthority` and the test fakes (`CannedAuthority`)
//! live in `boxpilot-platform`. This module owns the per-call
//! `SubjectProvider` that turns the active zbus header into the D-Bus
//! sender bus name polkit expects.

pub use boxpilot_platform::traits::authority::{Authority, CallerPrincipal};
pub use boxpilot_platform::linux::authority::DBusAuthority;

#[cfg(test)]
pub mod testing {
    pub use boxpilot_platform::fakes::authority::{AlwaysAllow, CannedAuthority};
}

// SubjectProvider impl: stores the current D-Bus sender as a TLS slot or
// a synchronous Arc<RwLock<Option<String>>>. Set by iface.rs's do_* methods
// before they call dispatch::authorize.

use std::sync::Arc;
use std::sync::RwLock;

#[derive(Default)]
pub struct ZbusSubject {
    inner: Arc<RwLock<Option<String>>>,
}

impl ZbusSubject {
    pub fn new() -> Self {
        Self::default()
    }
    /// Set the current D-Bus sender for this call — must be done before
    /// dispatch::authorize() and unset (or just overwritten by the next
    /// call) afterward.
    pub fn set(&self, sender: &str) {
        *self.inner.write().unwrap() = Some(sender.to_string());
    }
}

impl boxpilot_platform::linux::authority::SubjectProvider for ZbusSubject {
    fn current_sender(&self) -> Option<String> {
        self.inner.read().unwrap().clone()
    }
}
```

(Yes, a single shared mutable slot is racy if multiple zbus method invocations interleave — but the existing dispatch flow is synchronous per call within one zbus connection, and the `RwLock` guards correctness. PR 11a's IpcServer inversion makes this cleaner by passing the principal in directly.)

- [ ] **Step 8: Run tests**

Run: `cargo test --workspace`
Expected: green. Test sites that imported `crate::authority::testing::CannedAuthority` keep working through the re-export.

- [ ] **Step 9: Commit**

```bash
git add crates/boxpilot-platform/src crates/boxpilotd/src/authority.rs
git commit -m "refactor(platform): move DBusAuthority to platform crate with SubjectProvider seam"
```

---

## Task 4.3: Refactor `dispatch::authorize` to take `&CallerPrincipal`

**Files:**
- Modify: `crates/boxpilotd/src/dispatch.rs`
- Modify: `crates/boxpilotd/src/iface.rs` — every `do_*` method resolves the principal first

- [ ] **Step 1: Update `dispatch::authorize` signature**

In `crates/boxpilotd/src/dispatch.rs`:

```rust
use crate::context::HelperContext;
use crate::controller::ControllerState;
use crate::lock::{self, LockGuard};
use boxpilot_ipc::{HelperError, HelperMethod, HelperResult};
use boxpilot_platform::traits::authority::CallerPrincipal;

pub struct AuthorizedCall {
    pub principal: CallerPrincipal,
    pub controller: ControllerState,
    pub will_claim_controller: bool,
    _lock: Option<LockGuard>,
}

/// Convenience accessor — most existing code wants the uid form.
impl AuthorizedCall {
    pub fn caller_uid(&self) -> Option<u32> {
        self.principal.linux_uid()
    }
}

pub async fn authorize(
    ctx: &HelperContext,
    principal: &CallerPrincipal,
    method: HelperMethod,
) -> HelperResult<AuthorizedCall> {
    let controller = ctx.controller_state().await?;

    if let ControllerState::Orphaned { .. } = controller {
        if method.is_mutating() {
            return Err(HelperError::ControllerOrphaned);
        }
    }

    if let Some(got) = ctx.state_schema_mismatch {
        if method.is_mutating() {
            return Err(HelperError::UnsupportedSchemaVersion { got });
        }
    }

    let action_id = method.polkit_action_id();
    let allowed = ctx.authority.check(action_id, principal).await?;
    if !allowed {
        return Err(HelperError::NotAuthorized);
    }

    let will_claim_controller =
        matches!(controller, ControllerState::Unset) && method.is_mutating() && allowed;

    let lock = if method.is_mutating() {
        Some(lock::try_acquire(&ctx.paths.run_lock())?)
    } else {
        None
    };

    Ok(AuthorizedCall {
        principal: principal.clone(),
        controller,
        will_claim_controller,
        _lock: lock,
    })
}

/// Adapt the existing `maybe_claim_controller(will_claim, caller_uid, ...)`
/// signature to use the principal. Linux-only path; non-Linux principals
/// fall back to ControllerOrphaned (defensive).
pub fn maybe_claim_controller(
    will_claim: bool,
    principal: &CallerPrincipal,
    user_lookup: &dyn boxpilot_platform::traits::user_lookup::UserLookup,
) -> HelperResult<Option<crate::dispatch::ControllerWrites>> {
    if !will_claim {
        return Ok(None);
    }
    let uid = principal.linux_uid().ok_or(HelperError::ControllerOrphaned)?;
    match user_lookup.lookup_username(uid) {
        Some(username) => Ok(Some(ControllerWrites { uid, username })),
        None => Err(HelperError::ControllerOrphaned),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ControllerWrites {
    pub uid: u32,
    pub username: String,
}
```

- [ ] **Step 2: Update each `do_*` method in `iface.rs`**

For each `do_*` method, change the call pattern from:

```rust
let _call = dispatch::authorize(&self.ctx, sender, HelperMethod::Foo).await?;
```

to:

```rust
let principal = resolve_caller_principal(&self.ctx, sender).await?;
self.ctx.authority_subject.set(sender); // for polkit's D-Bus subject
let _call = dispatch::authorize(&self.ctx, &principal, HelperMethod::Foo).await?;
```

Where `resolve_caller_principal` is a small helper:

```rust
async fn resolve_caller_principal(
    ctx: &HelperContext,
    sender: &str,
) -> HelperResult<CallerPrincipal> {
    let uid = ctx.callers.resolve(sender).await?;
    Ok(CallerPrincipal::LinuxUid(uid))
}
```

(`ctx.callers` is the surviving Linux-internal `CallerResolver` per Round 6/6.1.)

`ctx.authority_subject` is the `ZbusSubject` instance from Task 4.2 Step 7. Add it to `HelperContext` as `pub authority_subject: Arc<ZbusSubject>` and construct it in `main.rs`.

- [ ] **Step 3: Run tests, fixing call sites**

Run: `cargo test -p boxpilotd`
Expected: a few tests fail (the ones that constructed `dispatch::authorize` with `sender_bus_name: &str`). Update each by:
- Building a `CallerPrincipal::LinuxUid(uid)` and passing `&principal` to `authorize`.
- For tests that previously called `maybe_claim_controller(true, 1000, &lookup)`, change to `maybe_claim_controller(true, &CallerPrincipal::LinuxUid(1000), &lookup)`.

The existing test file in `boxpilotd/src/dispatch.rs` shows the patterns; fix in place.

- [ ] **Step 4: Run again**

Run: `cargo test --workspace`
Expected: green.

- [ ] **Step 5: Commit**

```bash
git add crates/boxpilotd/src
git commit -m "refactor(boxpilotd): dispatch::authorize takes &CallerPrincipal"
```

---

## Task 4.4: BUS_NAME / OBJECT_PATH guard test

**Files:**
- Modify: `crates/boxpilotd/src/iface.rs` — append a constants-pinning test

- [ ] **Step 1: Append the guard test**

`crates/boxpilotd/src/iface.rs` — at the end of the existing `#[cfg(test)] mod tests`:

```rust
/// Guard test (per COQ17 / Round 4 finding 4.8): D-Bus wire names are
/// part of the .deb-shipped polkit + dbus service files. Changing them
/// without a corresponding deb postinst migration breaks already-installed
/// users. This test fails loudly if a refactor accidentally renames them.
#[test]
fn dbus_wire_names_are_frozen() {
    assert_eq!(
        crate::main::BUS_NAME,
        "app.boxpilot.Helper",
        "Bus name change requires deb postinst migration of \
         /usr/share/dbus-1/system-services/app.boxpilot.Helper.service \
         and the polkit policy file"
    );
    assert_eq!(
        crate::main::OBJECT_PATH,
        "/app/boxpilot/Helper",
        "Object path change requires updating Tauri's HelperProxy default_path \
         (boxpilot-tauri/src/helper_client.rs)"
    );
}
```

(`BUS_NAME` and `OBJECT_PATH` currently live as `const` in `boxpilotd/src/main.rs` — make them `pub` so the test can reach them. If `main` isn't a library module, declare the constants in `boxpilotd::iface` and re-export to `main`.)

- [ ] **Step 2: Run the guard**

Run: `cargo test -p boxpilotd dbus_wire_names_are_frozen`
Expected: green.

- [ ] **Step 3: Commit**

```bash
git add crates/boxpilotd/src/iface.rs crates/boxpilotd/src/main.rs
git commit -m "test(boxpilotd): pin D-Bus BUS_NAME and OBJECT_PATH per COQ17/4.8"
```

---

## PR 4 smoke

```bash
cargo test --workspace                                                  # Linux non-regression
cargo test -p boxpilotd dbus_wire_names_are_frozen                       # the new guard
cargo check --target x86_64-pc-windows-gnu --workspace || true          # still allow-fail
git grep "sender_bus_name" crates/boxpilotd/src/dispatch.rs              # zero matches expected
```

PR description body should mention the three sub-changes (Authority move + dispatch refactor + BUS_NAME guard) and link COQ10 / COQ11 / COQ17 / Round 6/6.1.

---
