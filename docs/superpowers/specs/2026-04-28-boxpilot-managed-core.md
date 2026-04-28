# BoxPilot Plan #2 Design — Managed sing-box Core Lifecycle

Date: 2026-04-28
Status: design draft for user review
Predecessors: plan #1 (skeleton + boxpilotd scaffolding), now under PR review.
Spec sections implemented: §5.2, §5.4, §6.5, §6.6 (claim path), §11.1–§11.3.

## 1. Goal

Implement the five `core.*` actions from the §6.3 helper whitelist
(`core.discover` / `core.install_managed` / `core.upgrade_managed` /
`core.rollback_managed` / `core.adopt`), the `/var/lib/boxpilot/cores/`
file layout (§5.2), the `install-state.json` ledger (§5.4), and the §6.5
trusted-executable-path checks. Plan #2 also closes the controller-claim
hook plan #1 left open in `dispatch::authorize` — `core.install_managed`
is the first authorized mutating action a user is likely to invoke, so
it has to honor §6.6's "first user to complete an authorized write
becomes controller" semantics.

## 2. Scope

### In scope

- `boxpilotd`: 5 typed methods fully implemented behind the existing
  `dispatch::authorize` chokepoint.
- §6.5 trust-check module reusable by plan #3 (service unit generation)
  and plan #5 (activation) without further modification.
- §5.4 `install-state.json` schema fully populated with managed +
  adopted entries.
- Controller-claim-on-commit (§6.6) wired into `dispatch::authorize` and
  the body-commit code paths.
- `boxpilot-tauri`: 5 `#[tauri::command]` wrappers and matching
  TypeScript types in `frontend/src/api/`.
- Frontend: a minimal Settings → Cores panel sufficient to demonstrate
  install / list / rollback / adopt end-to-end. Plan #7 polishes the UX.

### Out of scope (deferred)

- A version dropdown / "install specific version" UX → plan #7.
- GPG / cosign signature verification → future work, not in v1.0.
- The §6.3 whitelist stays at 19 methods; no new methods are added in
  plan #2.

## 3. Locked Decisions

| Area | Decision |
|------|----------|
| Helper scope | full implementation; Tauri commands; minimal Settings → Cores panel. |
| Controller-claim | implicit on first authorized mutating action; persisted at body's commit point, not at dispatch's authorization point. |
| HTTP client | `reqwest` with the `rustls-tls` feature, no native-tls. |
| Checksum policy | fetch upstream `sha256sum.txt` if present and verify; either way compute local SHA256 and record both states; surface "no upstream digest" warning when the upstream file is missing. |
| Version selection UX | helper accepts `version: "latest" | "<exact>"`; plan #2's GUI exposes only `"latest"`. Specific-version dropdown → plan #7. |
| Architecture | auto-detect via `uname -m`; v1.0 supports `x86_64` and `aarch64`; explicit override accepted from helper input. |
| Upgrade vs install | one shared pipeline; upgrade is install + atomic `current` swing. |
| Rollback | atomic `current` swing only; no version directory deletion. |
| GitHub API caching | helper resolves `"latest"` once and caches the result for 5 minutes in-process. |
| Signature verification | not in v1.0 (sha256-only). |

## 4. IPC Contract

The five methods already exist in `boxpilot-ipc::HelperMethod` and as
typed D-Bus methods on `app.boxpilot.Helper1`. Plan #2 fills in their
return types and arguments.

```rust
// All in boxpilot-ipc, additions to existing `response.rs`/new `core.rs`.

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoreDiscoverResponse {
    pub cores: Vec<DiscoveredCore>,
    pub current: Option<String>,           // version or adopted-label
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoveredCore {
    pub kind: CoreKind,                    // External | ManagedInstalled | ManagedAdopted
    pub path: String,                      // absolute, post-symlink-resolution
    pub version: String,                   // sing-box self-reported
    pub sha256: String,                    // hex
    pub installed_at: Option<String>,      // RFC3339; None for external
    pub source: Option<CoreSource>,        // None for external
    pub label: String,                     // version or "adopted-<ts>"; for external: "/usr/bin/sing-box" etc.
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CoreKind { External, ManagedInstalled, ManagedAdopted }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoreSource {
    pub url: Option<String>,               // None for adopted
    pub source_path: Option<String>,       // Some for adopted
    pub upstream_sha256_match: Option<bool>,
    pub computed_sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum VersionRequest {
    Latest,
    Exact { version: String },             // e.g. "1.10.0"
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ArchRequest {
    Auto,
    Exact { arch: String },                // "x86_64" | "aarch64"
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoreInstallRequest {
    pub version: VersionRequest,
    pub architecture: ArchRequest,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoreInstallResponse {
    pub installed: DiscoveredCore,
    pub became_current: bool,
    pub upstream_sha256_match: bool,       // false → UI surfaces "no upstream digest" warning
    pub claimed_controller: bool,          // true if this call made the caller the controller
}

pub struct CoreRollbackRequest { pub to_label: String }   // version like "1.10.0" or "adopted-…"
pub struct CoreAdoptRequest    { pub source_path: String }
```

The wire format remains JSON-encoded `String` per plan #1's convention
(see plan #1 review item I3, deferred).

## 5. File Layout

```
/var/lib/boxpilot/
  cores/
    1.10.0/
      sing-box                # 0755 root:root
      sha256                  # plaintext hex of sing-box, no newline trailer constraint
      install-source.json     # see schema in §5.1 below
    1.10.1/
      sing-box
      sha256
      install-source.json
    adopted-2026-04-28T10-00-00Z/
      sing-box
      sha256
      install-source.json
    current -> 1.10.1/        # symlink swung via rename(2) on a sibling current.new
  install-state.json          # §5.4 schema (full ledger)
  .staging-cores/             # cleared on daemon startup; ephemeral mid-install
/etc/boxpilot/
  boxpilot.toml               # core_path set to /var/lib/boxpilot/cores/current/sing-box
                              # core_state set per CoreState enum from plan #1
                              # controller_uid set on first claim
  controller-name             # username; written by claim path (plain text, 0644)
```

### 5.1 `install-source.json` schema (per-core)

```json
{
  "schema_version": 1,
  "kind": "managed-installed",      // or "managed-adopted"
  "version": "1.10.1",              // for managed; for adopted, the self-reported version
  "architecture": "x86_64",
  "url": "https://github.com/SagerNet/sing-box/releases/download/v1.10.1/sing-box-1.10.1-linux-amd64.tar.gz",
  "source_path": null,              // for adopted: original source path; for managed: null
  "upstream_sha256_match": true,    // null when upstream has no sha256sum.txt
  "computed_sha256_tarball": "<hex>",
  "computed_sha256_binary": "<hex>",
  "installed_at": "2026-04-28T10:00:00-07:00",
  "user_agent_used": "boxpilot/0.2.0"
}
```

For `managed-adopted`, `url` is `null`, `source_path` is the adopted path,
`computed_sha256_tarball` is `null` (no tarball — direct binary copy),
and `computed_sha256_binary` is the hash of the copied binary.

### 5.2 `install-state.json` schema

Mirrors spec §5.4 verbatim, with `schema_version: 1`. The struct in
`boxpilot-ipc` is added in plan #2:

```rust
pub struct InstallState {
    pub schema_version: u32,
    pub managed_cores: Vec<ManagedCoreEntry>,
    pub adopted_cores: Vec<AdoptedCoreEntry>,
    pub current_managed_core: Option<String>,   // label of the current target
}
```

Reads reject unknown `schema_version` the same way `BoxpilotConfig` does.

## 6. Module Layout

```
crates/boxpilotd/src/core/
  mod.rs              # public surface; iface.rs imports from here
  trust.rs            # §6.5 trust-check pipeline
  state.rs            # install-state.json read/write (atomic via tempfile + rename)
  download.rs         # reqwest GET to GitHub releases, redirect-following, streaming to tempfile
  github.rs           # GitHub API client: resolve "latest", fetch sha256sum.txt
                      # holds the 5-minute cache for `latest` resolution
  install.rs          # install / upgrade pipeline (single function, branches on whether `current` exists)
  adopt.rs            # adopt pipeline
  rollback.rs         # rollback (swing current to existing labeled directory)
  discover.rs         # list managed + adopted dirs, probe known external paths
crates/boxpilot-ipc/src/
  core.rs             # response/request types for the 5 methods
  install_state.rs    # InstallState + entry types
crates/boxpilotd/src/
  iface.rs            # 5 method bodies dispatch to core::* and serialize JSON
  dispatch.rs         # gains the `will_claim_controller` field on AuthorizedCall
                      # and the body-commit-time claim helper
crates/boxpilot-tauri/src/
  commands.rs         # 5 new #[tauri::command] wrappers
frontend/src/
  api/types.ts        # 5 new TS types mirroring the Rust IPC types
  api/helper.ts       # 5 new invoke wrappers
  components/CoresPanel.vue   # the Settings → Cores panel
```

## 7. Key Flows

### 7.1 Controller-claim mechanics (§6.6)

`dispatch::authorize` is updated in two ways:

1. The `AuthorizedCall` struct gains `pub will_claim_controller: bool`.
   This is `true` iff:
   - the caller's controller state is `Unset`,
   - the method is mutating,
   - polkit returned `Ok(true)` (the caller authenticated under the
     XML default `auth_admin_keep` because no controller-relaxation
     applies yet).
2. dispatch does NOT write `boxpilot.toml` or `controller-name` itself.
   It returns the lock guard and `will_claim_controller` to the body.

Each mutating body, at its commit point (the atomic state-write
sequence — see install pipeline below), calls a helper:

```rust
// in dispatch.rs (or a new commit.rs):
pub fn maybe_claim_controller(
    paths: &Paths,
    will_claim: bool,
    caller_uid: u32,
    user_lookup: &dyn UserLookup,
) -> HelperResult<Option<ControllerWrites>>;
```

`ControllerWrites` contains the deltas (`controller_uid` value to set in
toml, username to write to `/etc/boxpilot/controller-name`). The body
then merges these into its own atomic commit so both files land or
neither does, under a single rename(2) sequence.

If the body fails before commit: nothing claimed. The next attempt
re-evaluates `Unset` and tries again.

### 7.2 install / upgrade pipeline

Entry: `core::install::run(req, ctx, authorized)`.

1. Resolve version (`Latest` → `github::resolve_latest()`; `Exact` →
   pass through).
2. Resolve architecture (`Auto` → `uname -m` mapped to `x86_64` or
   `aarch64`; reject anything else).
3. Build canonical URL:
   `https://github.com/SagerNet/sing-box/releases/download/v<v>/sing-box-<v>-linux-<arch>.tar.gz`.
4. `core::download::fetch(url, &staging_dir)` streams the tarball to
   `/var/lib/boxpilot/.staging-cores/<version>-<random>/tarball.tar.gz`
   with a User-Agent of `boxpilot/<crate-version>`.
5. `core::github::fetch_sha256sums(version)` GETs `sha256sum.txt`
   from the same release. On 404, returns `None`. On success, parses
   the table and looks for the line matching our tarball filename.
6. If found, compare to a streaming SHA256 of the downloaded tarball;
   mismatch → abort and clean up. If matched, set
   `upstream_sha256_match: true`. If sha256sum.txt absent, set
   `upstream_sha256_match: null`.
7. Compute local SHA256 of the tarball (recorded either way).
8. Extract `sing-box` from the tarball into `staging_dir/sing-box`
   (only this file; ignore LICENSE / docs).
9. `chown 0:0` + `chmod 0755` on the extracted binary (it's already
   that way under root, but be explicit).
10. `core::trust::verify_executable_path(&staging_dir/sing-box)` —
    runs the §6.5 pipeline once before promotion.
11. Spawn `sing-box version` and confirm self-reported version matches
    the requested version (string match on `vX.Y.Z`).
12. Write `staging_dir/sha256` and `staging_dir/install-source.json`.
13. `rename(2)` `staging_dir` → `cores/<version>/`. (atomic; fails if
    target exists, which we treat as "already installed".)
14. **Commit step (single transaction):**
    a. Atomic-write `boxpilot.toml.new` containing the existing fields
       plus `core_path`, `core_state = managed-installed`,
       and (if `will_claim_controller`) `controller_uid = caller_uid`.
    b. Atomic-write `controller-name.new` (if claiming).
    c. Atomic-write `install-state.json.new` with the new managed
       entry appended and `current_managed_core` updated.
    d. Atomic-write a sibling `current.new` symlink pointing at
       `cores/<version>/`.
    e. Issue rename(2) calls in this order: `current.new` → `current`,
       `install-state.json.new` → `install-state.json`,
       `controller-name.new` → `controller-name` (if applicable),
       `boxpilot.toml.new` → `boxpilot.toml`.
15. Release the lock (RAII).

If steps 1-13 fail, staging directory is left for daemon startup to
clean up; nothing user-visible has changed. If step 14 fails between
renames, the daemon's startup-recovery routine (added in plan #2)
re-validates the state files and either rolls back partial commits or
surfaces a `RecoveryRequired` error to the GUI.

### 7.3 adopt pipeline

Entry: `core::adopt::run(req, ctx, authorized)`.

1. Source path validation:
   `trust::verify_executable_path(&req.source_path)` — same checks as
   we'd apply to the managed binary. This rejects `~/.local/bin/...`
   (under `/home`), files with setuid/setgid/sticky bits, group/world
   writable parents, etc.
2. Spawn `sing-box version` to read the binary's reported version.
3. Compute `adopted_label = format!("adopted-{}",
   utc_now_compact_iso8601())` (e.g. `adopted-2026-04-28T10-00-00Z`).
4. Stage in `.staging-cores/<adopted_label>-<random>/`:
   - copy the source binary to `sing-box` (preserve mode).
   - compute and write `sha256`.
   - write `install-source.json` (kind = `managed-adopted`,
     source_path = req.source_path).
5. trust-check the staged binary (defense-in-depth).
6. `rename(2)` staging → `cores/<adopted_label>/`.
7. Commit step:
   a. boxpilot.toml: only `controller_uid` may change (claim);
      `core_path` / `core_state` stay as-is — adoption does NOT
      switch `current`.
   b. controller-name: written if claiming.
   c. install-state.json: append to `adopted_cores`.
8. Lock released.

### 7.4 rollback pipeline

Entry: `core::rollback::run(req, ctx, authorized)`.

1. Resolve `req.to_label` to a directory under `cores/`. If missing,
   return `HelperError::Ipc { message: "no such core: <label>" }`.
2. trust-check `cores/<label>/sing-box`.
3. `sing-box version` smoke test.
4. Commit step:
   - Atomic-write a `current.new` symlink pointing at `cores/<label>/`,
     `rename(2)` over `current`.
   - install-state.json: update `current_managed_core` to `<label>`
     (if it's a versioned dir; for adopted dirs, store the label
     verbatim — `adopted-2026-04-28T10-00-00Z`).
   - boxpilot.toml: `core_path` is `/var/lib/boxpilot/cores/current/sing-box`
     either way, so it's unchanged. `core_state` may flip from
     `managed-installed` ↔ `managed-adopted` depending on the target.
   - controller-name written if claiming.

### 7.5 discover (read-only)

Entry: `core::discover::run(ctx)`.

1. Enumerate `cores/` subdirs:
   - For each managed dir (matches `^\d+\.\d+\.\d+$` or similar):
     read `sha256`, `install-source.json`, run `sing-box version`.
   - For each `adopted-*` dir: same.
2. Probe canonical external paths from a fixed list:
   `/usr/bin/sing-box`, `/usr/local/bin/sing-box`. For each, if file
   exists and trust-check passes, run `sing-box version` and report
   as `External`.
3. Resolve `current` symlink to its label.
4. Build `CoreDiscoverResponse`.

Read-only: no lock; no controller-claim; polkit returns
`allow_any=yes` for `core.discover`.

### 7.6 Daemon startup recovery

`boxpilotd::main` already cleans `.staging/` (per plan #1). Plan #2
extends this:

1. Sweep `/var/lib/boxpilot/.staging-cores/*` and remove.
2. Validate `cores/current` resolves under `cores/`. If broken
   (target missing), unset `core_path` + flag drift to GUI.
3. Validate `install-state.json` parses with `schema_version == 1`.
   If not, refuse to serve mutating methods until a manual recovery
   step runs (`controller.transfer` or out-of-band).

## 8. Trust check pipeline (§6.5)

`core::trust::verify_executable_path(&Path) -> Result<(), TrustError>`
runs in this order; the first failure aborts:

1. Resolve the path through any symlinks, recording every component.
2. For each resolved component, `nix::sys::stat::stat()`:
   - exists; is regular file (the binary) or directory (parents).
3. The binary itself:
   - owner uid == 0 and gid == 0 (root:root).
   - mode bits: not writable by group or others (`mode & 0o022 == 0`).
   - mode bits: setuid/setgid/sticky NOT set (`mode & 0o7000 == 0`).
   - executable for owner.
4. Each parent directory up to `/`:
   - owner uid == 0.
   - not writable by group or others.
5. Final canonical path is under one of:
   - `/usr/bin/`
   - `/usr/local/bin/`
   - `/var/lib/boxpilot/cores/<label>/` (label = version or `adopted-*`)
   - any path explicitly recorded in `install_state.adopted_cores[*].path`
6. Spawn the binary with `--version` (or whatever sing-box accepts;
   `sing-box version` is the canonical command); confirm exit 0 and
   stdout starts with `sing-box version`.

The `FsMetadataProvider` trait abstracts steps 2-4 for testing:

```rust
pub trait FsMetadataProvider: Send + Sync {
    fn stat(&self, path: &Path) -> io::Result<FileStat>;
    fn read_link(&self, path: &Path) -> io::Result<PathBuf>;
}
```

Real impl uses `std::fs::symlink_metadata` + `std::fs::read_link`.
Test impl is a `HashMap<PathBuf, FileStat>` returning canned values.

## 9. Tests

Plan #2 adds approximately 30 unit tests, all using mocks for
filesystem / HTTP / process-spawn so the suite still runs offline as
a normal user.

| Module | Coverage |
|--------|----------|
| `trust.rs` | 12 cases: owner-not-root, group-writable, world-writable, setuid, setgid, sticky, parent-not-root-owned, parent-group-writable, symlink escape, allowed-prefix mismatch, version-spawn fail, happy path |
| `state.rs` | 5 cases: parse v1, reject v2, atomic write round-trip, read missing → empty default, append/update entries |
| `github.rs` | 6 cases: resolve "latest" happy, resolve cache hit, sha256sum.txt happy, sha256sum.txt 404, sha256sum.txt malformed, User-Agent presence |
| `install.rs` | 4 cases: happy install, upstream digest mismatch aborts, version-string mismatch aborts, claim-controller commit |
| `adopt.rs` | 3 cases: happy adopt, source under /home rejected, source with setuid bit rejected |
| `rollback.rs` | 2 cases: happy rollback, missing label errors |
| `discover.rs` | 3 cases: only externals, only managed, mixed managed+adopted+external |

Plan #1's 48 tests stay green throughout.

## 10. Frontend Settings → Cores panel (minimal)

A new `frontend/src/components/CoresPanel.vue` mounted under the
existing app shell as a Settings tab. v1.0 polish lives in plan #7.

Layout:

```
+---------------------------------------------------------+
| Cores                                                   |
+---------------------------------------------------------+
| [Install latest sing-box]  [Refresh]                    |
+---------------------------------------------------------+
| ● 1.10.1 (managed)   sha 4f2c…  active                  |
|   1.10.0 (managed)   sha 8a91…  [Make active]           |
|   /usr/bin/sing-box  (external) v1.9.4 [Adopt]          |
|   adopted-2026-04-28T10-00-00Z (managed-adopted)        |
|     v1.10.0 sha c8e2…  source /usr/local/bin/sing-box   |
+---------------------------------------------------------+
| Adopt from path: [______________________________] [Adopt] |
+---------------------------------------------------------+
| Status: idle | "no upstream digest, computed locally" warn |
+---------------------------------------------------------+
```

Behavior:
- "Install latest" calls `helper_core_install_managed("latest", "auto")`.
  Polkit prompt is the OS's responsibility; the panel shows a spinner
  while the IPC call is in flight.
- After success, refresh discover to update the list.
- "Make active" → `helper_core_rollback_managed(label)`.
- "Adopt" path-input → `helper_core_adopt(path)`. Bad paths (failing
  trust check) return `HelperError::Ipc` with the rejection reason;
  the panel surfaces it inline.
- The "no upstream digest" warning appears next to any managed entry
  whose `install-source.json` records `upstream_sha256_match: null`.

## 11. Acceptance criteria

Plan #2 is acceptable when all of these are true on a supported systemd
desktop distribution:

1. `core.discover` returns external + managed + adopted entries
   correctly on a system that has them.
2. `core.install_managed("latest", auto)` downloads, verifies, and
   installs the upstream-latest sing-box from SagerNet's official
   release URL with no manual configuration.
3. After step 2, `cores/current` resolves to the new version and
   `boxpilot.toml::core_path` is `/var/lib/boxpilot/cores/current/sing-box`.
4. The first `core.install_managed` call from a fresh box claims the
   caller as controller — `boxpilot.toml::controller_uid` and
   `/etc/boxpilot/controller-name` are populated atomically with the
   install commit, not before.
5. A failed install (e.g. simulated network failure between download
   and rename) leaves `current` and `install-state.json` unchanged;
   no controller is claimed.
6. `core.upgrade_managed` to a different version atomically swings
   `current` after the new version is fully staged.
7. `core.rollback_managed(label)` swings `current` to a previously
   installed version still on disk; `install-state.json` updates;
   `cores/<previous>/` is not deleted.
8. `core.adopt(path)` rejects:
   - paths under `/home`, `/tmp`, `/run/user`
   - paths with setuid/setgid/sticky bits set
   - paths owned by non-root or with group/world-writable parents
   - and accepts paths under `/usr/bin/`, `/usr/local/bin/` and any
     other location passing §6.5 trust checks.
9. Adoption copies the binary into `cores/adopted-<ts>/` and does NOT
   change `current`.
10. Upstream `sha256sum.txt` is fetched per install; mismatches abort
    the install; missing-file is recorded as `null` and surfaced as a
    warning in the GUI.
11. Daemon startup cleans `.staging-cores/` and validates `current`'s
    resolution; broken state is surfaced as drift, not silently
    ignored.

## 12. Open questions for plan #2.5 / future work

Not committed in plan #2:

- **GPG / cosign signature verification.** Currently sha256-only;
  upstream can be MITM'd if attacker controls both binary and
  sha256sum.txt. Real fix is signature verification, deferred.
- **Specific-version dropdown UX.** Plan #7 will add a method (or
  reuse `core.discover`'s response shape) for browsing past releases.
- **Concurrent multiple installs.** All `core.*` mutating actions
  share `/run/boxpilot/lock`, so they're serialized. A long install
  can block other actions; the GUI surfaces this as `Busy` (HelperError
  variant from plan #1). Plan #5+ may want finer-grained locking.
- **Mirror / proxy support for SagerNet downloads.** Currently
  hardcoded to `github.com`. Plan #9 may expose a config knob for
  users in regions with throttled GitHub access.
