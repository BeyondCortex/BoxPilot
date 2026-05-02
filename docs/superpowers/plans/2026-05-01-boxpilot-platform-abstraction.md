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
