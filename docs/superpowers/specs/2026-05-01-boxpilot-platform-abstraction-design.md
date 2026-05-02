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
the dispatch's `Unimplemented` response. §5.4 amended.

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
  Pipe connections, and responds with `Unimplemented` to every helper verb.

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
   `HelperError::Unimplemented { os: "windows" }` for every helper verb,
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
| AC3 | Windows compiles | CI step `cargo check --target x86_64-pc-windows-msvc --workspace` returns 0 |
| AC4 | Windows fake tests run | CI step `cargo test --workspace` on `windows-latest` returns 0 |
| AC5 | Windows boot smoke | Manual: `sc create boxpilotd binPath= "<absolute>"` → `sc start` → run `boxpilotctl service.status` (the debug client added in PR 14b) → response is `Unimplemented` (Authority is `AlwaysAllow` in this phase, so `Unimplemented` is reachable) → `sc stop` exits cleanly |
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
  manifest building) stays here. Bundle *transfer* is now
  `BundleClient` from `boxpilot-platform`. The `nix::sys::memfd` /
  `nix::fcntl` direct usage in `bundle.rs` is moved to
  `boxpilot-platform/src/linux/bundle.rs`.
- `boxpilot-tauri` — `helper_client.rs` now wraps an `IpcClient` from
  `boxpilot-platform`. Tauri command handlers are unchanged.

## 5. Trait Inventory

| Trait | Linux impl | Windows impl (this phase) | Fake | Originally |
|-------|-----------|---------------------------|------|-----------|
| `Paths` (struct, not trait) | unix layout under `/` | windows layout under `%ProgramData%`, `%LocalAppData%` | `with_root` for tests | partly exists in `boxpilotd::paths` |
| `FileLock` | `flock(2)` via `fs2` | `LockFileEx` (real impl, simple enough to ship now) | `tokio::sync::Mutex` | inline (`boxpilotd/src/lock.rs`) |
| `IpcServer` + `IpcConnection` | zbus `ObjectServer`, system bus name `app.boxpilot.Helper` | `windows-service` driven Named Pipe accept loop on `\\.\pipe\boxpilot-helper` (real for AC5; carries `aux: AuxStream` per call) | `mpsc` channel pair + in-memory `AuxStream` | inline (`boxpilotd/src/iface.rs`) |
| `IpcClient` | zbus client (FD-passes a sealed memfd built from `AuxStream::Bytes`) | Named Pipe client (real; chunked-frames `AuxStream::Bytes` after the request body) | `mpsc` partner | exists (`boxpilot-tauri/src/helper_client.rs`) |
| `ServiceManager` | systemd via zbus (verbatim port of existing `Systemd` trait — surface NOT expanded; SCM-shape redesign deferred to Sub-project #2 per COQ4) | `unimplemented!()` returning `HelperError::Unimplemented` | in-memory state machine | exists as `Systemd` |
| `TrustChecker` | uid + mode bits + parent-dir walk + setuid check | `unimplemented!()` | always-trusted / always-rejected variants | inline (`boxpilotd/src/core/trust.rs`) |
| ~~`BundleClient` / `BundleServer`~~ | **dropped per COQ1+COQ2**; bundle bytes flow via `AuxStream` on the dispatch + IpcClient methods. Bundle preparation in `boxpilot-profile` returns `(manifest, AsyncRead, sha256)` | — | — | — |
| `ActivePointer` | symlink + `rename(2)` | `unimplemented!()` (marker-file design recorded) | in-memory state | inline (`boxpilotd/src/profile/release.rs`) |
| `CallerResolver` | `GetConnectionUnixUser` over zbus | `unimplemented!()` (real for AC5: resolves Named Pipe peer via `GetNamedPipeClientProcessId` → SID) | static `CallerPrincipal` | exists (`DBusCallerResolver`) |
| `UserLookup` | `getpwuid` via nix | `unimplemented!()` | static map | exists (`PasswdLookup`) |
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

### 5.2 Bundle byte transfer (no separate trait — COQ1+COQ2 resolution)

Bundles flow as bytes over the same call envelope that carries the
typed verb. There is no `BundleClient` or `BundleServer` trait. The IPC
layer carries an `aux: AuxStream` parameter alongside the typed body
(see §5.4); the dispatch consumes it, the platform-specific IPC impl
plumbs platform-native auxiliary handles through it.

```rust
pub enum AuxStream {
    None,
    /// A read-only byte source. Reader is consumed once by the dispatch.
    /// Caller-side: typically constructed from a memfd (Linux), tempfile
    /// (Windows), or `Cursor<Vec<u8>>` (fakes).
    /// Bytes are subject to per-method size caps applied at the dispatch
    /// layer (`HelperError::BundleTooLarge` if exceeded).
    Bytes(Box<dyn AsyncRead + Send + Unpin>),
}
```

Bundle preparation in `boxpilot-profile`:

```rust
pub struct PreparedBundle {
    pub manifest: ActivationManifest,  // serializes into request body
    pub stream: AuxStream,             // AsyncRead over tar bytes
    pub sha256: [u8; 32],              // included in the request body for server-side verification
}

pub async fn prepare(staging: &Path) -> Result<PreparedBundle, BundleError>;
```

Linux impl of `prepare`: builds tar into a `memfd_create()` FD,
seal-applies `F_SEAL_WRITE | F_SEAL_GROW | F_SEAL_SHRINK | F_SEAL_SEAL`,
wraps the now-immutable FD as `tokio::fs::File` and returns it as
`AuxStream::Bytes`. The IpcClient (Linux) receives `AuxStream::Bytes`,
detects an `OwnedFd` backing (or uses a small "would you like the FD?"
escape hatch in the AsyncRead), and FD-passes it through D-Bus zero-copy.

Windows impl of `prepare`: builds tar into a tempfile under
`%LocalAppData%\BoxPilot\tmp\bundle-<id>.tar`, ACL'd to the owner SID
only; returns the file as `AuxStream::Bytes`. The IpcClient (Windows)
chunked-frames the bytes (`[u32 len][bytes]`) on the same Named Pipe
after writing the request body, ending with a zero-length frame.

Server-side: IpcServer hands `AuxStream::Bytes` to dispatch. Dispatch
hashes-while-reading into the staging dir, compares to `sha256` in the
request body, fails with `HelperError::BundleAssetMismatch` on any
mismatch.

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
`profile.activate_bundle` today), callers pass `AuxStream::None`.
Dispatch enforces the per-method aux-shape contract: methods that
require aux fail `HelperError::Ipc { ... missing aux ... }` if absent;
methods that forbid aux fail if present. This contract is asserted in
the IPC layer's serialization, not in each verb's body.

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
| 1 | scaffold `crates/boxpilot-platform`; add to workspace; empty traits + facade re-export. **Workspace-wide bumps in this PR:** `tokio` features += `["net", "io-util"]` (COQ7); add `tracing-appender` to `[workspace.dependencies]` (COQ5). CI: `cargo check --target x86_64-pc-windows-gnu` (cheap, no MSVC linker setup; "allowed-to-fail" gate); MSVC target enabled in PR 14. | XS |
| 2 | introduce `EnvProvider` and `Paths` value type in `boxpilot-platform`; migrate `boxpilotd::paths::Paths` consumers to platform's `Paths`; Linux impl identical to current | S |
| 3 | move `FsMetadataProvider`, `VersionChecker`, `UserLookup` traits + Linux impls to platform; re-host existing fakes; remove originals from `boxpilotd` | S |
| 4 | move `CallerResolver` (renamed from `DBusCallerResolver`) and `Authority` (renamed from `DBusAuthority`) to platform; Linux behavior identical | S |
| 5 | move `Systemd` → `ServiceManager` and `JournalReader` → `LogReader` to platform. **Trait surface NOT expanded** (per COQ4 resolution) — methods, parameter types, return types, and `UnitState` shape are byte-identical to current Linux. Sub-project #2 owns the SCM-shape redesign. | M |
| 6 | introduce `FileLock` trait; replace direct `fs2`/`flock` calls in `boxpilotd::lock`; Linux impl wraps fs2 | S |
| 7 | introduce `TrustChecker` trait; wrap existing `boxpilotd::core::trust` logic as Linux impl | S |
| 8 | introduce `ActivePointer` trait; wrap existing symlink/rename logic in `boxpilotd::profile::release`; tests use fake | S |
| 9 | introduce `CoreAssetNaming` + `CoreArchive`; wrap tar.gz extract logic from `boxpilotd::core::install` | S |
| 10 | introduce `AuxStream` enum + bundle-flow refactor (per COQ1+COQ2 resolution). `boxpilot-profile::bundle::prepare()` returns `PreparedBundle { manifest, stream: AuxStream, sha256 }`. Linux impl preserves memfd+seal optimization internally; consumer side hashes-while-reading. **No `BundleClient` / `BundleServer` traits are introduced.** | L |
| 11 | introduce `IpcServer` / `IpcConnection` / `IpcClient` + `HelperDispatch::handle(conn, method, body, aux: AuxStream)`; Linux impl wraps zbus and converts `bundle_fd: OwnedFd` ↔ `AuxStream::Bytes`; `boxpilotd::iface` and `boxpilot-tauri::helper_client` route through traits. Per-method aux-shape contract enforced at IPC layer. | L |
| 12 | add Windows feature dependencies; provide Windows impls. **Real:** `EnvProvider`, `Paths`, `FileLock`, `IpcServer`/`IpcClient` (real for AC5), `Authority` = `AlwaysAllow` (per COQ3), `CallerResolver` (real for AC5: `GetNamedPipeClientProcessId` → SID). **Stub `unimplemented!()`:** everything else. `cargo check --target x86_64-pc-windows-msvc --workspace` passes on the Windows runner enabled in PR 14. **Sized:** L (not M as originally drafted; see Risk #4). | L |
| 13 | `boxpilotd.exe` Windows Service entry: `windows-service::service_dispatcher::start`, SCM control handler, Named Pipe accept loop returning `Unimplemented` for every verb. **Includes `tracing-appender` daily-rolling file sink at `%ProgramData%\BoxPilot\logs\boxpilotd.log` initialized before any IPC server starts** (per COQ5). | M |
| 14 | enable Windows GitHub Actions runner (`windows-latest`); switch PR 1's cargo-check target to `x86_64-pc-windows-msvc`; cross-platform fake-based unit tests added; AC3 + AC4 met | S |
| 14b | introduce `boxpilotctl` debug bin at `crates/boxpilotd/src/bin/boxpilotctl.rs` (per COQ6). Cross-platform; uses `IpcClient` to invoke any `HelperMethod` with raw JSON body and prints the response. Used for AC5 verification. Linux dev: `boxpilotctl service.status` → talks D-Bus. Windows dev: same command → talks Named Pipe. | XS |
| 15 | spec doc updates: revise Linux design spec §1 to reference platform abstraction; commit Windows-port roadmap pointing at Sub-projects #2/#3 | XS |

PRs 1–9 are Linux-only refactors and should each be reviewable in <300
LOC of meaningful change. PR 10 (bundle) and PR 11 (IPC) are larger by
necessity. PRs 12–14b are Windows-specific and don't touch Linux runtime
behavior.

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
   guarded by missing-config returning `Unimplemented`, which matches
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

### Stop criterion

Three rounds covered: API contract (Round 1), AC5/observability
(Round 2), build/dep/process minutiae (Round 3). The remaining
unaddressed items I can identify (e.g., specific tokio version vs
windows-service 0.7 compat; whether `nix` should be removed from
each crate's Cargo.toml individually; whether MSVC vs GNU runner is
in plan-time vs spec-time scope) are derivative of the items
already raised, and would not surface new categorical risks. Calling
review complete.

<promise>SPEC_REVIEW_DONE</promise>
