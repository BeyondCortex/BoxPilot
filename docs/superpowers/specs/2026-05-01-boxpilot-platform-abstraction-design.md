# BoxPilot Platform Abstraction Design

Sub-project #1 of the BoxPilot Windows port.

Date: 2026-05-01
Status: design draft (review COQs resolved; see "Critical Open Questions" for resolution log)
Branch: `feat/windows-support`

# Critical Open Questions

These items surfaced during skeptical review (see "Review Log" at end of
doc). All are now resolved; resolutions are reflected in the spec body.

## COQ1 + COQ2 (combined). Bundle protocol & dispatch FD channel

**Original concern:** §5.2 declared a two-call bundle shape
(`upload→handle`, then `activate_bundle(handle)`) inconsistent with the
Windows wire description (chunks streamed on the same Named Pipe
connection). §5.4's `HelperDispatch::handle(conn, method, body) → result`
had no channel for Linux's D-Bus FD-passing.

**Resolution:** Drop `BundleClient` / `BundleServer` traits entirely.
Bundle preparation in `boxpilot-profile` returns
`(manifest_bytes, AsyncRead, sha256)`; the IPC layer carries the byte
stream as a 4th `aux: AuxStream` parameter on `HelperDispatch::handle`
and on `IpcClient::call`. Platform impls translate native auxiliary
handles into / out of `AuxStream`:

- Linux server: wraps the incoming `OwnedFd` (sealed memfd from D-Bus
  FD-passing) in `tokio::fs::File` → `AuxStream::Bytes`.
- Linux client: takes `AuxStream::Bytes`, copies into a memfd, seals,
  FD-passes via D-Bus.
- Windows server: reads chunked frames from the same Named Pipe
  connection after the request body → `AuxStream::Bytes`.
- Windows client: takes `AuxStream::Bytes`, frames as length-prefixed
  chunks, streams on the same Named Pipe.
- Fakes: in-memory `Cursor<Vec<u8>>`.

The sealed-memfd integrity property is preserved on Linux because the
client-side IPC impl seals the memfd before FD-pass — once shipped, even
the GUI process that built it cannot mutate the bytes the helper
mmaps/reads. On Windows, the equivalent integrity property is the
helper-side SHA256 verification before unpack: the client cannot
substitute bytes mid-flight because the helper hashes everything it
reads.

§5.2 and §5.4 are amended to reflect this.

## COQ3. Authority on Windows blocks AC5 — RESOLVED

**Resolution:** Sub-project #1's Windows `Authority` impl is
`AlwaysAllow`, with a `warn!`-level startup log line stating that
authorization is bypassed pending Sub-project #2. AC5 therefore reaches
the dispatch's `NotImplemented` response. §5.4 amended.

## COQ4. ServiceManager trait shaped against systemd alone — RESOLVED (with deferred risk)

**Resolution:** PR 5 ships `ServiceManager` as a verbatim rename of the
existing `Systemd` trait. Trait surface is **not expanded** to second-guess
SCM semantics; Windows impl is `unimplemented!()` for every method (none
of AC5 needs ServiceManager). Sub-project #2's first task is explicitly
"review `ServiceManager` shape against SCM API; propose schema bump for
`UnitState` if needed". This accepts a known refactor cost in
Sub-project #2 in exchange for keeping Sub-project #1 narrowly scoped.
§5 trait inventory and §10 Sub-project #2 amended.

## COQ5. No tracing sink on Windows Service — RESOLVED

**Resolution:** PR 13 wires a `tracing-appender` daily-rolling file sink
to `%ProgramData%\BoxPilot\logs\boxpilotd.log` before
`service_dispatcher::start` is called. `tracing-appender` is added to
workspace dependencies in PR 1. Event Log integration is deferred to
Sub-project #3 (production telemetry). §4 deps and §6 amended.

## COQ6. AC5 verification needs a client tool — RESOLVED

**Resolution:** New PR 14b (after PR 14) adds a small `boxpilotctl` debug
binary (cross-platform, lives at
`crates/boxpilotd/src/bin/boxpilotctl.rs`). It uses the platform
`IpcClient` to invoke any `HelperMethod` with raw JSON body and prints
the response. AC5 verification: `boxpilotctl service.status` after the
service starts. §3 AC5 verification line and §8 PR table amended.

## COQ7. Workspace `tokio` missing `net` — RESOLVED

**Resolution:** PR 1 bumps workspace `tokio` features to
`["macros", "rt-multi-thread", "signal", "fs", "sync", "net", "io-util"]`.
§4 deps comment + §8 PR 1 amended.

## COQ8. `AuxStream::Bytes(Box<dyn AsyncRead>)` cannot recover an `OwnedFd` — RESOLVED

**Original concern (Round 4 / 4.1):** spec §5.2 said the Linux IpcClient
"detects an `OwnedFd` backing" — not a real Rust mechanism;
`Box<dyn AsyncRead>` cannot be downcast without `Any`.

**Resolution:** `AuxStream` becomes an opaque struct with crate-private
accessors. Public construction:

```rust
pub struct AuxStream { /* private */ }

impl AuxStream {
    pub fn none() -> Self;
    pub fn from_async_read(r: impl AsyncRead + Send + Unpin + 'static) -> Self;
    #[cfg(target_os = "linux")]
    pub fn from_owned_fd(fd: std::os::fd::OwnedFd) -> Self;
}
```

Crate-private inside `boxpilot-platform`:

```rust
pub(crate) enum AuxStreamRepr {
    None,
    AsyncRead(Box<dyn AsyncRead + Send + Unpin>),
    #[cfg(target_os = "linux")]
    LinuxFd(std::os::fd::OwnedFd),
}

impl AuxStream {
    pub(crate) fn into_repr(self) -> AuxStreamRepr { … }
}
```

Linux IpcClient calls `into_repr()`, matches `LinuxFd(fd)` and FD-passes
zero-copy; `AsyncRead(r)` falls back to copying bytes into a fresh
sealed memfd then FD-passing. Linux IpcServer always emits
`AsyncRead(_)` (the `OwnedFd` is wrapped in `tokio::fs::File`); dispatch
sees a uniform AsyncRead. Windows IpcClient sees only `None` or
`AsyncRead(_)`. The `LinuxFd` variant is invisible to Windows-side code
because it's gated. §5.2 amended.

## COQ9. Windows IPC wire format undefined — RESOLVED

**Original concern (Round 4 / 4.2):** §5.4 mentioned
"chunked frames" but did not specify byte-level layout;
`boxpilotctl` and the Windows IpcServer cannot ship without it.

**Resolution:** new §5.4.1 specifies the wire format:

```text
Per-call envelope on the Named Pipe (Windows) or D-Bus message body
(Linux fakes for tests):

  HEADER:
    [u32 magic = 0xB0B91107]   "BoxPilot"
    [u32 method_id]             # boxpilot_ipc::HelperMethod::wire_id()
    [u32 flags]                 # bit0 = aux_present, others reserved
    [u64 body_len]
    [u32 body_sha256_present]   # 0 or 1
    [u8  body_sha256[32]]       # only if present (bundle bytes integrity)
  BODY:
    [u8 ; body_len]             # JSON-serialized request payload
  AUX (only if flags.aux_present):
    repeat:
      [u32 chunk_len]           # 0 means EOF
      [u8 ; chunk_len]
  RESPONSE:
    [u32 status]                # 0 = ok, nonzero = HelperError variant id
    [u64 body_len]
    [u8 ; body_len]             # JSON-serialized response or error detail
```

All multi-byte integers are **little-endian** (matches x86_64 native;
documented to forestall network-byte-order confusion). Method-id and
HelperError-variant-id mappings live in `boxpilot-ipc::method::wire`
as additive accessors (not a schema change to `HelperMethod` itself).

The same envelope is used by:
- Windows Named Pipe IpcServer / IpcClient.
- The cross-platform fake (writes/reads on a `tokio::sync::mpsc` channel
  pair carrying these byte vectors).
- `boxpilotctl` (Windows side; Linux side uses zbus directly).

Linux native impl uses zbus's typed call mechanism (no envelope
serialization; method args are zbus-typed). The envelope format is
Windows-side only. §5.4 + new §5.4.1 amended.

## COQ10. `CallerResolver` does not unify across platforms — RESOLVED

**Original concern (Round 4 / 4.3):** Linux input is D-Bus sender
string, output uid; Windows input is Named Pipe handle, output SID.
A unified trait is fictional.

**Resolution:** `CallerResolver` is **dropped from the trait surface**.
Each platform's IpcServer impl resolves the caller internally and hands
a fully-resolved `CallerPrincipal` to dispatch. Linux IpcServer holds a
`Connection` and calls `GetConnectionUnixUser` per message; Windows
IpcServer accepts a Named Pipe connection and calls
`GetNamedPipeClientProcessId` + `OpenProcessToken` once per connection,
caches the SID for the connection's lifetime.

What changes vs the spec body:
- §5 trait inventory: `CallerResolver` row deleted.
- §5.4 `ConnectionInfo` is what dispatch sees; it already has
  `caller: CallerPrincipal`.
- §8 PR 4: rewritten to "move `Authority` to platform crate; absorb the
  Linux `DBusCallerResolver` into the Linux `IpcServer` impl, **not**
  into a standalone `CallerResolver` trait". The `caller_resolver`
  field on `HelperContext` goes away; dispatch's `authorize` receives
  `CallerPrincipal` directly (see COQ11).

§5 trait inventory + §8 PR 4 amended.

## COQ11. `dispatch::authorize` is heavily Linux-coupled — RESOLVED

**Original concern (Round 4 / 4.4):** Existing `authorize` takes
`sender_bus_name: &str`, returns `caller_uid: u32`, reads
`controller_uid` (u32), passes the D-Bus sender to polkit, hardcodes
`/run/boxpilot/lock`. Refactoring this is a major task spec called "S".

**Resolution:** PR 4 expands to encompass the dispatch refactor.
Re-shaped:

- Old `authorize(ctx, sender_bus_name: &str, method) -> AuthorizedCall`
  becomes `authorize(ctx, principal: &CallerPrincipal, method)
  -> AuthorizedCall`.
- `AuthorizedCall::caller_uid: u32` becomes
  `AuthorizedCall::principal: CallerPrincipal`.
- `ControllerWrites { uid, username }` stays as-is **on Linux**; Windows
  cannot use it because schema field is u32. Sub-project #2 introduces
  `ControllerWrites` v2 alongside `controller_principal` schema bump.
- `ctx.authority.check(action_id, sender_bus_name)` becomes
  `ctx.authority.check(action_id, &principal)`. Linux Authority impl
  internally maps `CallerPrincipal::LinuxUid(u32)` back to a D-Bus
  proxy via the IpcServer's connection (or accepts a `(uid, sender)`
  pair stashed in `ConnectionInfo`).
- `ctx.paths.run_lock()` stays as-is (Linux path); FileLock trait (PR 6)
  cleans this up later.

Sized: **PR 4 = L** (was S). Approximately 400–500 LOC of changes
including test updates. §8 PR 4 amended.

## COQ12. `boxpilot-profile` is Linux-coupled beyond `bundle.rs` — RESOLVED

**Original concern (Round 5 / 5.1):** `store.rs:1` is
`use std::os::unix::fs::PermissionsExt;` at module top — breaks Windows
compile of the entire workspace before PR 12 can possibly fix it.
Plus `meta.rs`, `import.rs`, `remotes.rs` (chmod for 0700/0600).

**Resolution:** New trait `FsPermissions` in `boxpilot-platform`:

```rust
#[async_trait]
pub trait FsPermissions: Send + Sync {
    /// Restrict path to owner-only access. Linux: chmod 0700 dir / 0600
    /// file. Windows: SetSecurityInfo to clear inheritance and grant
    /// only the owner SID full access.
    async fn restrict_to_owner(&self, path: &Path, kind: PathKind) -> Result<()>;
}

pub enum PathKind { Directory, File }
```

PR 3 (currently moves `FsMetadataProvider`/`VersionChecker`/
`UserLookup`) is expanded to also introduce `FsPermissions`. Linux
impl wraps the existing `PermissionsExt::set_mode(0o700/0o600)` calls;
Windows impl uses `windows-sys` `SetSecurityInfo`. All five
`boxpilot-profile` files (`store.rs`, `meta.rs`, `import.rs`,
`remotes.rs`, plus the test in `import.rs`) replace
`use std::os::unix::fs::PermissionsExt;` with calls into the trait.

Sized: PR 3 grows from S to **M**. §8 PR 3 amended; §5 trait
inventory adds `FsPermissions`.

## COQ13. Windows-compile-must-pass gate timing — RESOLVED

**Original concern (Round 5 / 5.2):** `boxpilot-profile/src/bundle.rs`
uses `nix::sys::memfd` until PR 10 moves it. Therefore Windows
compilation cannot pass until PR 10 lands. PR 1's "allow-to-fail" gate
spans PRs 1–9, not just PR 1.

**Resolution:** §3 AC3's verification line is amended:

> CI step `cargo check --target x86_64-pc-windows-gnu --workspace`
> runs **on every PR from PR 1 onward**, but the `allow_failure: true`
> flag is **set through PR 10** (where bundle's nix usage moves out)
> and **dropped at PR 11**. From PR 11 onward, Windows compile is a
> required check. The MSVC target replaces the GNU target at PR 14
> (where the Windows runner is enabled).

§8 PR 1 + PR 11 + PR 14 task descriptions amended to mention the
gate flips. AC3 verification reads "from PR 11 onward, hard-required;
PRs 1–10 allow-to-fail".

## COQ14. `check.rs` subprocess-tree kill on Windows — RESOLVED

**Original concern (Round 5 / 5.3):** `run_singbox_check` does
`unsafe { libc::kill(-pgid, libc::SIGKILL) }` on timeout — no Windows
analog (would need `JobObject` + `TerminateJobObject`).

**Resolution (option (c) from Round 5):** On Windows in this
sub-project, `run_singbox_check` is short-circuited:

```rust
#[cfg(target_os = "windows")]
pub fn run_singbox_check(_: &Path, _: &Path) -> Result<CheckOutput, CheckError> {
    Ok(CheckOutput {
        success: true,
        stdout: "sing-box check skipped on Windows in Sub-project #1".to_string(),
        stderr: String::new(),
    })
}
```

The Linux impl is unchanged; cfg-gated. Real Windows impl with
JobObject moves to Sub-project #2 (when `service.install_managed` /
`profile.activate_bundle` actually need preflight on Windows). Best-
effort preflight per Linux design spec §10 step 3 documents this
fallback as acceptable. New PR 9b or merged into PR 9. §8 amended.

## COQ15. `helper_client.rs` rewrite scope — split PR 11 — RESOLVED

**Original concern (Round 5 / 5.4):** `boxpilot-tauri/src/helper_client.rs`
is 314 lines of typed zbus proxies + `profile_cmds.rs` raw FD-passing
proxy. Spec PR 11 sized "L" understates a near-total rewrite.

**Resolution:** PR 11 splits into:

- **PR 11a (M):** introduce `IpcServer` / `IpcConnection` /
  `HelperDispatch` traits + Linux IpcServer impl (helper-side only).
  `boxpilotd::iface` routes incoming zbus calls through
  `HelperDispatch::handle`. `boxpilot-tauri` is unchanged in this PR.
- **PR 11b (L):** introduce `IpcClient` trait + Linux IpcClient impl;
  rewrite `boxpilot-tauri/src/helper_client.rs` to use
  `IpcClient::call`; absorb the raw `zbus::Proxy` FD-passing code from
  `boxpilot-tauri/src/profile_cmds.rs` into `boxpilot-platform/linux/ipc.rs`.
  Remove `zbus` direct dep from `boxpilot-tauri/Cargo.toml`.

§8 PR 11 split into 11a + 11b, both required before COQ13's gate flip.

## COQ16. `ProfileStorePaths::from_env` plus Tauri `Paths` plumbing — RESOLVED

**Original concern (Round 5 / 5.5):** `from_env()` reads
`XDG_DATA_HOME` / `HOME`; Windows has neither. Migration requires
`Paths` to be threaded through Tauri command state.

**Resolution:** PR 2 (Paths migration) gains a sub-task:

- Delete `boxpilot_profile::store::ProfileStorePaths::from_env()`.
- Add `ProfileStorePaths::from_paths(p: &boxpilot_platform::Paths)`
  that returns the `Paths::user_root().join("profiles")` sub-tree.
- Update Tauri:
  - `boxpilot-tauri/src/main.rs` constructs a single
    `boxpilot_platform::Paths::system()` at startup, registers it as
    a Tauri `tauri::State`.
  - `commands.rs` and `profile_cmds.rs` take
    `tauri::State<'_, boxpilot_platform::Paths>` and pass it to
    `ProfileStorePaths::from_paths(...)` per call.
  - Tests use `Paths::with_root(tmpdir)` as the state.

Sized: PR 2 grows from S to **M**. §8 PR 2 amended.

## COQ17 (carried-over from Rounds 1–4, plan-time-only). Cosmetic / test-only

These are folded into the relevant PRs without dedicated COQs:

- **4.5** spec body says `HelperError::NotImplemented`
  but the existing variant is `HelperError::NotImplemented` with no
  payload. **Decision:** spec uses the existing variant.
  All occurrences of `Unimplemented` in this doc are read as
  `NotImplemented`. Non-goal #5 (no schema change) is preserved.
- **4.6** `AuxStream` early-drop semantics: Linux closes the FD harmlessly;
  Windows IpcServer **must close the Named Pipe connection** rather
  than drain leftover chunked frames, because frame boundaries between
  one request and the next would otherwise be ambiguous. Documented
  in §5.4.1 (the wire format section added by COQ9).
- **4.7** Per-method aux-shape table lives at
  `boxpilot_ipc::method::HelperMethod::aux_shape() -> AuxShape`
  as an additive accessor. PR 11a defines this. Non-goal #5 is
  preserved (additive accessor, no struct/enum change).
- **4.8** PR 4 task list adds: "unit test in `boxpilotd::iface` (or wherever
  `BUS_NAME` / `OBJECT_PATH` end up after the Authority move)
  asserting the constants are unchanged from v0.1.1, with a comment
  explaining the wire-protocol freeze."

---

## 1. Positioning

This is the first of three planned Windows-port sub-projects. After this
sub-project lands:

- Linux users see no behavior change. All Linux smoke procedures and unit
  tests pass. The packaged `.deb` from v0.1.1 continues to work after upgrade.
- The codebase has a new `boxpilot-platform` crate with platform-neutral
  trait interfaces, a complete Linux implementation, Windows stub
  implementations (mostly `unimplemented!()`), and cross-platform fakes.
- Windows can `cargo check --target x86_64-pc-windows-msvc --workspace` and
  `boxpilotd.exe` registers as a Windows Service under SCM, accepts Named
  Pipe connections, and responds with `NotImplemented` to every helper verb.

It does **not** deliver any working Windows feature. The two follow-up
sub-projects are sketched in §11.

The Linux v1.0 design at
`docs/superpowers/specs/2026-04-27-boxpilot-linux-design.md` remains the
authoritative product spec for Linux behavior. This document amends it
only by introducing a portability seam; nothing in §3 (UI Model) or §10
(Activation flow) of the Linux spec changes from a user-visible
standpoint.

## 2. Goals and Non-goals

### Goals

1. **Linux behavior unchanged.** Every Linux smoke procedure (plans #1–#9)
   passes; `cargo test --workspace` is green; the `.deb` upgrade path from
   v0.1.1 produces an indistinguishable runtime.
2. **Comprehensive abstraction face.** Every direct use of `nix::*`,
   `std::os::unix::*`, `libc::`, systemd D-Bus calls, polkit calls, and
   `journalctl` exec is reviewed: it either lives behind a platform trait
   in `boxpilot-platform` or is explicitly `cfg(target_os = "linux")`-gated
   inside the Linux module of that crate.
3. **Windows compiles.** `cargo check` and `cargo build` succeed on
   `x86_64-pc-windows-msvc` for the entire workspace.
4. **Windows minimum boot.** `boxpilotd.exe`, when registered with SCM
   (`sc create boxpilotd binPath= "...\boxpilotd.exe"`) and started,
   transitions through `START_PENDING → RUNNING`, accepts a Named Pipe
   connection on `\\.\pipe\boxpilot-helper`, returns
   `HelperError::NotImplemented` for every helper verb,
   and stops cleanly on `sc stop boxpilotd`.
5. **Cross-platform fake-based tests.** Helper-side unit tests that use
   the new platform fakes pass on both the Linux and Windows CI runners.

### Non-goals (explicitly deferred)

1. Real implementation of any helper verb on Windows. (Sub-project #2.)
2. A Windows installer of any kind (MSI / MSIX / NSIS). Development uses
   manual `sc create`. (Sub-project #3.)
3. Wintun driver install or any TUN configuration.
4. Windows-side GUI text adjustments — wording such as "systemd",
   "polkit", "journal" remains as-is.
5. `boxpilot.toml` schema changes. `controller_uid: u32` stays as-is.
   Windows in this phase does not write `boxpilot.toml`.
6. macOS portability. The trait shapes are not warped to accommodate a
   future macOS port; if one happens it can amend.
7. Rewriting any existing Linux fake/mock. Existing fakes are moved into
   `boxpilot-platform` with their behavior intact.

## 3. Acceptance Criteria

| # | Criterion | Verification |
|---|-----------|-------------|
| AC1 | Linux smoke does not regress | All `docs/superpowers/plans/*-smoke-procedure.md` pass on a fresh Debian/Ubuntu VM |
| AC2 | Linux unit + integration tests green | `cargo test --workspace` on `x86_64-unknown-linux-gnu` |
| AC3 | Windows compiles | CI step `cargo check --target x86_64-pc-windows-{gnu,msvc} --workspace` returns 0. Per COQ13: GNU target runs from PR 1, allow-to-fail through PR 10 (bundle still uses `nix::memfd`); **required from PR 11a onward**. MSVC target replaces GNU at PR 14 once the Windows runner is enabled. |
| AC4 | Windows fake tests run | CI step `cargo test --workspace` on `windows-latest` returns 0 |
| AC5 | Windows boot smoke | Manual: `sc create boxpilotd binPath= "<absolute>"` → `sc start` → run `boxpilotctl service.status` (the debug client added in PR 14b) → response is `NotImplemented` (Authority is `AlwaysAllow` in this phase, so `NotImplemented` is reachable) → `sc stop` exits cleanly |
| AC6 | No leaked OS calls | `rg "nix::|std::os::unix::|libc::" crates/` reports hits only inside `crates/boxpilot-platform/src/linux/` |
| AC7 | `.deb` upgrade preserves state | Install v0.1.1 → activate a profile → upgrade to this build → `boxpilot-sing-box.service` still active, profile still active, `boxpilot.toml` unchanged |

## 4. Crate Structure

A new workspace member is added at `crates/boxpilot-platform/`. No other
crates are added. The shape:

```text
crates/boxpilot-platform/
├── Cargo.toml
└── src/
    ├── lib.rs                    # facade; re-exports per cfg
    ├── traits/                   # always compiled
    │   ├── mod.rs
    │   ├── lock.rs               # FileLock
    │   ├── ipc.rs                # IpcServer, IpcConnection, IpcClient, ConnectionInfo
    │   ├── service.rs            # ServiceManager
    │   ├── trust.rs              # TrustChecker
    │   ├── bundle.rs             # BundleClient, BundleServer
    │   ├── active.rs             # ActivePointer
    │   ├── credentials.rs        # CallerResolver, UserLookup
    │   ├── authority.rs          # Authority
    │   ├── logs.rs               # LogReader
    │   ├── core_assets.rs        # CoreAssetNaming, CoreArchive
    │   ├── fs_meta.rs            # FsMetadataProvider
    │   ├── version.rs            # VersionChecker
    │   └── env.rs                # EnvProvider (system root, local_app_data)
    ├── paths.rs                  # Paths struct (cfg-gated method bodies, not a trait)
    ├── linux/                    # cfg(target_os = "linux")
    │   ├── mod.rs
    │   ├── lock.rs               # flock(2) via fs2
    │   ├── ipc.rs                # zbus
    │   ├── service.rs            # systemd via zbus org.freedesktop.systemd1
    │   ├── trust.rs              # uid + mode-bit + parent-dir checks
    │   ├── bundle.rs             # memfd + F_SEAL_*
    │   ├── active.rs             # symlink + rename(2)
    │   ├── credentials.rs        # GetConnectionUnixUser + getpwuid
    │   ├── authority.rs          # polkit
    │   ├── logs.rs               # journalctl
    │   ├── core_assets.rs        # tar.gz / linux-<arch>.tar.gz
    │   ├── fs_meta.rs
    │   ├── version.rs
    │   └── env.rs
    ├── windows/                  # cfg(target_os = "windows")
    │   ├── mod.rs
    │   ├── lock.rs               # LockFileEx
    │   ├── ipc.rs                # tokio::net::windows::named_pipe + windows-service
    │   ├── service.rs            # SCM via windows-service crate (stub)
    │   ├── trust.rs              # NTFS ACL via windows-sys (stub)
    │   ├── bundle.rs             # streamed Named Pipe protocol (stub)
    │   ├── active.rs             # marker JSON + MoveFileEx (stub)
    │   ├── credentials.rs        # GetNamedPipeClientProcessId + OpenProcessToken (stub)
    │   ├── authority.rs          # SID match → allow / deny (stub)
    │   ├── logs.rs               # EvtQuery (stub)
    │   ├── core_assets.rs        # zip / windows-<arch>.zip (stub)
    │   ├── fs_meta.rs            # (stub)
    │   ├── version.rs            # exec sing-box.exe --version (stub)
    │   └── env.rs                # ProgramData / LocalAppData lookup
    └── fakes/                    # always compiled
        ├── mod.rs
        ├── lock.rs               # in-memory mutex
        ├── ipc.rs                # tokio::sync::mpsc channel pair
        ├── service.rs            # in-memory state
        ├── trust.rs              # always-trusted
        ├── bundle.rs             # in-memory Vec<u8>
        ├── active.rs             # in-memory state
        ├── credentials.rs        # static principal
        ├── authority.rs          # always-allow / always-deny variants
        ├── logs.rs               # in-memory ring buffer
        └── core_assets.rs        # in-memory archive
```

`Cargo.toml` shape (abbreviated):

```toml
[package]
name = "boxpilot-platform"
version = "0.1.1"
edition.workspace = true
license.workspace = true

[dependencies]
boxpilot-ipc.workspace = true
async-trait.workspace = true
tokio.workspace = true                # workspace features extended in PR 1: + "net", + "io-util"
serde.workspace = true
thiserror.workspace = true
tracing.workspace = true
tracing-appender.workspace = true     # added to workspace deps in PR 1; used by Windows tracing file sink

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
```

The downstream crates' role changes:

- `boxpilot-ipc` — unchanged. Stays platform-neutral schema crate.
- `boxpilotd` — drops direct `nix`/`zbus` deps; depends on
  `boxpilot-platform`. `main.rs` grows a `cfg`-gated entry split (Linux:
  zbus + tokio signal loop; Windows: `windows-service::service_dispatcher`).
  Linux-only modules that have no Windows analog
  (`legacy/*`, polkit drop-in writer, systemd unit-text generator, journal
  parsing helpers) keep their location but are wrapped in
  `#[cfg(target_os = "linux")]` and behind feature-equivalent traits or
  cfg-gated module loads.
- `boxpilot-profile` — bundle preparation (validation, asset checking,
  manifest building) stays here. Bundle byte transfer flows through
  `AuxStream` (no separate trait). The `nix::sys::memfd` / `nix::fcntl`
  direct usage in `bundle.rs` is moved to
  `boxpilot-platform/src/linux/bundle.rs`. Beyond `bundle.rs` (per
  COQ12), five files have module-top `use std::os::unix::fs::PermissionsExt;`
  (`store.rs`, `meta.rs`, `import.rs`, `remotes.rs`, plus `import.rs`
  test) — these become `FsPermissions::restrict_to_owner(...)` calls
  through the platform crate. `check.rs::run_singbox_check` is
  short-circuited on Windows in this sub-project (per COQ14); real
  JobObject impl in Sub-project #2. `ProfileStorePaths::from_env()` is
  removed and replaced with `from_paths(&boxpilot_platform::Paths)`
  (per COQ16). nix and libc deps are moved from package-level to
  `[target.'cfg(unix)'.dependencies]`.
- `boxpilot-tauri` — `helper_client.rs` is **rewritten** in PR 11b
  (per COQ15): each typed method body collapses to
  `IpcClient::call(method, body, aux)` + serde wrapping. The custom
  raw-`zbus::Proxy` FD-passing code in `profile_cmds.rs` is absorbed
  into `boxpilot-platform/src/linux/ipc.rs`. zbus direct dep is removed
  from `boxpilot-tauri/Cargo.toml`. Tauri command handlers gain a
  `tauri::State<'_, boxpilot_platform::Paths>` parameter (per COQ16).

## 5. Trait Inventory

| Trait | Linux impl | Windows impl (this phase) | Fake | Originally |
|-------|-----------|---------------------------|------|-----------|
| `Paths` (struct, not trait) | unix layout under `/` | windows layout under `%ProgramData%`, `%LocalAppData%` | `with_root` for tests | partly exists in `boxpilotd::paths` |
| `FileLock` | `flock(2)` via `fs2` | `LockFileEx` (real impl, simple enough to ship now) | `tokio::sync::Mutex` | inline (`boxpilotd/src/lock.rs`) |
| `IpcServer` + `IpcConnection` | zbus `ObjectServer`, system bus name `app.boxpilot.Helper` | `windows-service` driven Named Pipe accept loop on `\\.\pipe\boxpilot-helper` (real for AC5; carries `aux: AuxStream` per call) | `mpsc` channel pair + in-memory `AuxStream` | inline (`boxpilotd/src/iface.rs`) |
| `IpcClient` | zbus client (translates `AuxStream::LinuxFd` → D-Bus FD-pass; `AsyncRead` fallback copies into a fresh sealed memfd) | Named Pipe client (real; chunked-frames `AuxStream::AsyncRead` after the request body, per §5.4.1) | `mpsc` partner | exists (`boxpilot-tauri/src/helper_client.rs`) |
| `ServiceManager` | systemd via zbus (verbatim port of existing `Systemd` trait — surface NOT expanded; SCM-shape redesign deferred to Sub-project #2 per COQ4) | `unimplemented!()` returning `HelperError::NotImplemented` | in-memory state machine | exists as `Systemd` |
| `TrustChecker` | uid + mode bits + parent-dir walk + setuid check | `unimplemented!()` | always-trusted / always-rejected variants | inline (`boxpilotd/src/core/trust.rs`) |
| ~~`BundleClient` / `BundleServer`~~ | **dropped per COQ1+COQ2**; bundle bytes flow via `AuxStream` on the dispatch + IpcClient methods. Bundle preparation in `boxpilot-profile` returns `(manifest, AsyncRead, sha256)` | — | — | — |
| `ActivePointer` | symlink + `rename(2)` | `unimplemented!()` (marker-file design recorded) | in-memory state | inline (`boxpilotd/src/profile/release.rs`) |
| ~~`CallerResolver`~~ | **dropped per COQ10**; each platform's `IpcServer` resolves the caller internally and hands a `CallerPrincipal` to dispatch. Linux internal: `GetConnectionUnixUser`. Windows internal: `GetNamedPipeClientProcessId` + `OpenProcessToken`. Not a cross-platform trait. | — | — | — |
| `UserLookup` | `getpwuid` via nix | `unimplemented!()` | static map | exists (`PasswdLookup`) |
| `FsPermissions` (per COQ12) | chmod 0700 / 0600 via `PermissionsExt` | `SetSecurityInfo` (windows-sys) — owner-only DACL | always-success / record-calls | new — replaces module-top `use std::os::unix::fs::PermissionsExt` in `boxpilot-profile::store/meta/import/remotes` |
| `Authority` | polkit `CheckAuthorization` | **`AlwaysAllow` with startup `warn!` log** (per COQ3); real SID checks deferred to Sub-project #2 | always-allow / always-deny / table-driven | exists (`DBusAuthority`) |
| `LogReader` | `journalctl --unit … -o json` | `unimplemented!()` | in-memory ring buffer | exists (`JournalReader`) |
| `FsMetadataProvider` | `std::fs` + nix metadata | `unimplemented!()` | in-memory map | exists |
| `VersionChecker` | exec `sing-box version` | `unimplemented!()` | static string | exists |
| `CoreAssetNaming` + `CoreArchive` | `sing-box-<v>-linux-<arch>.tar.gz`, tar.gz extract | `sing-box-<v>-windows-<arch>.zip`, zip extract — naming function is real but extract-on-Windows path stays `unimplemented!()` | in-memory naming + extract | inline (`boxpilotd/src/core/install.rs`) |
| `EnvProvider` | reads `$HOME` etc. | reads `%ProgramData%` / `%LocalAppData%` (real on both — used by `Paths`) | static map | new |

Design notes for the four most consequential traits follow.

### 5.1 `Paths`

A struct, not a trait. It holds two roots:

```rust
pub struct Paths {
    system_root: PathBuf,    // Linux "/"; Windows "%ProgramData%\\BoxPilot"
    user_root: PathBuf,      // Linux "$HOME/.local/share/boxpilot"; Windows "%LocalAppData%\\BoxPilot"
}
```

Methods (`boxpilot_toml()`, `cores_dir()`, `releases_dir()`, …) have a
single public signature shared across platforms. Their bodies use
`cfg(target_os)` to assemble the platform-correct path. The historical
`Paths::with_root(tmpdir)` constructor remains, taking *one* root for
Linux compatibility, with the user-root defaulted to a subdirectory of
the same tmpdir on both platforms — preserving every existing test that
already uses it.

This is the only place in the platform crate that isn't a trait. Tests
that exercise path layouts are pure value tests and don't benefit from
trait indirection.

### 5.2 Bundle byte transfer (no separate trait — COQ1+COQ2 resolution; AuxStream shape per COQ8)

Bundles flow as bytes over the same call envelope that carries the
typed verb. There is no `BundleClient` or `BundleServer` trait. The IPC
layer carries an `aux: AuxStream` parameter alongside the typed body
(see §5.4); the dispatch consumes it, the platform-specific IPC impl
plumbs platform-native auxiliary handles through it.

`AuxStream` is an **opaque struct** with crate-private internals (per
COQ8). Public API:

```rust
pub struct AuxStream { /* private */ }

impl AuxStream {
    pub fn none() -> Self;
    pub fn from_async_read(r: impl AsyncRead + Send + Unpin + 'static) -> Self;
    #[cfg(target_os = "linux")]
    pub fn from_owned_fd(fd: std::os::fd::OwnedFd) -> Self;
}
```

Crate-private inside `boxpilot-platform`:

```rust
pub(crate) enum AuxStreamRepr {
    None,
    AsyncRead(Box<dyn AsyncRead + Send + Unpin>),
    #[cfg(target_os = "linux")]
    LinuxFd(std::os::fd::OwnedFd),
}

impl AuxStream {
    pub(crate) fn into_repr(self) -> AuxStreamRepr { … }
}
```

Bundle preparation in `boxpilot-profile`:

```rust
pub struct PreparedBundle {
    pub manifest: ActivationManifest,  // serializes into request body
    pub stream: AuxStream,
    pub sha256: [u8; 32],              // included in the request body for server-side verification
}

pub async fn prepare(
    staging: &Path,
    paths: &boxpilot_platform::Paths,
) -> Result<PreparedBundle, BundleError>;
```

Linux impl of `prepare`: builds tar into a `memfd_create()` FD,
seal-applies `F_SEAL_WRITE | F_SEAL_GROW | F_SEAL_SHRINK | F_SEAL_SEAL`,
returns `AuxStream::from_owned_fd(fd)`. The Linux IpcClient calls
`into_repr()`, matches `LinuxFd(fd)`, and FD-passes it through D-Bus
zero-copy. If a caller hands an `AsyncRead`-backed AuxStream (e.g., from
a fake or test), the Linux IpcClient falls back to copying bytes into a
fresh sealed memfd before FD-passing.

Windows impl of `prepare`: builds tar into a tempfile under
`%LocalAppData%\BoxPilot\tmp\bundle-<id>.tar`, ACL'd to the owner SID
only; returns `AuxStream::from_async_read(File::open(...))`. The
Windows IpcClient calls `into_repr()`, sees `AsyncRead(r)`, and
chunked-frames the bytes per the wire format in §5.4.1.

Linux IpcServer always emits `AsyncRead(_)` to dispatch (the incoming
`OwnedFd` is wrapped in `tokio::fs::File`); dispatch sees a uniform
`AsyncRead`, regardless of platform. The `LinuxFd` variant is invisible
to Windows-side code because the variant is cfg-gated.

Server-side: IpcServer hands the AuxStream to dispatch. Dispatch
hashes-while-reading into the staging dir (single pass, no replay),
compares to `sha256` in the request body, fails with
`HelperError::BundleAssetMismatch` on any mismatch.

Integrity property:

- **Linux** — sealed memfd is immutable post-seal. Even the GUI process
  that built it cannot alter the bytes the helper reads. Hash check is
  defense-in-depth.
- **Windows** — no kernel-level seal. The only integrity guarantee is
  the helper's hash check. The tempfile is owner-ACL'd to keep other
  local users out, but the GUI process could in principle write
  different bytes than `sha256` claims; the helper's hash mismatch
  detects this and aborts.

This is the deepest design decision in this phase. Linux gets a real
impl in PR 10; Windows in Sub-project #2 (the trait surface is decided
now so the protocol contract is locked).

### 5.3 `ActivePointer`

```rust
#[async_trait]
pub trait ActivePointer: Send + Sync {
    async fn read(&self) -> Result<Option<String>>;
    async fn set(&self, release_id: &str) -> Result<()>;
    fn release_dir(&self, release_id: &str) -> PathBuf;
    fn active_resolved(&self) -> Result<Option<PathBuf>>;
}
```

Linux impl: symlink `/etc/boxpilot/active` → `releases/<id>`.
`set()` creates `active.new` and `rename(2)`s it over `active`. Atomic.
`active_resolved()` reads the symlink target. Existing behavior
preserved bit-for-bit.

Windows design (impl deferred): a marker file
`%ProgramData%\BoxPilot\active.json` containing
`{"active_release_id": "<id>", "schema_version": 1}`. `set()` writes
`active.new.json`, `MoveFileEx(MOVEFILE_REPLACE_EXISTING)` over
`active.json`. `active_resolved()` joins
`releases/<id>` from the file's content.

Why marker file rather than NTFS junction: `MoveFileEx` over a plain JSON
file is unaffected by junction-type quirks (cross-volume edge cases, AV
scanners, NTFS-version differences). The ACL-once-at-install model
applies to both `active.json` and `releases/`. Trade-off: an extra read
on every status query, negligible compared to the systemd D-Bus calls
the Linux side makes.

Fake: in-memory `Option<String>` guarded by a mutex.

### 5.4 `IpcServer` / `IpcConnection` / `Authority`

`IpcServer` runs the platform-native acceptance loop. The Linux impl
claims the system D-Bus name and registers an `ObjectServer`. The
Windows impl is the reason this sub-project's "Windows can boot"
acceptance criterion is meaningful: the impl must really work end to end
through Named Pipes — otherwise AC5 fails.

```rust
#[async_trait]
pub trait IpcServer: Send + Sync {
    async fn run(&self, dispatch: Arc<dyn HelperDispatch>) -> Result<()>;
}

#[async_trait]
pub trait HelperDispatch: Send + Sync {
    async fn handle(
        &self,
        conn: ConnectionInfo,
        method: HelperMethod,
        body: Vec<u8>,
        aux: AuxStream,           // see §5.2
    ) -> HelperResult<Vec<u8>>;
}

#[async_trait]
pub trait IpcClient: Send + Sync {
    async fn call(
        &self,
        method: HelperMethod,
        body: Vec<u8>,
        aux: AuxStream,
    ) -> HelperResult<Vec<u8>>;
}

pub struct ConnectionInfo {
    pub caller: CallerPrincipal,
}

pub enum CallerPrincipal {
    LinuxUid(u32),
    WindowsSid(String),  // "S-1-5-21-…"
}
```

For methods that take no auxiliary stream (everything except
`profile.activate_bundle` today), callers pass `AuxStream::none()`.
Dispatch enforces the per-method aux-shape contract: methods that
require aux fail `HelperError::Ipc { ... missing aux ... }` if absent;
methods that forbid aux fail if present. This contract is asserted in
the IPC layer's serialization, not in each verb's body.

The aux-shape table lives at
`boxpilot_ipc::method::HelperMethod::aux_shape() -> AuxShape` (per
COQ17/4.7) — an additive accessor on the existing enum; no schema
change. `AuxShape` enum has variants `None`, `Required`, `Optional`.
Today only `HelperMethod::ProfileActivateBundle` returns `Required`;
all others return `None`.

Drop semantics (per COQ17/4.6): if a verb's body returns an error
before consuming the entire aux stream, the reader is dropped:

- **Linux:** memfd FD closes; no leftover state.
- **Windows:** the Named Pipe still has un-drained chunked frames in
  flight. The Windows IpcServer **must close the Named Pipe
  connection** rather than try to drain leftover bytes — frame
  boundaries between this request and the next would otherwise be
  ambiguous. The client reconnects for its next call. This is
  documented in the wire format (§5.4.1).

`Authority` is invoked by the dispatch layer *after* `IpcServer` resolves
`CallerPrincipal`. Its decision shape is unchanged from the current
boxpilotd code:

- **Linux impl** — polkit `CheckAuthorization`, identical to current
  `DBusAuthority`.
- **Windows impl, this sub-project** — `AlwaysAllow` (per COQ3). On
  startup, `entry::windows::run_under_scm()` emits a single
  `warn!`-level log line: `"windows authority is in pass-through mode
  pending sub-project #2 — do not run on a multi-user machine"`. Real
  SID-based authorization arrives in Sub-project #2 alongside the
  `controller_principal` schema bump.

UAC at the IPC boundary is the wrong shape on Windows because the GUI
process is per-user and unprivileged while the helper service runs as
`LocalSystem` — the elevation step happens at *installer* time, not at
IPC-call time.

### 5.4.1 Windows Named Pipe wire format (per COQ9)

The Linux IpcServer / IpcClient use zbus's typed call mechanism — no
envelope serialization, method args are zbus-typed (this preserves
existing v0.1.1 wire compatibility on the system bus). The wire
format in this section applies **only to the Windows Named Pipe
transport and to the cross-platform fake** (which writes/reads byte
vectors over a `tokio::sync::mpsc` channel pair).

Per-call envelope on `\\.\pipe\boxpilot-helper`:

```text
HEADER (fixed, 60 bytes):
  [u32 magic       = 0xB0B91107]   "BoxPilot" sentinel — IpcServer rejects mismatch.
  [u32 method_id]                   boxpilot_ipc::method::HelperMethod::wire_id().
  [u32 flags]                       bit0: aux_present. bits 1..31 reserved (must be 0).
  [u64 body_len]                    JSON body length in bytes.
  [u32 body_sha256_present]         0 or 1.
  [u8 ; 32]  body_sha256             (zero-filled if not present).
  [u32 reserved] = 0                 padding to 60-byte fixed header.
BODY:
  [u8 ; body_len]                   JSON-serialized request payload.
AUX (only if flags.aux_present):
  repeat:
    [u32 chunk_len]                 0 means EOF — final marker.
    [u8 ; chunk_len]                chunk bytes (0 < chunk_len ≤ 64 KiB).
RESPONSE:
  [u32 status]                      0 = ok; nonzero = HelperError::wire_id().
  [u64 body_len]
  [u8 ; body_len]                   JSON response or error detail.
```

All multi-byte integers are **little-endian** (matches x86_64 native).
Documented explicitly to forestall network-byte-order assumptions.

**Method-id and HelperError-id mappings** live at
`boxpilot_ipc::method::wire` as additive `wire_id()` accessors and
`from_wire_id()` reverse lookups. These are added in PR 11a; they are
**additive accessors** on existing enums, not schema bumps to
`HelperMethod` or `HelperError` (Non-goal #5 preserved).

**Per-call connection lifecycle:** one IPC call = one client connect.
After the response is read, the client closes the connection. Long-
lived connections are an anti-pattern here because:
1. SCM-restart races would invalidate any cached connection.
2. Aux-drop semantics (above) require the connection to be torn down
   on early-error anyway.
The Linux side reuses a single zbus connection across calls because
zbus / D-Bus has its own message-correlation (sender/serial), but
Windows Named Pipes don't. Plan-time may revisit this for
performance once Sub-project #2 lands real verbs.

**Server-side limits enforced before dispatch:**
- `magic` must equal `0xB0B91107` or connection is closed without
  reading further.
- `method_id` unknown to `HelperMethod::from_wire_id()` → response
  status = HelperError::Ipc, no body read.
- `body_len > 4 MiB` (typical request body cap; bundle bytes go in
  AUX, not body) → response status = HelperError::Ipc.
- AUX cumulative size capped per `HelperMethod::aux_size_cap()`
  (defaults to `BUNDLE_MAX_TOTAL_BYTES` for ProfileActivateBundle).

## 6. `boxpilotd` Binary Structure

A single `boxpilotd` bin crate. `main.rs` becomes a thin entry point
that delegates per platform:

```rust
fn main() -> anyhow::Result<()> {
    init_tracing();
    info!(version = env!("CARGO_PKG_VERSION"), "boxpilotd starting");

    #[cfg(target_os = "linux")]
    return entry::linux::run();

    #[cfg(target_os = "windows")]
    return entry::windows::run();
}
```

`entry/linux.rs` keeps the current logic: `ensure_running_as_root`,
build platform impls, spin up `Paths::system()`, build
`HelperContext`, run `IpcServer::run` blocking on SIGTERM/SIGINT.

`entry/windows.rs`:

```rust
pub fn run() -> anyhow::Result<()> {
    if std::env::var("BOXPILOTD_CONSOLE").is_ok() {
        return run_console();          // dev mode, tokio main, no SCM
    }
    windows_service::service_dispatcher::start("boxpilotd", ffi_service_main)?;
    Ok(())
}

fn ffi_service_main(_args: Vec<OsString>) {
    if let Err(e) = run_under_scm() {
        // Best-effort: log to Windows Event Log; SCM has already taken over.
        tracing::error!("service entry failed: {e:?}");
    }
}
```

`run_under_scm()` does these steps **in order**:

1. Initialize a `tracing-subscriber` `Registry` with a daily-rolling
   `tracing-appender` writer pointed at
   `%ProgramData%\BoxPilot\logs\boxpilotd.log`. This is the **first**
   thing run, so any subsequent failure produces a log entry on disk
   even if SCM marks the service `STOPPED` with a generic error code.
   Spec §6.5 (COQ5 resolution): without this sink, traces vanish into
   the SCM-owned dev/null and Windows-side debugging is dark.
2. Emit one `warn!` line: `"windows authority is in pass-through mode
   pending sub-project #2 — do not run on a multi-user machine"` (per
   COQ3 / §5.4 Windows Authority semantics).
3. Register the SCM control handler (handling `Stop`, `Shutdown`,
   `Interrogate`), set status `START_PENDING → RUNNING`.
4. Spawn a background tokio runtime hosting `IpcServer::run`.
5. Block the SCM thread on a stop channel. On `Stop`, status flips to
   `STOP_PENDING → STOPPED`, the IPC server is canceled, the tracing
   appender flushes.

The `BOXPILOTD_CONSOLE=1` escape hatch lets developers exercise the
binary outside SCM during this sub-project; in console mode tracing
also writes to stdout in addition to the log file.

## 7. Windows Path Layout

```text
%ProgramData%\BoxPilot\               (≈ /etc/boxpilot ∪ /var/lib/boxpilot)
├── boxpilot.toml                     (created in Sub-project #2; not present yet)
├── controller-name                   (Linux-only file; not created on Windows)
├── active.json                       (marker file; see §5.3)
├── releases\<activation-id>\
│   ├── config.json
│   ├── assets\
│   └── manifest.json
├── .staging\<activation-id>\
├── cores\
│   ├── <version>\
│   │   ├── sing-box.exe
│   │   ├── sha256
│   │   └── install-source.json
│   └── current\                      (junction; created by Sub-project #2)
├── backups\units\                    (Sub-project #2; service-config snapshots)
├── install-state.json
├── run\lock                          (LockFileEx target)
├── logs\boxpilotd.log                (tracing-appender daily rotation; per COQ5)
└── cache\
    ├── downloads\
    └── diagnostics\

%LocalAppData%\BoxPilot\              (≈ ~/.local/share/boxpilot)
├── profiles\<profile-id>\
│   ├── source.json
│   ├── assets\
│   ├── metadata.json
│   └── last-valid\
├── remotes.json
└── ui-state.json

%ProgramFiles%\BoxPilot\              (binaries; written by installer in Sub-project #3)
├── boxpilot.exe                      (GUI)
├── boxpilotd.exe                     (helper service)
└── resources\
```

ACL strategy (this sub-project sets the design; the actual ACL
application code is in Sub-project #2):

- `%ProgramData%\BoxPilot\` and subtree — Owner: BUILTIN\Administrators.
  ACL: Administrators (Full), SYSTEM (Full), Authenticated Users (Read &
  Execute on read-only paths only). Inheritance enabled for child
  objects.
- `%ProgramData%\BoxPilot\releases\` and `cores\` — same as parent; the
  controller user has read access only.
- `%LocalAppData%\BoxPilot\` — protected via `SetSecurityInfo` to clear
  inheritance and grant only the owner SID full access — equivalent to
  the Linux `0700` semantics for the user profile store.

## 8. PR Sequencing

Each PR keeps Linux green. Windows compilation is added in late PRs.
`feat/windows-support` does not become a long-lived branch; PRs land
back to `main` one at a time, matching the v0.1.0–v0.1.1 cadence.

| # | Subject | Size |
|---|---------|------|
| 1 | scaffold `crates/boxpilot-platform`; add to workspace; empty traits + facade re-export. **Workspace-wide bumps in this PR:** `tokio` features += `["net", "io-util"]` (COQ7); add `tracing-appender` to `[workspace.dependencies]`; move `nix` / `libc` from `boxpilot-profile`'s and `boxpilotd`'s package-level deps to `[target.'cfg(unix)'.dependencies]`. CI: `cargo check --target x86_64-pc-windows-gnu` runs **on every PR, allowed-to-fail through PR 10** (per COQ13 — `boxpilot-profile/bundle.rs` still uses `nix` until PR 10). MSVC target replaces GNU at PR 14. | S |
| 2 | introduce `EnvProvider` and `Paths` value type in `boxpilot-platform`; migrate `boxpilotd::paths::Paths` consumers to platform's `Paths`; Linux impl identical to current. **Also (per COQ16):** delete `boxpilot_profile::store::ProfileStorePaths::from_env()`; add `from_paths(&Paths)`; thread `tauri::State<'_, Paths>` into `boxpilot-tauri` command handlers; tests use `Paths::with_root(tmpdir)`. | M |
| 3 | move `FsMetadataProvider`, `VersionChecker`, `UserLookup` traits + Linux impls to platform; re-host existing fakes; remove originals from `boxpilotd`. **Also (per COQ12):** introduce `FsPermissions` trait; replace module-top `use std::os::unix::fs::PermissionsExt;` in `boxpilot-profile/{store,meta,import,remotes}.rs` with `FsPermissions::restrict_to_owner(...)` calls; Linux impl wraps existing chmod 0700/0600. | M |
| 4 | move `Authority` (renamed from `DBusAuthority`) to platform; Linux behavior identical. **Drop the `CallerResolver` trait per COQ10** — Linux IpcServer absorbs `GetConnectionUnixUser` internally. **Refactor `dispatch::authorize` per COQ11** to take `&CallerPrincipal` (was `sender_bus_name: &str`); rename `AuthorizedCall::caller_uid → principal: CallerPrincipal`. Add a unit test pinning `BUS_NAME == "app.boxpilot.Helper"` and `OBJECT_PATH == "/app/boxpilot/Helper"` (COQ17/4.8). | L |
| 5 | move `Systemd` → `ServiceManager` and `JournalReader` → `LogReader` to platform. **Trait surface NOT expanded** (per COQ4 resolution) — methods, parameter types, return types, and `UnitState` shape are byte-identical to current Linux. Sub-project #2 owns the SCM-shape redesign. | M |
| 6 | introduce `FileLock` trait; replace direct `fs2`/`flock` calls in `boxpilotd::lock`; Linux impl wraps fs2 | S |
| 7 | introduce `TrustChecker` trait; wrap existing `boxpilotd::core::trust` logic as Linux impl | S |
| 8 | introduce `ActivePointer` trait; wrap existing symlink/rename logic in `boxpilotd::profile::release`; tests use fake | S |
| 9 | introduce `CoreAssetNaming` + `CoreArchive`; wrap tar.gz extract logic from `boxpilotd::core::install`. **Also (per COQ14):** `boxpilot_profile::check::run_singbox_check` becomes cfg-gated — Linux retains current pgid+SIGKILL impl; Windows returns a stub `CheckOutput { success: true, stdout: "skipped on Windows in Sub-project #1", … }`. JobObject-based real impl deferred to Sub-project #2. | M |
| 10 | introduce `AuxStream` opaque struct (per COQ8) + bundle-flow refactor (per COQ1+COQ2). `boxpilot-profile::bundle::prepare(staging, paths)` returns `PreparedBundle { manifest, stream: AuxStream, sha256 }`. Linux impl preserves memfd+seal optimization via `AuxStream::from_owned_fd`; consumer side hashes-while-reading. **No `BundleClient` / `BundleServer` traits are introduced.** After this PR lands, `boxpilot-profile/bundle.rs` no longer uses `nix::*` directly → Windows allow-to-fail compile gate flips to **required from PR 11a onward** (COQ13). | L |
| 11a | introduce `IpcServer` / `IpcConnection` + `HelperDispatch::handle(conn, method, body, aux: AuxStream)` (per COQ15 split); Linux IpcServer impl wraps zbus and converts `bundle_fd: OwnedFd` ↔ `AuxStream`; `boxpilotd::iface` routes through dispatch trait. Define `boxpilot_ipc::method::wire` accessors (per COQ9 / COQ17/4.7) — additive `wire_id()` / `from_wire_id()` / `aux_shape()`. **`boxpilot-tauri` unchanged in this PR.** **Windows allow-to-fail flag dropped from CI starting this PR.** | M |
| 11b | introduce `IpcClient` trait + Linux IpcClient impl. **Rewrite `boxpilot-tauri/src/helper_client.rs`** to use `IpcClient::call`; absorb the raw `zbus::Proxy` FD-passing code from `profile_cmds.rs` into `boxpilot-platform/linux/ipc.rs`. Remove `zbus` direct dep from `boxpilot-tauri/Cargo.toml`. | L |
| 12 | add Windows feature dependencies; provide Windows impls. **Real:** `EnvProvider`, `Paths`, `FileLock`, `IpcServer`/`IpcClient` (real for AC5), `Authority` = `AlwaysAllow` (per COQ3), Windows-internal caller resolution via `GetNamedPipeClientProcessId` (real for AC5; absorbed into the Windows IpcServer impl per COQ10), `FsPermissions` (real, ACL-based). **Stub `unimplemented!()`:** everything else. `cargo check --target x86_64-pc-windows-msvc --workspace` passes on the Windows runner enabled in PR 14. | L |
| 13 | `boxpilotd.exe` Windows Service entry: `windows-service::service_dispatcher::start`, SCM control handler, Named Pipe accept loop returning `NotImplemented` for every verb. **Includes `tracing-appender` daily-rolling file sink at `%ProgramData%\BoxPilot\logs\boxpilotd.log` initialized before any IPC server starts** (per COQ5). | M |
| 14 | enable Windows GitHub Actions runner (`windows-latest`); switch CI cargo-check target from `x86_64-pc-windows-gnu` to `x86_64-pc-windows-msvc`; cross-platform fake-based unit tests added; AC3 + AC4 met | S |
| 14b | introduce `boxpilotctl` debug bin at `crates/boxpilotd/src/bin/boxpilotctl.rs` (per COQ6). Cross-platform; uses `IpcClient` to invoke any `HelperMethod` with raw JSON body and prints the response. Used for AC5 verification. Linux dev: `boxpilotctl service.status` → talks D-Bus. Windows dev: same command → talks Named Pipe (per §5.4.1 wire format). | XS |
| 15 | spec doc updates: revise Linux design spec §1 to reference platform abstraction; commit Windows-port roadmap pointing at Sub-projects #2/#3 | XS |

PRs 1–9 are Linux-only refactors. Each ideally reviewable in <300 LOC
of meaningful change; PR 4 (dispatch refactor per COQ11) and PR 10
(bundle / AuxStream) are larger by necessity. PR 11 is split into 11a
(server-side, M) and 11b (client-side rewrite, L) per COQ15. PRs
12–14b are Windows-specific and don't touch Linux runtime behavior.

## 9. Risks

1. **Bundle protocol contract** (PR 10). If the `BundleClient` /
   `BundleServer` traits don't accommodate both memfd+FD-pass *and*
   chunked Named Pipe streaming, the contract has to be revisited
   before Sub-project #2 can implement Windows. Mitigation: the trait
   takes `Bytes + declared_sha256` as the public surface and pushes the
   wire format inside the impl. The Linux impl's choice to use sealed
   memfd internally is invisible across the trait boundary.
2. **Tokio inside `windows-service`** (PR 13). `service_dispatcher::start`
   blocks the calling OS thread. Solution: spawn a tokio runtime on a
   background `std::thread`, communicate via an `mpsc` channel; the SCM
   control handler signals stop through the channel. This is a known
   pattern (e.g., `windows-service` examples).
3. **CI cost of Windows runner** (PR 14). Windows runners are slower
   and more expensive than Ubuntu. Mitigation: keep the Windows job to
   `cargo check` + `cargo test` on the platform crate and dependents
   only; skip Linux-specific integration tests; use path filters so
   pure-Linux PRs can skip Windows CI.
4. **`controller_uid` schema field is Linux-flavored.** This sub-project
   does not bump the schema. Sub-project #2 will introduce
   `controller_principal: { kind: "uid" | "sid", value: ... }` with
   `schema_version=2` and a migration. Risk: a Windows install in this
   sub-project runs without `boxpilot.toml`, so all helper verbs are
   guarded by missing-config returning `NotImplemented`, which matches
   AC4 but means Windows is *not* exercising the controller-claim
   pathway at all yet. That's acceptable — it's the exact deferral.
5. **Existing legacy / migration code is Linux-only.**
   `boxpilotd::legacy::*` parses systemd unit files for the
   `sing-box.service` migration flow. There is no Windows analog. The
   modules are wrapped `#[cfg(target_os = "linux")]` and the
   corresponding helper verbs (`legacy.observe_service`,
   `legacy.migrate_service`) are not exposed on Windows at all. This is
   simpler than fabricating a no-op Windows impl.
6. **Polkit drop-in writer is Linux-only.** Same treatment as legacy: a
   `#[cfg(target_os = "linux")]` module; on Windows the controller
   model uses SID checks done in `Authority` and there is no equivalent
   external file. Sub-project #2 introduces the ACL'd
   `controller_principal` storage in `boxpilot.toml`.
7. **Behavior parity check for moved tests.** Each PR moving an
   existing trait must run that trait's existing fake/mock-driven tests
   from their *new* location and assert identical results before
   deletion of the old tests. Mitigation: PR template requires a "tests
   moved, byte-identical" checklist line.

## 10. Future Sub-projects

### Sub-project #2: Windows v1.0 — real verbs

**First task (per COQ4 resolution):** Review `ServiceManager` trait
shape against SCM API surface. Decide whether the trait's method set
and `UnitState` field shape can host SCM semantics directly, or whether
a schema bump (`UnitState` v2 with platform-neutral `status` field +
opaque `raw: HashMap<String,String>` for platform extras) is needed
before any Windows verb can be implemented. This may also bump
`HelperMethod` schema (e.g., adding `service.set_start_type` because
SCM's "enable" semantics differ from systemd's). Don't ship Windows
verbs on a trait that doesn't fit.

**Second task:** Replace Windows `Authority::AlwaysAllow` with the real
SID-based check. Requires:
- `boxpilot.toml` schema v2 with `controller_principal: { kind: "uid"
  | "sid", value: ... }`
- Migration logic (Linux: read `controller_uid` if v1, write back v2
  on first boot)
- Controller claim flow on Windows using the connecting GUI's
  authenticated SID (via `IpcServer`-resolved `CallerPrincipal`)

**Then the verb impls:**

- `core.discover` / `core.install_managed` / `core.upgrade_managed` /
  `core.rollback_managed`: download SagerNet `windows-<arch>.zip`, ACL
  the install dir, swing `current` junction.
- `service.install_managed`: register `boxpilot-sing-box` Windows
  Service via SCM with the selected core path and `LocalSystem`
  identity. (No CapabilityBoundingSet equivalent on Windows; document
  the asymmetry.)
- `profile.activate_bundle` + `profile.rollback_release`: real bundle
  unpack to `%ProgramData%\BoxPilot\releases\`, marker-file `active`
  swap, SCM service restart, verification window unchanged. **Windows
  bundle integrity** — relies entirely on helper-side SHA256
  verification (no kernel-level seal equivalent of memfd; see §5.2).
- `service.start` / `stop` / `restart` / `status` / `logs` against SCM
  + Event Log.
- Tauri GUI text adjustments: replace "systemd" / "polkit" /
  "journalctl" wording with platform-aware strings.
- `IpcClient` Windows reconnect-on-not-running logic (Windows lacks
  D-Bus auto-activation; the GUI must tolerate transient SCM-restart
  windows).

### Sub-project #3: Windows v1.1 — packaging, drivers, polish

- MSI or NSIS installer that registers `boxpilotd` as an auto-start
  Windows Service, places binaries under `%ProgramFiles%\BoxPilot\`,
  applies the `%ProgramData%\BoxPilot\` ACLs.
- Wintun driver bundling and install flow; surface "TUN unavailable"
  vs "TUN ready" in the Home page like the Linux `/dev/net/tun` check.
- Code signing (Authenticode) for `boxpilotd.exe` and the installer.
- Diagnostics export Windows-side: Event Log redaction, registry
  service-config export with secret filtering.
- Windows-specific drift detection (SCM service still installed,
  binPath unchanged, ACLs not weakened).

## 11. References

- Linux v1.0 design:
  `docs/superpowers/specs/2026-04-27-boxpilot-linux-design.md`
- `windows-service` crate:
  https://docs.rs/windows-service
- Named Pipe security and SO_PEERCRED equivalent: `GetNamedPipeClientProcessId`,
  `OpenProcessToken`, `GetTokenInformation(TokenUser)`.
- `MoveFileEx` semantics:
  https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-movefileexa
- Tokio Named Pipe support:
  https://docs.rs/tokio/latest/tokio/net/windows/named_pipe/index.html
- sing-box Windows release artifacts:
  https://github.com/SagerNet/sing-box/releases

## Review Log

### Round 1 — API surface & wire protocol (perspective: contract design)

**1.1 Bundle trait shape vs Windows wire reality (§5.2).**
The trait says `upload(bytes, sha) → BundleHandle`, then the handle is
consumed by a later verb. The Windows wire description in the same
section streams chunks "over the same Named Pipe connection that issued
the IPC call" — but Named Pipe streams die when the connection closes,
so a `BundleHandle` returned from one call is meaningless to a later
call. Either the Linux model bleeds through (durable handles), or the
Windows model needs a separate upload pipe per handle, or the trait
shape should be one-call streaming. Spec contradicts itself. → COQ1.

**1.2 `HelperDispatch::handle` flattens body to `Vec<u8>`, dropping
FDs (§5.4).** Linux D-Bus FD-passing puts the OwnedFd in the message
envelope, not the body. The trait signature
`(method, Vec<u8>) → Vec<u8>` has nowhere to attach the FD. PR 11
cannot route Linux FDs to bundle verbs without a third channel. → COQ2.

**1.3 `HelperMethod` enum is platform-shared, but verbs diverge
(§9 Risk #5).** `legacy.observe_service` /
`legacy.migrate_service` exist in the enum (it's in
`boxpilot-ipc`, unchanged). Windows dispatch must explicitly handle
them — `Unimplemented` is misleading; they are *Unsupported* on
this platform. AC4 (fake tests) requires fakes cover the full enum.
Worth pinning the response shape. (Not blocking; cosmetic
distinction between `Unimplemented` and `Unsupported`.)

**1.4 `ServiceManager` trait designed against systemd alone risks
mismatch with SCM (§8 PR 5; §10 Sub-project #2).** The existing
`Systemd` trait has methods shaped around the systemd model
(`install_unit(unit_text)`, unit-state enums matching systemd
substates). SCM has a different shape (`CreateService` takes a
struct; service-status enum differs). PR 5 ships the trait verbatim
unless the spec calls out cross-design against SCM. → COQ4.

### Round 2 — AC5 viability & Windows debuggability (perspective: ops & test)

**2.1 Authority denies all calls on Windows when no controller is set
(§5.4 + §9 Risk #4).** Windows `Authority::check` allows only if
`caller == controller_sid`. No `boxpilot.toml` is written in this
sub-project, so `controller_sid` is unset → all calls denied. AC5
expects `Unimplemented`, but every call returns `AccessDenied` first.
AC5 is unprovable as written. → COQ3.

**2.2 No tracing sink on Windows Service (§6).**
`tracing-subscriber` defaults to stdout; Windows Service has no
console. If `service_dispatcher::start` panics or `run_under_scm`
errors before SCM gets `RUNNING`, debugging has zero observable
output. AC5 failure modes will be opaque without an Event Log writer
or file sink. → COQ5.

**2.3 AC5 has no defined verification client (§3 AC5).**
"Connect to Named Pipe → invoke `service.status`" — with what tool?
No `gdbus call` equivalent in repo for Windows. Without a
`boxpilotctl` debug binary or explicit "use GUI" instruction, AC5
sign-off is hand-wavy. → COQ6.

**2.4 SCM start mode and reconnect semantics for Sub-project #2
(§3 AC5; §10).** AC5 uses `start= demand`. Production needs `start=
auto`, plus an `IpcClient` that retries-on-not-running because
Windows lacks D-Bus auto-activation. The trait shape decided in PR 11
must accommodate retry-on-connect; if not, Sub-project #2 reshapes
the trait. (Not blocking #1 sign-off; flag for plan.)

### Round 3 — Build, deps, schema details (perspective: integration & process)

**3.1 Workspace `tokio` features missing `net` (§4 Cargo.toml).**
`tokio::net::windows::named_pipe` requires the `net` feature.
Workspace declares `["macros", "rt-multi-thread", "signal", "fs",
"sync"]` — `net` absent. PR 12 fails to compile until added. PR 1
should bump features. → COQ7.

**3.2 `ConnectionInfo { caller }` drops pid + auxiliary creds
(§5.4).** The current `boxpilotd::credentials::CallerCredentials`
carries `{ uid, pid }` and is used for audit logging. Spec's
`ConnectionInfo { caller: CallerPrincipal }` flattens to principal.
Either keep `pid: Option<u32>` (Some on Linux from
GetConnectionUnixProcessID, Some on Windows from
GetNamedPipeClientProcessId), or document that pid is intentionally
dropped. As written, PR 11 silently regresses audit log fidelity.

**3.3 D-Bus wire names are frozen (`app.boxpilot.Helper`,
`/app/boxpilot/Helper`) (§4 boxpilotd role).** The .deb ships service,
conf, policy files referencing the bus name. A refactor renaming
`DBusCallerResolver → CallerResolver` may inadvertently alter a
constant string. Recommend explicit unit test pinning these as
constants in `boxpilotd::iface` or `boxpilot-platform/src/linux/ipc.rs`,
plus a comment explaining why they cannot change without a deb postinst
migration.

**3.4 PR 12 sized "M" but contains six real Windows impls
(§8 PR 12).** Real `IpcServer` (Named Pipe accept loop with
windows-sys credential lookup) is alone several hundred LOC. Combined
with real Authority + CallerResolver + FileLock + EnvProvider + Paths
this is "L" or should be split into PR 12a (deps + stubs) and PR 12b
(real impls for AC5). Mis-sized PRs fall behind on review SLAs.

**3.5 PR 1 cross-compile target choice (§8 PR 1).**
`cargo check --target x86_64-pc-windows-msvc` on a Linux runner
requires xwin or similar to ship `link.exe` + Windows SDK. Cheaper:
`-windows-gnu` (mingw, free on Linux). Spec is silent. PR 1 may
stall on toolchain setup if MSVC is required from day 1; recommend
GNU early, MSVC native on PR 14.

**3.6 New crate at version 0.1.1 is misleading (§4 Cargo.toml).**
`boxpilot-platform` is fresh; should start at `0.0.1` or `0.1.0`.
Setting `0.1.1` implies a missing 0.1.0. Cosmetic but easy to fix at
scaffolding time.

**3.7 Windows `.staging` dir ACL is a Sub-project #2 problem with a
Sub-project #1 design footprint (§5.2 Windows semantics).**
`%ProgramData%\BoxPilot\.staging\<id>\` inherits the parent ACL
which grants Authenticated Users read+execute. Bundle bytes leak to
local users mid-flight unless `BundleServer::receive` calls
`SetSecurityInfo` to restrict. Windows BundleServer is
`unimplemented!()` in this phase, so no current bug — but the
trait's documented contract must say "implementation MUST ACL the
staging dir before writing bytes" so Sub-project #2 doesn't ship the
hole. Spec is silent on this contract.

### Round 4 — Re-review after COQ resolutions (perspective: post-resolution gaps)

After the COQ1–7 resolutions were folded into the spec body, a re-read
surfaced several issues that the resolutions glossed over or created.

**4.1 `AuxStream::Bytes(Box<dyn AsyncRead>)` cannot recover an
`OwnedFd` for Linux FD-passing (§5.2).** The amended §5.2 says the
Linux IpcClient "detects an `OwnedFd` backing (or uses a small 'would
you like the FD?' escape hatch in the AsyncRead)". This is not a real
Rust mechanism. `Box<dyn AsyncRead>` cannot be downcast to a concrete
type unless it also implements `Any`, and even then the FD is buried
inside the `tokio::fs::File`. To preserve zero-copy FD-passing on Linux
the spec must commit to one of:

- **A.** Add a cfg-gated variant: `#[cfg(target_os = "linux")]
  AuxStream::OwnedFd(std::os::fd::OwnedFd)`. Linux `prepare()` returns
  this variant; Linux IpcClient matches and FD-passes; `AuxStream::Bytes`
  remains the cross-platform fallback.
- **B.** Make `AuxStream` opaque with crate-private accessors:
  `AuxStream::from_owned_fd(fd)` (Linux) and `AuxStream::from_bytes(r)`
  (any), with crate-private `into_linux_fd()` / `into_async_read()` used
  inside the platform crate. Construction and consumption stay in
  `boxpilot-platform`, so the type stays clean publicly.
- **C.** Drop zero-copy: always copy bytes through `AsyncRead`, accept
  the perf hit (≤64 MiB bundles, copy is ≤80 ms on modern HW). Sealing
  is then redundant; rely entirely on SHA256 verification on both
  platforms.

The current spec language does not pick one. **Recommend B**:
keeps `AuxStream` shape platform-agnostic in the public API while
preserving Linux's memfd zero-copy. → blocks PR 10 / PR 11.

**4.2 Windows wire format for IPC is undefined (§5.4 + §8 PR 12 + PR 13
+ PR 14b).** The spec says the Windows IpcServer "carries `aux:
AuxStream` per call" and that PR 14b's `boxpilotctl` uses `IpcClient`,
but does not specify the byte-level frame format on the Named Pipe.
Without it, three concrete things cannot ship:

- The Windows IpcServer impl cannot decode method/body/aux from raw
  bytes off the pipe.
- `boxpilotctl` cannot encode requests.
- The chunked-frame aux protocol (`[u32 len][bytes]` is mentioned but
  never specified end-to-end: e.g., is length network-byte-order? how
  is method name encoded? is body length-prefixed JSON? what's the
  response error envelope?).

Need a §5.4.x sub-section pinning down the frame format. **Blocks PR 12
/ PR 13.**

**4.3 `CallerResolver` trait does not unify cleanly across platforms
(§5 trait inventory; §8 PR 4).** Linux `CallerResolver::resolve(&str)
-> u32` takes a D-Bus sender bus name. Windows analog takes a Named
Pipe handle (or process id from `GetNamedPipeClientProcessId`) and
returns a SID string. Different inputs, different outputs. A unified
trait either:
- Takes an opaque `NativeCallerId` enum that is always one of the two
  variants (callers cfg-gate on construction), making the trait useless
  as an abstraction; or
- Disappears entirely: each platform's IpcServer impl resolves the
  caller internally and hands a `CallerPrincipal` to dispatch, so the
  trait `CallerResolver` lives in `linux/` only (with an analogous
  Windows-internal helper in `windows/`).

The second is cleaner. Spec implies the first by listing
`CallerResolver` as a cross-platform trait in §5. **Blocks PR 4.**

**4.4 `dispatch::authorize` is heavily Linux-coupled — PR scope
underestimated (§8 PR 4 + PR 11).** Inspecting the existing
`boxpilotd::dispatch::authorize`:
- Takes `sender_bus_name: &str` (D-Bus specific input)
- Returns `caller_uid: u32` in `AuthorizedCall` (Linux principal type)
- Reads `controller_uid` from `boxpilot.toml` (schema field is u32)
- Calls `ctx.authority.check(action_id, sender_bus_name)` — passing
  the D-Bus sender to polkit
- Acquires `/run/boxpilot/lock` directly (no `FileLock` trait yet)
- `maybe_claim_controller` returns `ControllerWrites { uid: u32,
  username: String }` (Linux-shaped)

Refactoring this to a platform-neutral `authorize(ctx, principal:
&CallerPrincipal, method)` requires changes to: `AuthorizedCall`,
`ControllerWrites`, `ControllerState`, the `boxpilot.toml`
controller-claim flow, and every caller site in `iface.rs`. None of
this is mechanical. PR 4 (move CallerResolver) and PR 11 (introduce
IPC traits) cannot ship without it. Either:
- Insert a new "PR 4.5: refactor `dispatch::authorize` to operate on
  `CallerPrincipal`, keeping Linux semantics identical", or
- Acknowledge that PR 4 is **L** not **S** and includes the dispatch
  refactor.

**Blocks PR 4 sizing accuracy.**

**4.5 `HelperError` variant naming inconsistency: spec says
`Unimplemented`, code has `NotImplemented` (multiple places).** AC5
and §1 use `HelperError::NotImplemented`. Existing
code at `boxpilotd::iface::to_zbus_err` uses `HelperError::NotImplemented`.
The variant doesn't carry an `os` payload either. This is either:
- A spec drift (intent: keep `NotImplemented`, drop the `{ os }`
  payload, just return the existing variant), or
- A schema bump (add new variant `Unimplemented { os: String }` to
  `boxpilot-ipc::HelperError`, alongside `NotImplemented`)

The latter is `boxpilot-ipc` schema change, which Non-goal #5 prohibits
this phase. **Recommend updating spec to use `NotImplemented` (existing
variant) without the `{ os }` payload.** → cosmetic, but every PR 12 /
13 / AC5 reference is wrong as written.

**4.6 `AuxStream` consumed-once + drop-on-error semantics undocumented
(§5.2 / §5.4).** If a verb's body returns an error before it consumes
the entire `AuxStream::Bytes`, the reader is dropped:
- **Linux:** memfd FD closes, no leftover state.
- **Windows:** the Named Pipe stream still has un-drained chunked
  frames in the pipe. The next request from the same client would
  read these as if they were the next request body. The IpcServer
  must explicitly close the connection on aux-incomplete-drop, or
  drain to EOF before processing the next request.

Spec is silent. → important behavior contract; PR 12 / 13.

**4.7 Per-method aux-shape contract — where does the table live?
(§5.4)** §5.4 says "per-method aux-shape contract enforced at IPC
layer; methods that require aux fail if absent". Implementation needs
a method→shape map, e.g.,

```rust
impl HelperMethod {
    pub fn aux_shape(&self) -> AuxShape {
        match self {
            HelperMethod::ProfileActivateBundle => AuxShape::Required,
            _ => AuxShape::Forbidden,
        }
    }
}
```

This is a `boxpilot-ipc` addition (Non-goal #5 prohibits schema
*changes* but additive accessors are arguably allowed). Or it lives
in `boxpilot-platform` as a free function. Spec doesn't say. **PR 11.**

**4.8 D-Bus wire-name guard test (carried over from 3.3, not addressed
in any PR).** Should be a unit test in PR 4 or 11 asserting
`BUS_NAME == "app.boxpilot.Helper"` and
`OBJECT_PATH == "/app/boxpilot/Helper"`. Trivial; just hasn't been
added to any PR's task list.

### Round 4 priority summary

Blocking plan-writing: 4.1 (AuxStream/OwnedFd), 4.2 (Windows wire
format), 4.3 (CallerResolver unification), 4.4 (dispatch refactor
scope).

Important but defer-to-plan-time: 4.6 (AuxStream drop semantics), 4.7
(aux-shape table location).

Cosmetic: 4.5 (NotImplemented naming), 4.8 (wire-name guard test).

### Round 5 — Cross-platform compile reality (perspective: does the workspace actually compile on Windows?)

The previous rounds focused on `boxpilotd` and the platform crate's
trait surface. A fresh look at `boxpilot-profile` and `boxpilot-tauri`
reveals that the spec's §4 description of these crates' role
("`boxpilot-profile` — bundle preparation stays here, just bundle
*transfer* moves out") is incomplete. Multiple modules across both
crates use Linux-only APIs at the **module top level**, which means
the workspace fails to compile on Windows long before PR 12's "make it
compile" gate.

**5.1 `boxpilot-profile` is Linux-coupled in modules the spec doesn't
mention.**

Beyond `bundle.rs` (memfd, already in the plan), grep finds Unix-only
APIs in:

| File | Line | API | Purpose |
|------|------|-----|---------|
| `store.rs` | 1 | `use std::os::unix::fs::PermissionsExt;` | Setting `0700` / `0600` mode on profile dirs and files |
| `meta.rs` | 57 | same | Same for metadata files |
| `import.rs` | 292 | same | Same in tests |
| `remotes.rs` | 62 | same | Permission setting on remotes.json |
| `check.rs` | 31 | `use std::os::unix::process::CommandExt;` | `cmd.process_group(0)` so SIGKILL kills the subprocess tree |
| `check.rs` | 76 | `unsafe { libc::kill(-pgid, libc::SIGKILL) }` | Process-group-tree kill on `sing-box check` timeout |

`store.rs` line 1 is at module top — the `use` itself fails to compile
on Windows. Since `boxpilot-tauri` depends on `boxpilot-profile`, this
single line breaks the entire workspace's Windows compile.

PR 11 ("introduce IPC traits") cannot be the fix point because these
modules aren't IPC-related. The spec needs explicit PRs for:

- File-mode setting → either a `set_owner_only_perms(path)` helper in
  `boxpilot-platform/src/{linux,windows}/fs_meta.rs`, or cfg-gated
  inline (Linux uses chmod 0600, Windows uses `SetSecurityInfo` to
  restrict to owner SID).
- `check.rs` subprocess-tree kill → a `ChildTree` trait or
  cfg-gated inline (Linux uses pgid + SIGKILL, Windows uses JobObject
  + `TerminateJobObject`).

This is design surface the spec hasn't covered. **Blocks PR 1's
scaffolding goal of "Windows allowed-to-fail check passes" if the
allowed-to-fail bar is "compiles".**

**5.2 `boxpilot-profile/Cargo.toml` declares `nix` and `libc` as
unconditional dependencies (lines 18–19).** These are workspace deps
declared at the package level (`nix = { workspace = true }`, not
`[target.'cfg(unix)'.dependencies]`). On Windows, `nix` itself
mostly does not compile (its modules guard themselves with cfgs but
the crate's existence in the dep graph is fine; what matters is
whether downstream code uses Windows-incompatible items).

The `libc` crate compiles on Windows (it's just types/constants), so
that one is fine. `nix` compiles only the cross-platform subset on
Windows. So the *deps* are technically OK; what breaks is
`boxpilot-profile`'s own use of `nix::sys::memfd`, `nix::fcntl`,
`nix::sys::stat::fstat`. PR 10 (bundle refactor) is supposed to move
all that out. But until PR 10 lands, intermediate PRs (1–9) cannot
gate "Windows compiles" because `bundle.rs` still uses nix.

This means the "Windows allowed-to-fail compile check" introduced in
PR 1 will produce useful CI signal **only after PR 10**. PRs 1–9 must
expect Windows compile to fail and not gate on it. Spec says PR 1's
CI gate is "allowed-to-fail" — fine, but that's true through PR 9 too,
and only flips to "must-pass" after PR 10. The spec's PR 14 "switch
to MSVC and require pass" should be moved earlier (right after PR 10)
or AC3's verification timing clarified.

**5.3 `boxpilot-profile/src/check.rs::run_singbox_check` is a
synchronous function with `std::thread::spawn` for pipe drain.** It is
called from `boxpilotd::profile::checker` and from Tauri commands. The
function:

- Spawns `sing-box check` as `std::process::Command`.
- Sets `process_group(0)` so all descendants share the parent's pgid.
- Drains stdout/stderr via two `std::thread::spawn` threads.
- On timeout, calls `unsafe { libc::kill(-pgid, libc::SIGKILL) }`.

There is **no Windows analog** for `kill(-pgid)`. The right Windows
approach is:

1. Create a `JobObject` with `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`.
2. `AssignProcessToJobObject(child)` immediately after spawn.
3. On timeout, `TerminateJobObject(job, 1)` — kills the child and
   any descendants it spawned.

This is a meaningful chunk of code (~50 LOC on each platform) and a
new trait or cfg-gated module. The spec doesn't have it. PR 9
("CoreAssetNaming + CoreArchive") doesn't cover process management.
**Blocks `boxpilot-profile` Windows compile.** Mitigation options:

- (a) Add a `ChildTree` trait to `boxpilot-platform`; `run_singbox_check`
  becomes `async` and consumes it.
- (b) Cfg-gate the kill code inside `check.rs`: Linux block + Windows
  block (no trait).
- (c) Defer `sing-box check` from running on Windows entirely; the
  preflight is "best-effort" already (Linux spec §10 step 3) — return
  `CheckOutput { success: true, stdout: "skipped on Windows", … }`
  in this phase, real impl in Sub-project #2.

(c) is the lightest for Sub-project #1 since AC4/AC5 don't need
preflight to actually run on Windows. **Recommend (c) for Sub-project
#1, then (a) in Sub-project #2.**

**5.4 `boxpilot-tauri::helper_client.rs` refactor scope is a
near-total rewrite, not "use IpcClient instead of zbus".** The file is
314 lines of typed `#[zbus::proxy]` method declarations + per-verb
serde wrappers. With the new `IpcClient::call(method, body, aux)`
shape, every typed method body collapses to:

```rust
let body = serde_json::to_vec(&req)?;
let resp = self.ipc.call(HelperMethod::ServiceStart, body, AuxStream::None).await?;
serde_json::from_slice(&resp)?
```

Plus `boxpilot-tauri/src/profile_cmds.rs` contains the custom raw
`zbus::Proxy` for `ProfileActivateBundle` (the FD-passing code that
the typed proxy macro can't express; documented at `helper_client.rs`
lines 49–52). All of that moves to `boxpilot-platform/linux/ipc.rs`.

Spec PR 11 sized "L"; this brings it close to "very-L". Recommend
splitting:

- **PR 11a:** introduce `IpcServer` / `IpcConnection` /
  `HelperDispatch` traits + Linux IpcServer impl (helper-side only);
  `boxpilotd::iface` routes through dispatch trait. boxpilot-tauri
  unchanged.
- **PR 11b:** introduce `IpcClient` trait + Linux client impl; rewrite
  `helper_client.rs` and absorb `profile_cmds.rs`'s raw-proxy code
  into the Linux IpcClient.

PR 11a is "M", PR 11b is "L". Two ~ 300-LOC PRs each are easier to
review than one ~600-LOC PR. **Plan-time scope decision.**

**5.5 `ProfileStorePaths::from_env()` reads `XDG_DATA_HOME` /
`HOME`** (`store.rs:16-25`). These env vars are unset on Windows. On
Windows the equivalent is `%LocalAppData%`, which `EnvProvider`
resolves. The fix is to make `ProfileStorePaths` consume a
`boxpilot_platform::Paths` (which has `user_root` per §5.1) rather
than reading env vars directly. Spec §4 says boxpilot-profile depends
on `boxpilot-platform` — but doesn't call out that `from_env` needs
to go through it. PR 2 (Paths migration) should also delete
`from_env()` and force callers to use `Paths::system()` /
`Paths::with_root(...)`.

This affects every call site that currently does
`ProfileStorePaths::from_env()` — primarily
`boxpilot-tauri/src/commands.rs` and `profile_cmds.rs`. Those Tauri
command handlers will need a `Paths` instance, which means the Tauri
runtime state (`tauri::State`) must hold one. Cross-cutting refactor;
spec is silent.

### Round 5 priority summary

Blocking compile on Windows: 5.1 (Unix `PermissionsExt` at module top
in `boxpilot-profile`), 5.2 (timing of "Windows compile must pass"
gate is not PR 1 but PR 10), 5.3 (`check.rs` subprocess-tree kill —
recommend skip on Windows for Sub-project #1).

Underestimated PR scope: 5.4 (split PR 11 into 11a + 11b),
5.5 (`ProfileStorePaths::from_env` rewrite + Tauri state plumbing).

### Stop criterion

Round 5 found five compile-realism issues all anchored in
`boxpilot-profile` and `boxpilot-tauri` having Linux-coupling that the
trait surface alone doesn't cover. After resolving these, a sixth
round would surface in-the-weeds detail: the exact `tracing-appender`
log rotation config, `JobObject` HRESULT mapping, Windows panicking
service exit codes, etc. — all genuinely plan-time concerns. Calling
review complete.

<promise>SPEC_REVIEW_DONE</promise>
