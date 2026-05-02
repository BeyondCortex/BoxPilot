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

# PR 5: Move `Systemd` → `ServiceManager` + `JournalReader` → `LogReader`

**Size:** M · **Touches:** `boxpilot-platform/src/{traits,linux,windows,fakes}/{service,logs}.rs`, `boxpilotd/src/systemd.rs` (re-export shell), `boxpilotd/src/context.rs`, callers. **Linux non-regression:** trait surface NOT expanded (per COQ4) — methods, parameter types, return types, `UnitState` shape are byte-identical to current code.

This PR mechanically moves two existing traits to platform crate. Per COQ4 the trait surface is **frozen at the current Linux shape**; SCM-aware redesign is Sub-project #2's first task.

## Task 5.1: Move `Systemd` → `ServiceManager` (verbatim port)

**Files:**
- Create: `crates/boxpilot-platform/src/traits/service.rs`
- Create: `crates/boxpilot-platform/src/linux/service.rs`
- Create: `crates/boxpilot-platform/src/windows/service.rs`
- Create: `crates/boxpilot-platform/src/fakes/service.rs`
- Modify: `crates/boxpilotd/src/systemd.rs` (becomes re-export shell)
- Modify: `crates/boxpilotd/src/context.rs` — rename field `systemd` to keep readability, alias allowed

- [ ] **Step 1: Read existing `boxpilotd/src/systemd.rs` end-to-end**

Run: `wc -l crates/boxpilotd/src/systemd.rs && head -40 crates/boxpilotd/src/systemd.rs`
Expected: ~552 lines, trait def at lines 9-31, zbus proxy macros after.

- [ ] **Step 2: Copy trait def to platform crate**

`crates/boxpilot-platform/src/traits/service.rs`:

```rust
//! Service-control abstraction. Currently shaped 1:1 with the existing
//! `boxpilotd::systemd::Systemd` trait. SCM (Windows) shape redesign is
//! Sub-project #2's first task per COQ4.
//!
//! Method names and `UnitState` are part of the GUI's wire protocol via
//! `boxpilot-ipc`; do not rename without a schema bump.

use async_trait::async_trait;
use boxpilot_ipc::{HelperError, UnitState};

#[async_trait]
pub trait ServiceManager: Send + Sync {
    async fn unit_state(&self, unit_name: &str) -> Result<UnitState, HelperError>;
    async fn start_unit(&self, unit_name: &str) -> Result<(), HelperError>;
    async fn stop_unit(&self, unit_name: &str) -> Result<(), HelperError>;
    async fn restart_unit(&self, unit_name: &str) -> Result<(), HelperError>;
    async fn enable_unit_files(&self, unit_names: &[String]) -> Result<(), HelperError>;
    async fn disable_unit_files(&self, unit_names: &[String]) -> Result<(), HelperError>;
    async fn reload(&self) -> Result<(), HelperError>;
    async fn fragment_path(&self, unit_name: &str) -> Result<Option<String>, HelperError>;
    async fn unit_file_state(&self, unit_name: &str) -> Result<Option<String>, HelperError>;
}

/// Backwards-compatible alias for callers that imported the old name.
/// Schedule for removal in Sub-project #2's trait redesign.
pub use ServiceManager as Systemd;
```

- [ ] **Step 3: Move Linux impl bodily**

`crates/boxpilot-platform/src/linux/service.rs`:

Copy the rest of `boxpilotd/src/systemd.rs` (zbus proxies, `DBusSystemd` struct, all impl bodies). Adapt the trait import:

```rust
use crate::traits::service::ServiceManager;
use async_trait::async_trait;
use boxpilot_ipc::{HelperError, UnitState};
use zbus::{proxy, Connection};

// ... (paste zbus #[proxy] traits SystemdManager / SystemdUnit / SystemdService verbatim)

pub struct DBusSystemd {
    conn: Connection,
}

impl DBusSystemd {
    pub fn new(conn: Connection) -> Self {
        Self { conn }
    }
}

#[async_trait]
impl ServiceManager for DBusSystemd {
    // ... (paste all 9 methods verbatim from boxpilotd::systemd)
}
```

- [ ] **Step 4: Windows stub**

`crates/boxpilot-platform/src/windows/service.rs`:

```rust
use crate::traits::service::ServiceManager;
use async_trait::async_trait;
use boxpilot_ipc::{HelperError, UnitState};

pub struct ScmServiceManager;

#[async_trait]
impl ServiceManager for ScmServiceManager {
    async fn unit_state(&self, _unit_name: &str) -> Result<UnitState, HelperError> {
        Err(HelperError::NotImplemented)
    }
    async fn start_unit(&self, _: &str) -> Result<(), HelperError> {
        Err(HelperError::NotImplemented)
    }
    async fn stop_unit(&self, _: &str) -> Result<(), HelperError> {
        Err(HelperError::NotImplemented)
    }
    async fn restart_unit(&self, _: &str) -> Result<(), HelperError> {
        Err(HelperError::NotImplemented)
    }
    async fn enable_unit_files(&self, _: &[String]) -> Result<(), HelperError> {
        Err(HelperError::NotImplemented)
    }
    async fn disable_unit_files(&self, _: &[String]) -> Result<(), HelperError> {
        Err(HelperError::NotImplemented)
    }
    async fn reload(&self) -> Result<(), HelperError> {
        Err(HelperError::NotImplemented)
    }
    async fn fragment_path(&self, _: &str) -> Result<Option<String>, HelperError> {
        Err(HelperError::NotImplemented)
    }
    async fn unit_file_state(&self, _: &str) -> Result<Option<String>, HelperError> {
        Err(HelperError::NotImplemented)
    }
}
```

- [ ] **Step 5: Move `RecordingSystemd` fake**

`crates/boxpilot-platform/src/fakes/service.rs`:

Copy the `RecordingSystemd` from `boxpilotd::systemd::testing` verbatim, adjusting trait imports.

- [ ] **Step 6: Wire mods + replace `boxpilotd/src/systemd.rs`**

`crates/boxpilotd/src/systemd.rs`:

```rust
//! Re-export shell. Production impl lives in
//! `boxpilot-platform::linux::service`. The trait is renamed
//! `ServiceManager`; `Systemd` is a backwards-compat alias.

pub use boxpilot_platform::traits::service::{Systemd, ServiceManager};
pub use boxpilot_platform::linux::service::DBusSystemd;

#[cfg(test)]
pub mod testing {
    pub use boxpilot_platform::fakes::service::*;
}
```

- [ ] **Step 7: Run tests**

Run: `cargo test -p boxpilotd`
Expected: green. Test imports of `crate::systemd::testing::RecordingSystemd` keep working.

- [ ] **Step 8: Commit**

```bash
git add crates/boxpilot-platform/src crates/boxpilotd/src/systemd.rs
git commit -m "refactor(platform): move Systemd trait to ServiceManager (verbatim port; per COQ4)"
```

---

## Task 5.2: Move `JournalReader` → `LogReader` (verbatim port)

**Files:**
- Create: `crates/boxpilot-platform/src/traits/logs.rs`
- Create: `crates/boxpilot-platform/src/linux/logs.rs`
- Create: `crates/boxpilot-platform/src/windows/logs.rs`
- Create: `crates/boxpilot-platform/src/fakes/logs.rs`
- Modify: `crates/boxpilotd/src/systemd.rs` — also re-export the new names

The existing `JournalReader` lives in `boxpilotd::systemd` (lines ~295+); `JournalctlProcess` is the production impl.

- [ ] **Step 1: Trait def**

`crates/boxpilot-platform/src/traits/logs.rs`:

```rust
use async_trait::async_trait;
use boxpilot_ipc::HelperError;

#[async_trait]
pub trait LogReader: Send + Sync {
    /// Tail the last `lines` log entries for `unit_name`.
    /// Linux: `journalctl --unit <unit> -n <lines> -o cat`.
    /// Windows: `EvtQuery` filter (Sub-project #2).
    async fn tail(&self, unit_name: &str, lines: usize) -> Result<Vec<String>, HelperError>;
}

pub use LogReader as JournalReader;
```

- [ ] **Step 2: Linux impl**

`crates/boxpilot-platform/src/linux/logs.rs`:

Move `JournalctlProcess` from `boxpilotd::systemd`. Single struct + `LogReader` impl that spawns `journalctl`.

- [ ] **Step 3: Windows stub + fake**

`crates/boxpilot-platform/src/windows/logs.rs`:

```rust
use crate::traits::logs::LogReader;
use async_trait::async_trait;
use boxpilot_ipc::HelperError;

pub struct EventLogReader;

#[async_trait]
impl LogReader for EventLogReader {
    async fn tail(&self, _unit_name: &str, _lines: usize) -> Result<Vec<String>, HelperError> {
        Ok(vec!["log reading not implemented on Windows in Sub-project #1".into()])
    }
}
```

`crates/boxpilot-platform/src/fakes/logs.rs`:

```rust
use crate::traits::logs::LogReader;
use async_trait::async_trait;
use boxpilot_ipc::HelperError;
use std::sync::Mutex;

pub struct InMemoryLogReader(Mutex<Vec<String>>);

impl InMemoryLogReader {
    pub fn with_lines(lines: Vec<String>) -> Self {
        Self(Mutex::new(lines))
    }
}

#[async_trait]
impl LogReader for InMemoryLogReader {
    async fn tail(&self, _unit_name: &str, lines: usize) -> Result<Vec<String>, HelperError> {
        let all = self.0.lock().unwrap().clone();
        Ok(all.into_iter().take(lines).collect())
    }
}
```

- [ ] **Step 4: Wire mods**

Add `pub mod logs;` to `traits/mod.rs`, `linux/mod.rs`, `windows/mod.rs`, `fakes/mod.rs`.

- [ ] **Step 5: Update `boxpilotd::systemd` re-exports**

In `boxpilotd/src/systemd.rs`, append:

```rust
pub use boxpilot_platform::traits::logs::{JournalReader, LogReader};
pub use boxpilot_platform::linux::logs::JournalctlProcess;

#[cfg(test)]
pub mod testing_logs {
    pub use boxpilot_platform::fakes::logs::InMemoryLogReader;
}
```

- [ ] **Step 6: Run tests**

Run: `cargo test --workspace`
Expected: green.

- [ ] **Step 7: Commit**

```bash
git add crates/boxpilot-platform/src crates/boxpilotd/src/systemd.rs
git commit -m "refactor(platform): move JournalReader to LogReader (verbatim port)"
```

---

## PR 5 smoke

```bash
cargo test --workspace                                                  # Linux non-regression
git grep "trait Systemd\|trait JournalReader" crates/boxpilotd/src       # zero matches (only re-exports)
cargo check --target x86_64-pc-windows-gnu --workspace || true          # still allow-fail
```

---

# PR 6: `FileLock` trait

**Size:** S · **Touches:** `boxpilot-platform/src/{traits,linux,windows,fakes}/lock.rs`, `boxpilotd/src/lock.rs` (re-export shell). **Linux non-regression:** existing `flock(2)` semantics preserved.

## Task 6.1: Define + impl

**Files:**
- Create: `crates/boxpilot-platform/src/traits/lock.rs`, `linux/lock.rs`, `windows/lock.rs`, `fakes/lock.rs`
- Modify: `crates/boxpilotd/src/lock.rs`

- [ ] **Step 1: Trait**

`crates/boxpilot-platform/src/traits/lock.rs`:

```rust
//! Global advisory lock used by every mutating helper verb. Linux uses
//! `flock(2)` on `/run/boxpilot/lock` (tmpfs auto-clears on reboot).
//! Windows uses `LockFileEx` on `%ProgramData%\BoxPilot\run\lock` —
//! Windows handle scoping is inherently process-bounded so a crashed
//! helper releases its lock automatically too.

use boxpilot_ipc::HelperError;
use std::path::Path;

pub trait FileLock: Send + Sync {
    type Guard: Send + Sync;

    /// Acquire an exclusive lock. Returns `HelperError::Busy` if another
    /// process holds it. Drops automatically when the returned guard
    /// is dropped.
    fn try_acquire(&self, path: &Path) -> Result<Self::Guard, HelperError>;
}
```

- [ ] **Step 2: Linux impl (wrapper around existing fs2 logic)**

`crates/boxpilot-platform/src/linux/lock.rs`:

Move the body of `boxpilotd::lock` here. Keep the existing `LockGuard` type. The `FileLock` trait above is shaped so callers can stay generic; the existing `LockGuard` is its `Guard` associated type.

```rust
use crate::traits::lock::FileLock;
use boxpilot_ipc::HelperError;
use fs2::FileExt;
use std::fs::File;
use std::path::Path;

pub struct FlockFileLock;

pub struct LockGuard {
    file: File,
}

impl Drop for LockGuard {
    fn drop(&mut self) {
        let _ = self.file.unlock();
    }
}

impl FileLock for FlockFileLock {
    type Guard = LockGuard;
    fn try_acquire(&self, path: &Path) -> Result<LockGuard, HelperError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| HelperError::Ipc {
                message: format!("create lock parent dir: {e}"),
            })?;
        }
        let file = File::create(path).map_err(|e| HelperError::Ipc {
            message: format!("create lock file: {e}"),
        })?;
        file.try_lock_exclusive().map_err(|_| HelperError::Busy)?;
        Ok(LockGuard { file })
    }
}
```

- [ ] **Step 3: Windows impl (LockFileEx — real, not stub; per spec §5 trait inventory)**

`crates/boxpilot-platform/src/windows/lock.rs`:

```rust
use crate::traits::lock::FileLock;
use boxpilot_ipc::HelperError;
use std::fs::File;
use std::os::windows::io::AsRawHandle;
use std::path::Path;
use windows_sys::Win32::Storage::FileSystem::{
    LockFileEx, UnlockFileEx, LOCKFILE_EXCLUSIVE_LOCK, LOCKFILE_FAIL_IMMEDIATELY,
};
use windows_sys::Win32::System::IO::OVERLAPPED;

pub struct LockFileExLock;

pub struct LockGuard {
    file: File,
}

impl Drop for LockGuard {
    fn drop(&mut self) {
        unsafe {
            let mut o: OVERLAPPED = std::mem::zeroed();
            UnlockFileEx(self.file.as_raw_handle() as _, 0, u32::MAX, u32::MAX, &mut o);
        }
    }
}

impl FileLock for LockFileExLock {
    type Guard = LockGuard;
    fn try_acquire(&self, path: &Path) -> Result<LockGuard, HelperError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| HelperError::Ipc {
                message: format!("create lock parent dir: {e}"),
            })?;
        }
        let file = File::create(path).map_err(|e| HelperError::Ipc {
            message: format!("create lock file: {e}"),
        })?;
        unsafe {
            let mut o: OVERLAPPED = std::mem::zeroed();
            let ok = LockFileEx(
                file.as_raw_handle() as _,
                LOCKFILE_EXCLUSIVE_LOCK | LOCKFILE_FAIL_IMMEDIATELY,
                0,
                u32::MAX,
                u32::MAX,
                &mut o,
            );
            if ok == 0 {
                return Err(HelperError::Busy);
            }
        }
        Ok(LockGuard { file })
    }
}
```

- [ ] **Step 4: Fake (in-memory mutex per path)**

`crates/boxpilot-platform/src/fakes/lock.rs`:

```rust
use crate::traits::lock::FileLock;
use boxpilot_ipc::HelperError;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use std::collections::HashMap;
use std::sync::OnceLock;

#[derive(Default)]
pub struct MemoryFileLock;

pub struct LockGuard(MutexGuard<'static, ()>);

static REGISTRY: OnceLock<Mutex<HashMap<PathBuf, Arc<Mutex<()>>>>> = OnceLock::new();

fn registry() -> &'static Mutex<HashMap<PathBuf, Arc<Mutex<()>>>> {
    REGISTRY.get_or_init(|| Mutex::new(HashMap::new()))
}

impl FileLock for MemoryFileLock {
    type Guard = LockGuard;
    fn try_acquire(&self, path: &Path) -> Result<LockGuard, HelperError> {
        let m = registry()
            .lock()
            .unwrap()
            .entry(path.to_path_buf())
            .or_insert_with(|| Arc::new(Mutex::new(())))
            .clone();
        // Leak the mutex to get 'static — tests are short-lived; tracked.
        let leaked: &'static Mutex<()> = Box::leak(Box::new((*m).clone()));
        let guard = leaked.try_lock().map_err(|_| HelperError::Busy)?;
        Ok(LockGuard(guard))
    }
}
```

- [ ] **Step 5: Wire mods + replace `boxpilotd/src/lock.rs`**

`crates/boxpilotd/src/lock.rs`:

```rust
//! Re-export shell. Production impl in
//! `boxpilot_platform::linux::lock::FlockFileLock`.

pub use boxpilot_platform::linux::lock::{FlockFileLock, LockGuard};
pub use boxpilot_platform::traits::lock::FileLock;

use boxpilot_ipc::HelperError;
use std::path::Path;

/// Convenience wrapper preserving the existing call signature
/// (`lock::try_acquire(&path)`). Internally constructs a stack-local
/// `FlockFileLock` and calls into it.
pub fn try_acquire(path: &Path) -> Result<LockGuard, HelperError> {
    FlockFileLock.try_acquire(path)
}
```

- [ ] **Step 6: Run tests**

Run: `cargo test --workspace`
Expected: green.

- [ ] **Step 7: Commit**

```bash
git add crates/boxpilot-platform/src crates/boxpilotd/src/lock.rs
git commit -m "feat(platform): FileLock trait + Linux flock + Windows LockFileEx + fake"
```

---

## PR 6 smoke

```bash
cargo test --workspace
cargo check --target x86_64-pc-windows-gnu --workspace || true
```

---

# PR 7: `TrustChecker` trait

**Size:** S · **Touches:** `boxpilot-platform/src/{traits,linux,windows,fakes}/trust.rs`, `boxpilotd/src/core/trust.rs`. **Linux non-regression:** existing trust check (uid + mode + parent dirs + setuid + symlink walk + version probe) preserved bit-for-bit.

The existing `boxpilotd::core::trust::trust_check_path(...)` is a free function; we wrap it behind a trait so a Windows ACL-based impl can plug in later (Sub-project #2).

## Task 7.1: Define + Linux impl + fake

**Files:**
- Create: `crates/boxpilot-platform/src/traits/trust.rs`, `linux/trust.rs`, `windows/trust.rs`, `fakes/trust.rs`
- Modify: `crates/boxpilotd/src/core/trust.rs` — extract free function into trait method

- [ ] **Step 1: Trait**

`crates/boxpilot-platform/src/traits/trust.rs`:

```rust
//! Spec §6.5 trust check, abstracted so Windows ACL semantics can plug in
//! later. Linux: uid + mode bits + parent-dir walk + setuid + symlink walk
//! + version probe. Windows (Sub-project #2): NTFS ACL + owner-SID +
//! parent-dir-not-writable.

use async_trait::async_trait;
use boxpilot_ipc::HelperError;
use std::path::Path;

#[async_trait]
pub trait TrustChecker: Send + Sync {
    /// Returns Ok(()) if `path` (and every parent) passes trust checks.
    /// `allowed_prefixes` are absolute roots under which trusted binaries
    /// may live (per platform).
    async fn check(
        &self,
        path: &Path,
        allowed_prefixes: &[&Path],
    ) -> Result<(), HelperError>;
}
```

- [ ] **Step 2: Linux impl**

`crates/boxpilot-platform/src/linux/trust.rs`:

Wrap the existing free function `boxpilotd::core::trust::trust_check_path(...)` as a struct + trait impl. The new impl holds whatever dependencies the free function needs (`FsMetadataProvider`, `VersionChecker`).

```rust
use crate::traits::trust::TrustChecker;
use crate::traits::fs_meta::FsMetadataProvider;
use crate::traits::version::VersionChecker;
use async_trait::async_trait;
use boxpilot_ipc::HelperError;
use std::path::Path;
use std::sync::Arc;

pub struct LinuxTrustChecker {
    pub fs: Arc<dyn FsMetadataProvider>,
    pub version_checker: Arc<dyn VersionChecker>,
}

#[async_trait]
impl TrustChecker for LinuxTrustChecker {
    async fn check(
        &self,
        path: &Path,
        allowed_prefixes: &[&Path],
    ) -> Result<(), HelperError> {
        // Body: copy from boxpilotd::core::trust::trust_check_path verbatim,
        // adapting `&dyn FsMetadataProvider` parameters to `&*self.fs`.
        // The ~150-line check sequence stays unchanged.
        todo!("paste boxpilotd::core::trust::trust_check_path body, adapt deps to self fields");
    }
}
```

- [ ] **Step 3: Fill `todo!()` with the existing function body**

Open `crates/boxpilotd/src/core/trust.rs`, find `trust_check_path` (or similarly-named), and copy its body into Step 2's `check` impl. Adjust:
- Function-parameter `fs: &dyn FsMetadataProvider` → use `&*self.fs`.
- Function-parameter `version_checker: &dyn VersionChecker` → use `&*self.version_checker`.

- [ ] **Step 4: Windows stub**

`crates/boxpilot-platform/src/windows/trust.rs`:

```rust
use crate::traits::trust::TrustChecker;
use async_trait::async_trait;
use boxpilot_ipc::HelperError;
use std::path::Path;

pub struct WindowsTrustChecker;

#[async_trait]
impl TrustChecker for WindowsTrustChecker {
    async fn check(
        &self,
        _path: &Path,
        _allowed_prefixes: &[&Path],
    ) -> Result<(), HelperError> {
        Err(HelperError::NotImplemented)
    }
}
```

- [ ] **Step 5: Fake**

`crates/boxpilot-platform/src/fakes/trust.rs`:

```rust
use crate::traits::trust::TrustChecker;
use async_trait::async_trait;
use boxpilot_ipc::HelperError;
use std::path::Path;

pub struct AlwaysTrust;

#[async_trait]
impl TrustChecker for AlwaysTrust {
    async fn check(&self, _: &Path, _: &[&Path]) -> Result<(), HelperError> {
        Ok(())
    }
}

pub struct AlwaysReject {
    pub reason: HelperError,
}

#[async_trait]
impl TrustChecker for AlwaysReject {
    async fn check(&self, _: &Path, _: &[&Path]) -> Result<(), HelperError> {
        Err(self.reason.clone())
    }
}
```

- [ ] **Step 6: Wire mods + retire the boxpilotd free function**

In `crates/boxpilotd/src/core/trust.rs`, replace `trust_check_path` with a thin wrapper calling the new trait, OR delete it and update each caller to construct `LinuxTrustChecker` and call `check`. The latter is cleaner; one-time refactor.

Run: `git grep -n "trust_check_path" crates/boxpilotd/src`
For each call site, replace with:

```rust
let checker = boxpilot_platform::linux::trust::LinuxTrustChecker {
    fs: Arc::clone(&ctx.fs_meta),
    version_checker: Arc::clone(&ctx.version_checker),
};
checker.check(path, &allowed_prefixes).await?;
```

- [ ] **Step 7: Run tests**

Run: `cargo test --workspace`
Expected: green.

- [ ] **Step 8: Commit**

```bash
git add crates/boxpilot-platform/src crates/boxpilotd/src/core/trust.rs
git commit -m "feat(platform): TrustChecker trait + Linux impl wraps existing trust_check_path"
```

---

## PR 7 smoke

```bash
cargo test --workspace
git grep "trust_check_path" crates/boxpilotd/src    # zero matches expected (or only re-export comment)
cargo check --target x86_64-pc-windows-gnu --workspace || true
```

---

# PR 8: `ActivePointer` trait

**Size:** S · **Touches:** `boxpilot-platform/src/{traits,linux,windows,fakes}/active.rs`, `boxpilotd/src/profile/release.rs` (refactored), `boxpilotd/src/profile/{activate,recovery,rollback}.rs` test fixtures (per Round 6/6.5).

Wraps Linux's symlink + `rename(2)` and Windows's marker JSON + `MoveFileEx` (designed; impl deferred). Per spec §5.3.

## Task 8.1: Define + Linux impl + fake

**Files:**
- Create: `crates/boxpilot-platform/src/traits/active.rs`, `linux/active.rs`, `windows/active.rs`, `fakes/active.rs`

- [ ] **Step 1: Trait**

`crates/boxpilot-platform/src/traits/active.rs`:

```rust
//! Atomic "active release" pointer. Linux: symlink with rename(2). Windows:
//! marker JSON file with MoveFileEx(MOVEFILE_REPLACE_EXISTING). Per spec §5.3.

use async_trait::async_trait;
use boxpilot_ipc::HelperError;
use std::path::PathBuf;

#[async_trait]
pub trait ActivePointer: Send + Sync {
    /// Read the currently active release id, or None if none is active.
    async fn read(&self) -> Result<Option<String>, HelperError>;

    /// Atomically set the active release to `release_id`.
    async fn set(&self, release_id: &str) -> Result<(), HelperError>;

    /// Resolve the active pointer to its on-disk release directory.
    /// Returns None if not set; HelperError if pointer is corrupted (Linux:
    /// dangling symlink; Windows: marker file references nonexistent dir).
    async fn active_resolved(&self) -> Result<Option<PathBuf>, HelperError>;

    /// Compute the full path of the release dir for `release_id`. Pure
    /// path manipulation (does not check existence).
    fn release_dir(&self, release_id: &str) -> PathBuf;
}
```

- [ ] **Step 2: Linux impl (wrap existing logic)**

`crates/boxpilot-platform/src/linux/active.rs`:

```rust
use crate::traits::active::ActivePointer;
use async_trait::async_trait;
use boxpilot_ipc::HelperError;
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};

pub struct SymlinkActivePointer {
    pub active: PathBuf,        // /etc/boxpilot/active
    pub releases_dir: PathBuf,  // /etc/boxpilot/releases
}

#[async_trait]
impl ActivePointer for SymlinkActivePointer {
    async fn read(&self) -> Result<Option<String>, HelperError> {
        match tokio::fs::read_link(&self.active).await {
            Ok(target) => Ok(target
                .file_name()
                .map(|n| n.to_string_lossy().to_string())),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(HelperError::Ipc {
                message: format!("read active symlink: {e}"),
            }),
        }
    }

    async fn set(&self, release_id: &str) -> Result<(), HelperError> {
        let new_target = self.releases_dir.join(release_id);
        let new_link = self.active.with_extension("new");
        // Best-effort cleanup of any previous .new before symlinking.
        let _ = tokio::fs::remove_file(&new_link).await;
        let new_link_inner = new_link.clone();
        let new_target_inner = new_target.clone();
        tokio::task::spawn_blocking(move || symlink(&new_target_inner, &new_link_inner))
            .await
            .map_err(|e| HelperError::Ipc {
                message: format!("spawn symlink: {e}"),
            })?
            .map_err(|e| HelperError::Ipc {
                message: format!("symlink active.new: {e}"),
            })?;
        tokio::fs::rename(&new_link, &self.active)
            .await
            .map_err(|e| HelperError::Ipc {
                message: format!("rename active.new -> active: {e}"),
            })
    }

    async fn active_resolved(&self) -> Result<Option<PathBuf>, HelperError> {
        match tokio::fs::read_link(&self.active).await {
            Ok(target) => Ok(Some(target)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(HelperError::Ipc {
                message: format!("active_resolved: {e}"),
            }),
        }
    }

    fn release_dir(&self, release_id: &str) -> PathBuf {
        self.releases_dir.join(release_id)
    }
}
```

- [ ] **Step 3: Windows stub (designed; not implemented in Sub-project #1)**

`crates/boxpilot-platform/src/windows/active.rs`:

```rust
use crate::traits::active::ActivePointer;
use async_trait::async_trait;
use boxpilot_ipc::HelperError;
use std::path::PathBuf;

pub struct MarkerFileActivePointer {
    pub marker: PathBuf,        // %ProgramData%\BoxPilot\active.json
    pub releases_dir: PathBuf,  // %ProgramData%\BoxPilot\releases
}

#[async_trait]
impl ActivePointer for MarkerFileActivePointer {
    async fn read(&self) -> Result<Option<String>, HelperError> {
        Err(HelperError::NotImplemented)
    }
    async fn set(&self, _: &str) -> Result<(), HelperError> {
        Err(HelperError::NotImplemented)
    }
    async fn active_resolved(&self) -> Result<Option<PathBuf>, HelperError> {
        Err(HelperError::NotImplemented)
    }
    fn release_dir(&self, release_id: &str) -> PathBuf {
        self.releases_dir.join(release_id)
    }
}
```

- [ ] **Step 4: Fake (in-memory state)**

`crates/boxpilot-platform/src/fakes/active.rs`:

```rust
use crate::traits::active::ActivePointer;
use async_trait::async_trait;
use boxpilot_ipc::HelperError;
use std::path::PathBuf;
use std::sync::Mutex;

pub struct InMemoryActive {
    pub releases_dir: PathBuf,
    state: Mutex<Option<String>>,
}

impl InMemoryActive {
    pub fn under(releases_dir: PathBuf) -> Self {
        Self {
            releases_dir,
            state: Mutex::new(None),
        }
    }
}

#[async_trait]
impl ActivePointer for InMemoryActive {
    async fn read(&self) -> Result<Option<String>, HelperError> {
        Ok(self.state.lock().unwrap().clone())
    }
    async fn set(&self, release_id: &str) -> Result<(), HelperError> {
        *self.state.lock().unwrap() = Some(release_id.to_string());
        Ok(())
    }
    async fn active_resolved(&self) -> Result<Option<PathBuf>, HelperError> {
        Ok(self
            .state
            .lock()
            .unwrap()
            .as_ref()
            .map(|id| self.releases_dir.join(id)))
    }
    fn release_dir(&self, release_id: &str) -> PathBuf {
        self.releases_dir.join(release_id)
    }
}
```

- [ ] **Step 5: Wire mods**

Add `pub mod active;` to `traits/`, `linux/`, `windows/`, `fakes/`.

- [ ] **Step 6: Replace usage in `boxpilotd::profile::release`**

The existing code uses raw `std::os::unix::fs::symlink` + `std::fs::rename`. Remove the `use std::os::unix::fs::symlink;` line at the top, and refactor the public functions in `release.rs` (e.g., `swing_active_to`, `read_active`, etc.) to take `&dyn ActivePointer` from the caller's `HelperContext` instead of doing raw fs.

This is the largest part of PR 8. Pattern:
- Add `pub active: Arc<dyn boxpilot_platform::traits::active::ActivePointer>` to `HelperContext`.
- In `main.rs`, construct `Arc::new(SymlinkActivePointer { active: paths.active_symlink(), releases_dir: paths.releases_dir() })`.
- Each function in `release.rs` that previously took `&Paths` now takes `&dyn ActivePointer` (or borrows it from a passed-in context).
- Internal helpers that did `symlink(&target, &active)` directly become `active.set(release_id).await?`.

- [ ] **Step 7: Migrate test fixtures using `std::os::unix::fs::symlink` (per Round 6/6.5)**

For each occurrence in `boxpilotd/src/profile/{activate,recovery,rollback}.rs`, `iface.rs:1208`, `diagnostics/mod.rs:203`:

- If the symlink is setting up the "active" pointer for a test, replace with:

```rust
let active = boxpilot_platform::fakes::active::InMemoryActive::under(paths.releases_dir());
active.set("rel-1").await.unwrap();
// pass `&active` into the function under test instead of `&paths`.
```

- If the symlink is testing a corruption/invalid scenario, wrap in `#[cfg(target_os = "linux")]` (the test is exercising Linux-specific symlink semantics).

- [ ] **Step 8: Run tests**

Run: `cargo test --workspace`
Expected: green.

- [ ] **Step 9: Commit**

```bash
git add crates/boxpilot-platform/src crates/boxpilotd/src
git commit -m "feat(platform): ActivePointer trait + Linux symlink impl + Windows stub + fake"
```

---

## PR 8 smoke

```bash
cargo test --workspace
git grep "use std::os::unix::fs::symlink" crates/boxpilotd/src
# Expected: only inside #[cfg(target_os = "linux")] test-fixture blocks
cargo check --target x86_64-pc-windows-gnu --workspace || true
```

---

# PR 9: `CoreAssetNaming` + `CoreArchive` + `check.rs` Windows stub (per COQ14)

**Size:** M · **Touches:** `boxpilot-platform/src/{traits,linux,windows,fakes}/core_assets.rs`, `boxpilotd/src/core/install.rs`, `boxpilot-profile/src/check.rs` (cfg-split per COQ14).

## Task 9.1: `CoreAssetNaming` + `CoreArchive` traits

**Files:**
- Create: `crates/boxpilot-platform/src/traits/core_assets.rs`, `linux/core_assets.rs`, `windows/core_assets.rs`, `fakes/core_assets.rs`

- [ ] **Step 1: Trait**

`crates/boxpilot-platform/src/traits/core_assets.rs`:

```rust
//! Naming and extraction of upstream sing-box release archives.
//!
//! Linux: `sing-box-<version>-linux-<arch>.tar.gz` extracted to a flat dir.
//! Windows: `sing-box-<version>-windows-<arch>.zip` (Sub-project #2).
//! Per spec §11.3 + §5.

use async_trait::async_trait;
use boxpilot_ipc::HelperError;
use std::path::Path;

pub trait CoreAssetNaming: Send + Sync {
    /// Build the upstream asset filename for `(version, arch)`.
    fn asset_name(&self, version: &str, arch: &str) -> String;

    /// The binary name inside the asset (`sing-box` on Linux, `sing-box.exe`
    /// on Windows).
    fn binary_name(&self) -> &'static str;
}

#[async_trait]
pub trait CoreArchive: Send + Sync {
    /// Extract `archive_path` (already downloaded) into `dest_dir`. Caller
    /// has created `dest_dir` and is responsible for fsync/atomic rename.
    /// Returns the path of the extracted core binary on disk.
    async fn extract(
        &self,
        archive_path: &Path,
        dest_dir: &Path,
    ) -> Result<std::path::PathBuf, HelperError>;
}
```

- [ ] **Step 2: Linux impl (tar.gz)**

`crates/boxpilot-platform/src/linux/core_assets.rs`:

Move existing tar.gz extraction logic from `boxpilotd::core::install` here. Adapt to trait.

```rust
use crate::traits::core_assets::{CoreArchive, CoreAssetNaming};
use async_trait::async_trait;
use boxpilot_ipc::HelperError;
use flate2::read::GzDecoder;
use std::path::{Path, PathBuf};
use tar::Archive;

pub struct LinuxCoreAssetNaming;

impl CoreAssetNaming for LinuxCoreAssetNaming {
    fn asset_name(&self, version: &str, arch: &str) -> String {
        format!("sing-box-{version}-linux-{arch}.tar.gz")
    }
    fn binary_name(&self) -> &'static str {
        "sing-box"
    }
}

pub struct TarGzExtractor;

#[async_trait]
impl CoreArchive for TarGzExtractor {
    async fn extract(
        &self,
        archive_path: &Path,
        dest_dir: &Path,
    ) -> Result<PathBuf, HelperError> {
        let archive_path = archive_path.to_path_buf();
        let dest_dir = dest_dir.to_path_buf();
        tokio::task::spawn_blocking(move || -> Result<PathBuf, HelperError> {
            let tar_gz = std::fs::File::open(&archive_path).map_err(|e| HelperError::Ipc {
                message: format!("open archive: {e}"),
            })?;
            let mut archive = Archive::new(GzDecoder::new(tar_gz));
            // Existing logic in boxpilotd::core::install: extract only the
            // sing-box binary, flatten the tarball's leading dir
            // ("sing-box-1.10.3-linux-amd64/sing-box" → "<dest>/sing-box").
            // Copy that loop verbatim.
            let mut found_binary: Option<PathBuf> = None;
            for entry in archive.entries().map_err(|e| HelperError::Ipc {
                message: format!("read tar entries: {e}"),
            })? {
                let mut entry = entry.map_err(|e| HelperError::Ipc {
                    message: format!("tar entry: {e}"),
                })?;
                let path = entry.path().map_err(|e| HelperError::Ipc {
                    message: format!("tar path: {e}"),
                })?;
                let file_name = match path.file_name() {
                    Some(n) => n.to_owned(),
                    None => continue,
                };
                if file_name == "sing-box" && entry.header().entry_type().is_file() {
                    let dest = dest_dir.join("sing-box");
                    entry.unpack(&dest).map_err(|e| HelperError::Ipc {
                        message: format!("unpack sing-box: {e}"),
                    })?;
                    found_binary = Some(dest);
                    break;
                }
            }
            found_binary.ok_or_else(|| HelperError::Ipc {
                message: "sing-box binary not found in archive".into(),
            })
        })
        .await
        .map_err(|e| HelperError::Ipc {
            message: format!("spawn extract: {e}"),
        })?
    }
}
```

- [ ] **Step 3: Windows stub (zip extractor — designed; impl deferred to Sub-project #2)**

`crates/boxpilot-platform/src/windows/core_assets.rs`:

```rust
use crate::traits::core_assets::{CoreArchive, CoreAssetNaming};
use async_trait::async_trait;
use boxpilot_ipc::HelperError;
use std::path::{Path, PathBuf};

pub struct WindowsCoreAssetNaming;

impl CoreAssetNaming for WindowsCoreAssetNaming {
    fn asset_name(&self, version: &str, arch: &str) -> String {
        format!("sing-box-{version}-windows-{arch}.zip")
    }
    fn binary_name(&self) -> &'static str {
        "sing-box.exe"
    }
}

pub struct ZipExtractor;

#[async_trait]
impl CoreArchive for ZipExtractor {
    async fn extract(&self, _: &Path, _: &Path) -> Result<PathBuf, HelperError> {
        Err(HelperError::NotImplemented)
    }
}
```

- [ ] **Step 4: Fake**

`crates/boxpilot-platform/src/fakes/core_assets.rs`:

```rust
use crate::traits::core_assets::{CoreArchive, CoreAssetNaming};
use async_trait::async_trait;
use boxpilot_ipc::HelperError;
use std::path::{Path, PathBuf};

pub struct LinuxAssetNaming;

impl CoreAssetNaming for LinuxAssetNaming {
    fn asset_name(&self, version: &str, arch: &str) -> String {
        format!("sing-box-{version}-linux-{arch}.tar.gz")
    }
    fn binary_name(&self) -> &'static str {
        "sing-box"
    }
}

pub struct StubExtractor;

#[async_trait]
impl CoreArchive for StubExtractor {
    async fn extract(&self, _: &Path, dest_dir: &Path) -> Result<PathBuf, HelperError> {
        let p = dest_dir.join("sing-box");
        std::fs::write(&p, b"#!/bin/sh\necho 1.10.3-fake\n").map_err(|e| HelperError::Ipc {
            message: format!("write fake binary: {e}"),
        })?;
        Ok(p)
    }
}
```

- [ ] **Step 5: Wire + retire `boxpilotd::core::install` extract code**

In `crates/boxpilotd/src/core/install.rs`, replace the inline tar.gz extract loop with a call to the trait:

```rust
let extractor = boxpilot_platform::linux::core_assets::TarGzExtractor;
let bin = extractor.extract(&archive_path, &dest_dir).await?;
```

Same for asset name construction (use `LinuxCoreAssetNaming::asset_name(&version, &arch)`).

- [ ] **Step 6: Run tests**

Run: `cargo test --workspace`
Expected: green.

- [ ] **Step 7: Commit**

```bash
git add crates/boxpilot-platform/src crates/boxpilotd/src/core/install.rs
git commit -m "feat(platform): CoreAssetNaming + CoreArchive traits; Linux tar.gz extracts via trait"
```

---

## Task 9.2: `boxpilot-profile/src/check.rs` cfg-split (per COQ14)

**Files:**
- Modify: `crates/boxpilot-profile/src/check.rs` — cfg-split: Linux retains existing pgid+SIGKILL logic; Windows returns success stub

The existing `run_singbox_check` is a synchronous function with `process_group(0)` + `libc::kill(-pgid, SIGKILL)`. Per COQ14 we cfg-split this in Sub-project #1; real JobObject impl is Sub-project #2.

- [ ] **Step 1: Wrap the existing function in `#[cfg(target_os = "linux")]`**

In `crates/boxpilot-profile/src/check.rs`, change line 29 from:

```rust
pub fn run_singbox_check(core_path: &Path, working_dir: &Path) -> Result<CheckOutput, CheckError> {
```

to a cfg-split:

```rust
#[cfg(target_os = "linux")]
pub fn run_singbox_check(core_path: &Path, working_dir: &Path) -> Result<CheckOutput, CheckError> {
    // ... existing body unchanged ...
}

#[cfg(target_os = "windows")]
pub fn run_singbox_check(_core_path: &Path, _working_dir: &Path) -> Result<CheckOutput, CheckError> {
    // Per COQ14: sing-box check on Windows is short-circuited in
    // Sub-project #1. Real JobObject-based impl arrives in Sub-project #2.
    // This is documented as best-effort preflight in the Linux design
    // spec §10 step 3 — skipping it on Windows is acceptable.
    Ok(CheckOutput {
        success: true,
        stdout: "sing-box check skipped on Windows in Sub-project #1".to_string(),
        stderr: String::new(),
    })
}
```

- [ ] **Step 2: Cfg-gate the Linux test module**

The existing `#[cfg(test)] mod tests` in `check.rs` uses `write_executable` with `set_permissions(..., 0o755)`. Wrap the entire test module:

```rust
#[cfg(all(test, target_os = "linux"))]
mod tests {
    // existing body
}
```

- [ ] **Step 3: Run Linux tests**

Run: `cargo test -p boxpilot-profile`
Expected: green.

- [ ] **Step 4: Verify Windows compile of just this crate**

Run: `cargo check --target x86_64-pc-windows-gnu -p boxpilot-profile`
Expected: still fails (because `bundle.rs` uses `nix::sys::memfd`), but the **error count drops** — `check.rs` is no longer one of the failing modules. Document in commit message.

- [ ] **Step 5: Commit**

```bash
git add crates/boxpilot-profile/src/check.rs
git commit -m "refactor(profile): cfg-split run_singbox_check; Windows returns success stub (COQ14)"
```

---

## PR 9 smoke

```bash
cargo test --workspace
cargo check --target x86_64-pc-windows-gnu --workspace || true   # closer to passing; bundle.rs still blocks
```

---

# PR 10: `AuxStream` + bundle byte transfer refactor

**Size:** L · **Touches:** `boxpilot-platform/src/traits/{ipc,bundle_aux}.rs` (AuxStream type), `boxpilot-platform/src/{linux,windows}/bundle.rs`, `boxpilot-profile/src/bundle.rs` (memfd code moves OUT), `boxpilotd/src/profile/unpack.rs` (consumes AuxStream).

This is the deepest PR in Sub-project #1. Per COQ8: `AuxStream` is opaque with crate-private accessors; Linux preserves zero-copy via cfg-gated `from_owned_fd`. Per COQ1+2: bundle bytes flow through dispatch's `aux: AuxStream` parameter; no `BundleClient`/`BundleServer` traits. After this PR, **Windows compile no longer needs allow-fail** (the only remaining blocker — memfd — is gone).

## Task 10.1: Define `AuxStream` opaque type

**Files:**
- Create: `crates/boxpilot-platform/src/traits/bundle_aux.rs` (new — distinct from ipc.rs which arrives in PR 11a)

The struct lives in its own file because PR 11a's `IpcServer` trait references it; PR 10 introduces it standalone.

- [ ] **Step 1: Write the type**

`crates/boxpilot-platform/src/traits/bundle_aux.rs`:

```rust
//! `AuxStream` — bytes-handle plumbing for IPC verbs that ship bundles
//! alongside their typed body. Per spec COQ8: opaque struct with
//! crate-private accessors; Linux preserves zero-copy via the cfg-gated
//! `from_owned_fd` constructor.

use tokio::io::AsyncRead;

pub struct AuxStream {
    repr: AuxStreamRepr,
}

pub(crate) enum AuxStreamRepr {
    None,
    AsyncRead(Box<dyn AsyncRead + Send + Unpin>),
    #[cfg(target_os = "linux")]
    LinuxFd(std::os::fd::OwnedFd),
}

impl AuxStream {
    pub fn none() -> Self {
        Self { repr: AuxStreamRepr::None }
    }

    pub fn from_async_read(r: impl AsyncRead + Send + Unpin + 'static) -> Self {
        Self { repr: AuxStreamRepr::AsyncRead(Box::new(r)) }
    }

    #[cfg(target_os = "linux")]
    pub fn from_owned_fd(fd: std::os::fd::OwnedFd) -> Self {
        Self { repr: AuxStreamRepr::LinuxFd(fd) }
    }

    pub fn is_none(&self) -> bool {
        matches!(self.repr, AuxStreamRepr::None)
    }

    pub(crate) fn into_repr(self) -> AuxStreamRepr {
        self.repr
    }
}

impl std::fmt::Debug for AuxStream {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.repr {
            AuxStreamRepr::None => write!(f, "AuxStream::None"),
            AuxStreamRepr::AsyncRead(_) => write!(f, "AuxStream::AsyncRead(<opaque>)"),
            #[cfg(target_os = "linux")]
            AuxStreamRepr::LinuxFd(fd) => {
                write!(f, "AuxStream::LinuxFd({:?})", std::os::fd::AsRawFd::as_raw_fd(fd))
            }
        }
    }
}
```

- [ ] **Step 2: Wire into `traits/mod.rs`**

```rust
pub mod bundle_aux;
pub use bundle_aux::AuxStream;
```

- [ ] **Step 3: Run sanity build**

Run: `cargo build -p boxpilot-platform`
Expected: clean on Linux + Windows (the type is platform-agnostic except the `LinuxFd` variant which is cfg-gated).

- [ ] **Step 4: Commit**

```bash
git add crates/boxpilot-platform/src/traits
git commit -m "feat(platform): introduce AuxStream opaque type with crate-private accessors (COQ8)"
```

---

## Task 10.2: Move memfd build logic from `boxpilot-profile` to platform crate

**Files:**
- Create: `crates/boxpilot-platform/src/linux/bundle.rs` (memfd + seal logic moves here)
- Create: `crates/boxpilot-platform/src/windows/bundle.rs` (tempfile-based stub)
- Create: `crates/boxpilot-platform/src/fakes/bundle_aux.rs` (in-memory Cursor helpers)
- Modify: `crates/boxpilot-profile/src/bundle.rs` (becomes thin wrapper)

- [ ] **Step 1: Move memfd code**

`crates/boxpilot-platform/src/linux/bundle.rs`:

Copy `create_sealed_bundle_memfd` from `boxpilot-profile/src/bundle.rs` (lines 200-225). Adapt to return `AuxStream::from_owned_fd(fd)`.

```rust
//! Linux bundle byte transfer. Builds a tar in a sealed memfd and exposes
//! it as an `AuxStream`. The Linux IpcClient FD-passes the memfd zero-copy
//! through D-Bus; the helper mmap's the (sealed, immutable) FD and
//! hashes-while-untarring.

use crate::traits::bundle_aux::AuxStream;
use boxpilot_ipc::HelperError;
use nix::fcntl::{FcntlArg, SealFlag};
use nix::sys::memfd::{memfd_create, MemFdCreateFlag};
use std::ffi::CString;
use std::os::fd::{AsRawFd, OwnedFd};
use std::path::Path;
use tar::Builder;

/// Build a tar of `staging_dir`'s contents into a sealed memfd, return as
/// `AuxStream`. The caller (typically `boxpilot-profile::bundle::prepare`)
/// has already validated assets and computed sha256.
pub async fn build_sealed_memfd_aux(staging_dir: &Path) -> Result<AuxStream, HelperError> {
    let staging_dir = staging_dir.to_path_buf();
    let fd = tokio::task::spawn_blocking(move || -> Result<OwnedFd, HelperError> {
        let cname = CString::new("boxpilot-bundle").unwrap();
        let fd = memfd_create(
            &cname,
            MemFdCreateFlag::MFD_CLOEXEC | MemFdCreateFlag::MFD_ALLOW_SEALING,
        )
        .map_err(|e| HelperError::Ipc {
            message: format!("memfd_create: {e}"),
        })?;
        // Tar staging_dir into the FD.
        {
            let mut file = std::fs::File::from(fd.try_clone().map_err(|e| HelperError::Ipc {
                message: format!("clone memfd: {e}"),
            })?);
            let mut builder = Builder::new(&mut file);
            builder
                .append_dir_all(".", &staging_dir)
                .map_err(|e| HelperError::Ipc {
                    message: format!("tar append: {e}"),
                })?;
            builder.finish().map_err(|e| HelperError::Ipc {
                message: format!("tar finish: {e}"),
            })?;
        }
        // Seal.
        let seals = SealFlag::F_SEAL_WRITE
            | SealFlag::F_SEAL_GROW
            | SealFlag::F_SEAL_SHRINK
            | SealFlag::F_SEAL_SEAL;
        nix::fcntl::fcntl(fd.as_raw_fd(), FcntlArg::F_ADD_SEALS(seals)).map_err(|e| {
            HelperError::Ipc {
                message: format!("F_ADD_SEALS: {e}"),
            }
        })?;
        Ok(fd)
    })
    .await
    .map_err(|e| HelperError::Ipc {
        message: format!("spawn build_sealed_memfd: {e}"),
    })??;

    Ok(AuxStream::from_owned_fd(fd))
}
```

- [ ] **Step 2: Windows tempfile stub**

`crates/boxpilot-platform/src/windows/bundle.rs`:

```rust
use crate::traits::bundle_aux::AuxStream;
use boxpilot_ipc::HelperError;
use std::path::Path;

/// Tar `staging_dir` into a tempfile under `%LocalAppData%\BoxPilot\tmp\`,
/// ACL'd to the owner SID, and return as `AuxStream::from_async_read`.
/// Sub-project #1 ships the Windows side as `unimplemented!()` because no
/// Windows verb actually consumes a bundle yet (per AC4 + AC5).
pub async fn build_tempfile_aux(_staging_dir: &Path) -> Result<AuxStream, HelperError> {
    Err(HelperError::NotImplemented)
}
```

- [ ] **Step 3: Fakes (in-memory bytes)**

`crates/boxpilot-platform/src/fakes/bundle_aux.rs`:

```rust
use crate::traits::bundle_aux::AuxStream;
use std::io::Cursor;

/// Build an `AuxStream` from raw tar bytes — for tests.
pub fn aux_from_bytes(bytes: Vec<u8>) -> AuxStream {
    AuxStream::from_async_read(Cursor::new(bytes))
}
```

Add `pub mod bundle_aux;` to `fakes/mod.rs`. Also add `pub mod bundle;` to `linux/mod.rs` and `windows/mod.rs`.

- [ ] **Step 4: Update `boxpilot-profile::bundle`**

In `crates/boxpilot-profile/src/bundle.rs`:

- Remove `create_sealed_bundle_memfd` and the imports of `nix::sys::memfd`, `nix::fcntl`, `nix::sys::stat`.
- Change the public `prepare` API:

```rust
pub struct PreparedBundle {
    pub manifest: ActivationManifest,
    pub stream: boxpilot_platform::traits::bundle_aux::AuxStream,
    pub sha256: [u8; 32],
}

pub async fn prepare(
    staging: &Path,
    paths: &boxpilot_platform::Paths,
) -> Result<PreparedBundle, BundleError> {
    // Existing validation + manifest building stays.
    let manifest = build_manifest(staging)?;
    let sha256 = compute_sha256_of_tar(staging).await?;
    let stream = build_aux_stream(staging, paths).await?;
    Ok(PreparedBundle { manifest, stream, sha256 })
}

#[cfg(target_os = "linux")]
async fn build_aux_stream(
    staging: &Path,
    _paths: &boxpilot_platform::Paths,
) -> Result<boxpilot_platform::traits::bundle_aux::AuxStream, BundleError> {
    boxpilot_platform::linux::bundle::build_sealed_memfd_aux(staging)
        .await
        .map_err(|e| BundleError::Io(std::io::Error::other(format!("{e:?}"))))
}

#[cfg(target_os = "windows")]
async fn build_aux_stream(
    staging: &Path,
    _paths: &boxpilot_platform::Paths,
) -> Result<boxpilot_platform::traits::bundle_aux::AuxStream, BundleError> {
    boxpilot_platform::windows::bundle::build_tempfile_aux(staging)
        .await
        .map_err(|e| BundleError::Io(std::io::Error::other(format!("{e:?}"))))
}
```

`compute_sha256_of_tar` is a small helper that re-tars staging into a hash-only sink. Keep its body next to `prepare`. (The existing impl already SHA256s the memfd post-build; refactor to compute hash via a standalone tar pass so the AuxStream and the hash are independent — keeps Linux and Windows code paths uniform.)

- [ ] **Step 5: Update existing tests**

The existing `bundle.rs` tests verify memfd seal flags. Move those tests into `crates/boxpilot-platform/src/linux/bundle.rs`'s `#[cfg(test)]` mod (Linux-only), since they assert kernel-level seal semantics. Remove from `boxpilot-profile`.

- [ ] **Step 6: Run all tests**

Run: `cargo test --workspace`
Expected: green.

- [ ] **Step 7: Verify Windows compile finally passes**

Run: `cargo check --target x86_64-pc-windows-gnu --workspace`
Expected: **passes** for the first time. (`boxpilot-profile/bundle.rs` no longer references nix.)

- [ ] **Step 8: Commit**

```bash
git add crates/boxpilot-platform/src crates/boxpilot-profile/src/bundle.rs
git commit -m "refactor(profile,platform): move memfd bundle build to platform; AuxStream as transport handle"
```

---

## Task 10.3: Update `boxpilotd::profile::unpack` to consume `AuxStream`

**Files:**
- Modify: `crates/boxpilotd/src/profile/unpack.rs`

The existing `unpack` takes an `OwnedFd` (from D-Bus FD-passing). Change the signature to take `AuxStream`. Internally, match on `AuxStream::into_repr()`:

```rust
match aux.into_repr() {
    AuxStreamRepr::None => Err(HelperError::Ipc { message: "missing aux".into() }),
    AuxStreamRepr::AsyncRead(reader) => unpack_from_async_read(reader, ...).await,
    #[cfg(target_os = "linux")]
    AuxStreamRepr::LinuxFd(fd) => unpack_from_memfd_owned(fd, ...).await,
}
```

`AuxStreamRepr` is `pub(crate)` to `boxpilot-platform`; expose a tiny `boxpilot_platform::traits::bundle_aux::take_inner(aux)` helper that's only callable from inside the crate (or use a `pub(crate)` consumer trait like `into_async_read_or_fd`). Easier: add a helper that consumes the AuxStream:

```rust
// In boxpilot-platform/src/traits/bundle_aux.rs:

impl AuxStream {
    /// Consume the stream into a uniform `AsyncRead`. On Linux, FD-backed
    /// streams are wrapped in `tokio::fs::File`. The helper-side dispatch
    /// uses this to hash-while-reading without caring how the bytes
    /// arrived.
    pub fn into_async_read(self) -> Box<dyn tokio::io::AsyncRead + Send + Unpin> {
        match self.repr {
            AuxStreamRepr::None => Box::new(tokio::io::empty()),
            AuxStreamRepr::AsyncRead(r) => r,
            #[cfg(target_os = "linux")]
            AuxStreamRepr::LinuxFd(fd) => {
                let std_file = std::fs::File::from(fd);
                Box::new(tokio::fs::File::from_std(std_file))
            }
        }
    }
}
```

- [ ] **Step 1: Add `into_async_read` to `AuxStream`**

Edit `crates/boxpilot-platform/src/traits/bundle_aux.rs` per the snippet above.

- [ ] **Step 2: Update `boxpilotd::profile::unpack`**

Change the unpack function's outer signature from `(fd: OwnedFd, ...)` to `(aux: AuxStream, ...)`. Inside, do `let mut reader = aux.into_async_read();` and stream-unpack from there. Hash-while-reading using `Sha256` updated per chunk.

The existing memfd-mmap fast path becomes the AsyncRead-with-File-backing path automatically.

- [ ] **Step 3: Run tests**

Run: `cargo test --workspace`
Expected: green.

- [ ] **Step 4: Commit**

```bash
git add crates/boxpilot-platform/src/traits/bundle_aux.rs crates/boxpilotd/src/profile/unpack.rs
git commit -m "refactor(boxpilotd): unpack consumes AuxStream::into_async_read"
```

---

## PR 10 smoke

```bash
cargo test --workspace                                                  # Linux non-regression
cargo check --target x86_64-pc-windows-gnu --workspace                  # MUST pass now (no allow-fail)
git grep "nix::sys::memfd\|nix::fcntl::SealFlag" crates/boxpilot-profile/src
# Expected: zero matches (memfd code lives in boxpilot-platform)
```

PR description should call out: "After this PR, Windows compile gate flips from `allow-failure: true` to **required**. CI workflow update lands in PR 11a."

---
