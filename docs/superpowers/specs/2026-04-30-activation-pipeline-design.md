# BoxPilot Plan #5 — Activation Pipeline (fd-passing + atomic rename)

**Date:** 2026-04-30
**Spec parent:** `docs/superpowers/specs/2026-04-27-boxpilot-linux-design.md` §9.2 / §10 / §7.2 / §13
**Status:** Design (pre-plan)

## 1. Scope

Plan #5 implements the system-side activation pipeline that turns a user-prepared
profile bundle into a verified-running release. Concretely:

In scope:

- `profile.activate_bundle` D-Bus verb — replace stub in `iface.rs:171-177`.
- `profile.rollback_release` D-Bus verb — replace stub in `iface.rs:178-184`.
- fd-passing transport (sealed memfd + plain tar) per §9.2.
- `boxpilotd` bundle unpacker enforcing every §9.2 rejection rule.
- §10 full 15-step transaction including auto-rollback and the two terminal
  failure surfaces (`rollback_target_missing`, `rollback_unstartable`).
- Wire `service::verify::wait_for_running` (already shipped in plan #3,
  currently `#[allow(dead_code)]`).
- `/etc/boxpilot/active` symlink atomic swap via `rename(2)` on `active.new`.
- `boxpilot.toml` extension: `previous_*` fields and `active_*` write path.
- Crash recovery on daemon startup: clean `.staging/`, validate `active`.
- Garbage collection of `releases/` per §10 retention rules.
- User-side `prepare_bundle` returns a sealed memfd in addition to a staging
  directory (the directory becomes a debug aid, not the transport).
- Tauri user-side `profile_activate` command wiring (calls daemon with the
  memfd; no GUI changes — that is plan #7).

Out of scope (deferred to other plans):

- Existing `sing-box.service` observation/migration (plan #6).
- GUI progress UX, error surfacing, history view (plan #7).
- Diagnostics export with schema-aware redaction (plan #8).
- `.deb` packaging (plan #9).

## 2. Dependencies on prior plans

| From plan | Contract this plan relies on |
|-----------|------------------------------|
| #1 | D-Bus iface chokepoint, `dispatch::authorize`, `Paths::with_root` |
| #2 | `core::commit::StateCommit` atomic-write pattern |
| #3 | `service::verify::wait_for_running`, `service::install` for unit unchanged |
| #4 | `boxpilot_ipc::profile::ActivationManifest`, `boxpilot-profile::bundle::prepare_bundle`, `BoxpilotConfig::active_*` fields |

## 3. Transport: sealed memfd + plain tar

### 3.1 Why memfd

Spec §9.2 offers two options: sealed `memfd` or chunked stream over a passed
fd. We pick memfd for these reasons:

- The §9.2 total cap is 64 MiB. Holding that in RAM once during activation is
  trivial cost on any machine that runs sing-box.
- A sealed memfd is a single immutable artifact: once `F_SEAL_WRITE`,
  `F_SEAL_GROW`, `F_SEAL_SHRINK`, and `F_SEAL_SEAL` are set, the kernel
  guarantees the bytes the daemon reads are exactly the bytes the user wrote.
  No race, no double-read, no need to checksum the channel itself.
- Single-shot transfers compose cleanly with retry logic — if the call fails
  the GUI can resubmit the same memfd or build a fresh one.
- Pipe-based chunked streams need a writer task on the user side, which adds a
  process boundary inside the GUI and complicates cancellation. Pipes also
  give the daemon no way to verify total size up-front; it has to read first
  and reject later.

### 3.2 Format

Plain `tar` (POSIX ustar). No compression. Reasons:

- `tar` is universally well-tested. Compression libraries add bytes-of-input
  parsing surface for no functional benefit when the cap is 64 MiB.
- Plain tar lets the daemon enforce per-entry size limits before allocating
  buffers — header tells us the size; we reject before reading the body.
- The user-side bundle prep already produces a directory; converting that to
  `tar` is a few lines using the `tar` crate.

Layout inside the tar:

```text
config.json
manifest.json
assets/<...>
```

Same names as §9.2. Path separators normalized to `/` on the user side; the
daemon rejects backslashes anyway.

### 3.3 Sealing

User side (boxpilot-profile crate):

```text
fd = memfd_create("boxpilot-bundle", MFD_CLOEXEC | MFD_ALLOW_SEALING)
write tar bytes
fcntl(fd, F_ADD_SEALS, F_SEAL_WRITE | F_SEAL_GROW | F_SEAL_SHRINK | F_SEAL_SEAL)
```

`F_SEAL_SEAL` blocks any future seal change, making the file's identity
permanent for the life of the fd. `boxpilotd` does NOT verify seals —
verification happens implicitly: the daemon `mmap`s the fd read-only, and any
attempt by the user to mutate the bytes after sealing would have failed at
seal time. This avoids paying for a redundant seal-readback over D-Bus.

### 3.4 D-Bus signature

`profile.activate_bundle` (D-Bus method `ProfileActivateBundle`):

- Args: `(s, h)` where `s = JSON request body`, `h = UNIX_FD index` per
  D-Bus UNIX_FD spec.
- JSON request: `ActivateBundleRequest`
  ```json
  {
    "verify_window_secs": 5,
    "expected_total_bytes": 12345
  }
  ```
- Reply: `s = JSON response body`. `ActivateBundleResponse`:
  ```json
  {
    "outcome": "active" | "rolled_back" | "rollback_target_missing" | "rollback_unstartable",
    "activation_id": "2026-04-30T00-00-00Z-abc123",
    "previous_activation_id": "2026-04-29T...",
    "verify": {
      "window_used_ms": 4321,
      "n_restarts_pre": 2,
      "n_restarts_post": 2,
      "final_unit_state": { ... }
    }
  }
  ```

`expected_total_bytes` is a soft hint the daemon uses to short-circuit
oversized bundles before mmap; the §9.2 hard caps still apply during walk.

`profile.rollback_release` (D-Bus method `ProfileRollbackRelease`):

- Args: `(s)` only — no fd. JSON request:
  ```json
  {
    "target_activation_id": "2026-04-29T...",
    "verify_window_secs": 5
  }
  ```
- Reply: same `ActivateBundleResponse` but `outcome ∈ {"active", "rollback_unstartable", "rollback_target_missing"}`. (`rolled_back` is meaningless here — manual rollback IS the rollback.)

### 3.5 zbus integration

`zbus` represents passed fds as `zbus::zvariant::OwnedFd`. The interface
method takes `OwnedFd` directly; zbus closes the user-side reference after
delivery, leaving boxpilotd as the sole owner. The handler `mmap`s read-only
and drops the fd after unpack.

## 4. Daemon-side module layout

New module tree under `crates/boxpilotd/src/profile/`:

```text
profile/
  mod.rs           — re-exports
  activate.rs      — orchestrator (the §10 state machine)
  rollback.rs      — manual rollback verb impl
  unpack.rs        — tar entry walker + §9.2 enforcement
  release.rs       — releases/<id> path management, active symlink swap
  gc.rs            — retention policy enforcement
  recovery.rs      — startup .staging/ cleanup + active symlink validation
  toml_commit.rs   — extends StateCommit with active/previous fields
```

`profile/mod.rs` is added to `boxpilotd/src/main.rs`'s `mod` list. Tests live
alongside each module.

`paths::Paths` gets:

- `releases_dir() -> /etc/boxpilot/releases`
- `staging_dir() -> /etc/boxpilot/.staging`
- `active_symlink() -> /etc/boxpilot/active`
- `release_dir(activation_id) -> /etc/boxpilot/releases/<id>`
- `staging_subdir(activation_id) -> /etc/boxpilot/.staging/<id>`

All `#[allow(dead_code)]` annotations on `service::verify::*` are removed in
the first commit that wires verify into `activate.rs`.

## 5. Activation state machine (§10 mapping)

Mapping spec §10 steps 5–15 to module calls:

| §10 step | Daemon action |
|----------|--------------|
| 5  | `lock::acquire_global` (existing from plan #2) |
| 6  | `unpack::unpack_into(memfd, staging_subdir(id))` |
| 7  | `release::run_singbox_check(core_path, staging_subdir(id))` |
| 8  | `rename(staging_subdir, release_dir)` |
| 9  | `release::swap_active_symlink(release_dir)` (creates `active.new`, renames over `active`) |
| 10 | `service::control::run(Restart, target_service, systemd)` (existing) |
| 11–12 | `verify::wait_for_running(target_service, n_restarts_pre, window, systemd)` |
| 13 | `toml_commit::commit_active(active_*, previous_*)` (extends StateCommit) |
| 14 | rollback path: `release::swap_active_symlink(previous_release_dir)` + restart |
| 15 | second `verify::wait_for_running` over rolled-back release; map outcomes to terminal states |

Pre-step 5: `n_restarts_pre = systemd.unit_state(target_service).n_restarts`
captured BEFORE service restart so the verify can detect crash-relaunch loops.

The orchestrator returns one of four outcomes:

- `Active` — step 11 verify passed, step 13 toml committed.
- `RolledBack` — step 11 failed, step 14 rollback succeeded, step 15 verify
  passed; toml NOT updated (still points at previous).
- `RollbackUnstartable` — step 14 succeeded but step 15 verify failed; toml
  NOT updated; service stopped to break Restart loop.
- `RollbackTargetMissing` — step 11 failed and there is no previous release on
  disk; toml NOT updated; service stopped; `active` left at the failed-but-
  checked release.

GC runs at the very end of the `Active` path, inside the same lock.

### 5.1 Boxpilot.toml writes

On `Active`:

- `previous_*` fields ← old `active_*` field values (if present).
- `active_*` fields ← new release's manifest values.
- `activated_at` ← RFC3339 of step 13.

On any rolled-back outcome: no toml write. The pre-state is preserved.

`StateCommit::apply` is reused but extended with optional active/previous
field assignments. The polkit drop-in path is left untouched here — it
already exists from plan #3.

## 6. Unpacker (§9.2 enforcement)

`unpack::unpack_into(fd: OwnedFd, dest: &Path) -> Result<UnpackReport, UnpackError>`

Algorithm:

1. mmap fd read-only. If `expected_total_bytes` was given and the file size
   exceeds `BUNDLE_MAX_TOTAL_BYTES`, abort before mmap.
2. Create `dest` with mode 0o700 (root-owned, no group/world access).
3. Iterate tar entries via `tar::Archive::entries_with_seek`. For each entry
   header, BEFORE reading the body:
   - Reject `EntryType` not in `{Regular, Directory}`. Symlinks, hardlinks,
     fifos, char/block devices, sockets, GNU sparse — all refused.
   - Reject `entry.size() > BUNDLE_MAX_FILE_BYTES`.
   - Reject `path` if any component contains:
     - `..` (after Unicode normalization to NFC; the daemon rejects on raw
       byte equality with `b".."` AND on NFC-normalized equality, since
       Linux filesystems compare bytes but a hostile bundle could ship a
       compatible-looking variant the GUI's preview would render).
     - NUL byte (Linux treats NUL as path terminator anyway, but tar entries
       with embedded NULs are pathological).
     - `\0`, `\\`, U+2215 DIVISION SLASH, U+FF0F FULLWIDTH SOLIDUS, or any
       ASCII control character `< 0x20`.
     - Absolute prefix (`/` as first component).
4. Resolve the destination path: `dest.join(path)`. After joining, canonicalize
   the path via a safe walker (DON'T `std::fs::canonicalize` — the dest doesn't
   exist yet for non-leaf entries). The walker manually resolves each
   component against `dest` and rejects any prefix that escapes `dest`.
5. Enforce running totals as bytes are streamed:
   - `total_bytes`, `file_count`, `nesting_depth`.
   - Each exceeds rejects with the matching `BundleError`.
6. Write file with `OpenOptions::create(true).write(true).mode(0o600)` (root-
   owned via daemon EUID).
7. Directory entries created with mode 0o700.
8. After the walk: verify `manifest.json` exists, parses as
   `ActivationManifest`, and its `assets[*].sha256` match what we just wrote
   on disk. Mismatch → reject.
9. `verify_asset_refs` (already in `boxpilot-profile`) runs against
   `config.json` to ensure every reference resolves. Refused if not.

`UnpackReport` returns the parsed manifest plus `bytes_written`.

### 6.1 Symlink-following defense

Tar bombs can interleave a symlink entry pointing outside the dest, then a
file entry whose path would write through the symlink. We refuse symlink
entries outright (step 3). We also refuse hardlinks for the same reason. The
"after symlink expansion at every step of the walk" requirement from spec
§9.2 is satisfied trivially: there are no symlinks to follow because we
never create one.

## 7. Verification wire-up

`activate::run` calls `verify::wait_for_running` twice:

- Step 11: `wait_for_running(target_service, n_restarts_pre, window, systemd)`.
  - `Running` → success path.
  - `Stuck { final_state }` → rollback path.
  - `NotFound` → return `HelperError::Systemd { ... }` — the unit doesn't
    exist, which means plan #3 install_managed never ran. Surface honestly.
- Step 15 (rollback path only): same call, but `n_restarts_pre` is whatever
  systemd reported at the START of step 14 (we captured it just before the
  rollback restart, NOT at top of activation).

`window` plumbed from request, clamped via `verify::MAX_WINDOW`. Default 5 s
when request omits the field.

## 8. Crash recovery

On `boxpilotd` startup, before D-Bus interface registration, run
`recovery::reconcile(&paths) -> RecoveryReport`:

1. `walk(staging_dir)` — for each subdir, `remove_dir_all`. Log each removal
   at info. A staging dir is only valid mid-call; its existence at startup
   means a prior crash. Any errors during cleanup are logged and tolerated
   (best-effort; real failures surface on next activation).
2. `validate_active(active_symlink, releases_dir)`:
   - `active` doesn't exist → ok (fresh install).
   - `active` resolves under `releases_dir/<id>/` AND the target dir exists
     → ok.
   - `active` is dangling, points outside `releases_dir`, or points at a
     non-existent target → set `active_corrupt = true` in report.
3. The `RecoveryReport` is exposed via a private getter on `HelperContext`;
   when `active_corrupt` is true, all activation/rollback verbs fail with a
   new `HelperError::ActiveCorrupt` rather than attempting writes. The GUI
   surfaces a repair prompt (plan #7); this plan only adds the error
   plumbing.

If startup recovery itself fails (filesystem unreadable, etc.), the daemon
logs and starts anyway; subsequent verbs will fail naturally on the same
filesystem. We don't refuse startup over a transient error.

## 9. Garbage collection

`gc::run(paths) -> GcReport` runs at the end of `Active` path under the
same global lock.

Algorithm:

1. List `releases/<id>/` directories; resolve `active` → keep_a.
2. Read `active_release_id` and `previous_release_id` from `boxpilot.toml`
   AFTER the toml commit. (We need the new toml because `active` was just
   updated.) keep_b = `previous_release_id`.
3. Sort remaining directories oldest-first by `mtime`.
4. Compute total `releases/` size. While
   `count(remaining) + 2 > 10` OR `total_size > 2 GiB`, delete oldest.
5. Skip `keep_a` and `keep_b` always.

GC errors are logged but do not fail the activation — the user sees
"activated successfully; GC encountered N errors" rather than activation
fail. GC is best-effort housekeeping.

## 10. Manual rollback verb

`profile.rollback_release` flow:

1. Acquire global lock.
2. Read `boxpilot.toml`. Resolve `target_activation_id`:
   - Must exist under `releases_dir`.
   - Must NOT equal current `active_release_id` (that would be a no-op; we
     reject with `HelperError::Ipc { message: "already active" }` for
     clarity).
3. Capture `n_restarts_pre`.
4. `release::swap_active_symlink(release_dir(target))`.
5. Restart service.
6. Verify (one window).
7. On verify success: toml commit (target becomes active, current active
   becomes previous).
8. On verify failure: re-swap symlink back to original, restart, second
   verify. If second verify also fails → `RollbackUnstartable`. (Symmetric
   to auto-rollback.)
9. `target == previous_release_id` → after swap, the new previous is the
   former active. Always: `previous = whatever active was before this op`.

This verb does not run GC. The user might be cycling through history; we
don't want a GC-during-rollback to delete the next-target.

## 11. User-side bundle handoff

`boxpilot-profile::bundle::prepare_bundle` becomes:

```rust
pub struct PreparedBundle {
    pub staging: tempfile::TempDir,   // existing — kept for tests/debug
    pub manifest: ActivationManifest, // existing
    pub memfd: OwnedFd,               // NEW — sealed tar
    pub tar_size: u64,                // NEW
}
```

After populating `staging` (existing logic), the function tars the directory
into a memfd and seals it. The TempDir is dropped on `PreparedBundle`'s
drop, but `memfd` stays alive because it's an independent fd. The Tauri
command in `boxpilot-tauri::profile_activate` consumes the `memfd`, sends it
over D-Bus, and lets it close after the call.

`prepare_bundle` continues to be unit-testable without a daemon: tests
inspect `staging/` for layout assertions and `memfd` only via fd-roundtrip
through the tar reader.

A new `boxpilot-tauri` command `profile_activate(profile_id, options)` calls
`prepare_bundle` then issues `profile.activate_bundle` over D-Bus with the
fd. This is the one user-side wiring change in this plan.

## 12. Error model

New `HelperError` variants:

- `BundleTooLarge { total: u64, limit: u64 }`
- `BundleEntryRejected { reason: String }` — covers absolute paths, symlinks,
  hardlinks, devices, traversal, unicode aliasing.
- `BundleAssetMismatch { path: String }` — manifest sha256 vs on-disk sha256.
- `SingboxCheckFailed { exit: i32, stderr_tail: String }` — stderr capped at
  256 bytes, redacted via §14 hooks (plan #8 will tighten; for now we strip
  any line containing `password`, `uuid`, `private_key`).
- `ActivationVerifyStuck { final_state: UnitState }`.
- `RollbackTargetMissing` — explicit terminal.
- `RollbackUnstartable { final_state: UnitState }` — explicit terminal.
- `ActiveCorrupt` — startup recovery flagged.
- `ReleaseAlreadyActive` — manual rollback to current.
- `ReleaseNotFound { activation_id: String }` — manual rollback target gone.

Each gets a dedicated D-Bus error name in `to_zbus_err` so the GUI can
discriminate without parsing strings.

## 13. Test strategy

The verb has many independent failure surfaces. Tests live in three layers:

### 13.1 Unit tests per module

- `unpack.rs`: ~25 tests covering each §9.2 rejection case (one tar fixture
  per rule), plus happy-path layouts. Fixtures are constructed in-memory via
  the `tar` crate writer; no real fd needed for unit tests.
- `release.rs`: rename semantics around `active` symlink (atomic swap, no
  unlink-then-symlink window). Uses `tempfile` + real symlinks.
- `gc.rs`: synthesize 15 release dirs with controlled mtime/size, assert
  retention policy. Mocks `fs_meta` for deterministic sizes.
- `recovery.rs`: synthesize crashed `.staging/` and corrupt `active`
  scenarios.
- `toml_commit.rs`: round-trip active/previous fields through `StateCommit`.
- `activate.rs`: full state machine. Mock systemd + mock sing-box check
  (`SingboxChecker` trait, `FakeSingboxChecker` configurable to pass/fail).
  ~15 tests covering each `Outcome` variant, including
  `RollbackTargetMissing` and `RollbackUnstartable`.

### 13.2 Integration tests in `boxpilotd`

`tests/activate_pipeline.rs` (new file):

- Boots the daemon with `Paths::with_root(tempdir)`, recording systemd, fake
  checker, and exercises the public verb through a synthetic memfd.
- 4 scenarios: happy activation, manifest sha mismatch, sing-box check fail,
  verify timeout-then-rollback succeeds.

### 13.3 Boxpilot-profile bundle test

Existing `prepare_bundle` tests grow a `produces_sealed_memfd_with_tar`
case that opens the memfd, runs the tar reader, and asserts the same files
the staging dir contains. The seal is asserted via
`fcntl(F_GET_SEALS)` showing all four bits set.

Total target: ≥60 new tests across crates. `make test` (which gates the
plan) must pass with `cargo test --workspace --no-fail-fast` in under 60 s
on the CI runner profile.

## 14. boxpilot.toml schema notes

We do NOT bump `schema_version` in this plan. Rationale: every new field in
`active_*` and `previous_*` is `Option<...> #[serde(default)]`. A v1.0 toml
without `previous_*` parses fine; once activation runs the fields populate.
The schema_version becomes 2 only if we ever rename or remove an existing
field, or change a value's semantic interpretation. Plan #5 does neither.

The `BoxpilotConfig` struct gains:

```rust
#[serde(default)] pub previous_release_id: Option<String>,
#[serde(default)] pub previous_profile_id: Option<String>,
#[serde(default)] pub previous_profile_sha256: Option<String>,
#[serde(default)] pub previous_activated_at: Option<String>,
```

## 15. Acceptance criteria

Plan #5 ships when:

1. `cargo test --workspace` passes; +60 tests minimum.
2. `cargo clippy --workspace --all-targets -- -D warnings` clean.
3. `cargo fmt --check` clean.
4. `frontend && pnpm build` clean (no GUI surface added beyond the Tauri
   command stub; existing build must not regress).
5. Smoke procedure document `docs/superpowers/plans/2026-04-30-activation-pipeline-smoke-procedure.md` exists, listing manual steps to:
   - Build the daemon.
   - Install + adopt a core (existing plan #2/#3 procedure).
   - Import a profile (plan #4 procedure).
   - Call `profile.activate_bundle` via `gdbus` with a hand-crafted memfd.
   - Verify `active` symlink, `boxpilot.toml`, `boxpilot-sing-box.service`
     state, and a successful auto-rollback case.
6. `service::verify::wait_for_running` no longer carries `#[allow(dead_code)]`.
7. Both stub bodies in `iface.rs` are replaced with real implementations
   that go through `dispatch::authorize` (plan #1 chokepoint).
8. Unit tests cover all four `Outcome` variants for `profile.activate_bundle`
   and all three for `profile.rollback_release`.
9. Spec §9.2 acceptance — every rejection rule has at least one negative
   test in `unpack.rs`.

## 16. Plan decomposition (input to writing-plans)

This spec maps to one plan with the following commit-sized steps:

1. Plumbing types: `paths::Paths` extensions, `HelperError` variants,
   `BoxpilotConfig` previous_* fields, `ActivateBundleRequest/Response`
   IPC types. Tests for serde round-trip.
2. `unpack.rs` with full §9.2 enforcement + ~25 unit tests.
3. `release.rs` (atomic symlink swap + rename helpers) + tests.
4. `recovery.rs` startup hook + integration into `main.rs` + tests.
5. `verify` rewire — drop `#[allow(dead_code)]` and add a
   `ServiceVerifier` trait so `activate.rs` is mockable.
6. `gc.rs` + tests.
7. `toml_commit.rs` extending StateCommit + tests.
8. `activate.rs` orchestrator + 15 tests across all `Outcome` variants.
   Replace `iface.rs::profile_activate_bundle` stub.
9. `rollback.rs` + tests. Replace `iface.rs::profile_rollback_release`
   stub.
10. User-side `prepare_bundle` memfd extension + Tauri
    `profile_activate` command. Existing
    `boxpilot-profile::bundle` tests grow the memfd assertions.
11. Smoke procedure doc.

Each step compiles, tests pass, and is independently mergeable in
principle. Step 8 is the largest (~800 LOC); steps 2 and 6 are next
(~400 LOC each).
