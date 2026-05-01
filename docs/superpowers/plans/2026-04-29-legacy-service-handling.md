# Existing `sing-box.service` Handling — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement BoxPilot's spec §8 "existing `sing-box.service` handling": observation mode (read-only inspection) and migration mode (copy config to user profile store, then atomically stop+disable the legacy unit so the standard activation pipeline can take over). Plan #6 of 9 in the BoxPilot Linux v1.0 series.

**Architecture:** Add a `legacy/` module under `boxpilotd` with three concerns — observe (read-only D-Bus + on-disk fragment inspection), migrate-prepare (read config bytes + sibling assets, return them to the user-side for import — no system mutation), migrate-cutover (stop + disable the legacy unit, back up its fragment under `/var/lib/boxpilot/backups/units/`). Add IPC types under `boxpilot-ipc::legacy`, fill the two helper-method stubs in `iface.rs`, expose Tauri commands + a thin TS wrapper. The two stages of migration share one polkit ID (`legacy.migrate_service`, auth_admin / re-prompt) but are dispatched on a `step` discriminator; the stop+disable cutover is the only mutation. The "enable + start `boxpilot-sing-box.service`" half of spec §8 step 4 is **not** done by this plan — that's the standard activation pipeline (plan #5) consuming the imported profile after cutover.

**Tech Stack:** Rust 2021, tokio, zbus (D-Bus), tracing, thiserror, serde / serde_json / base64, tempfile + pretty_assertions in tests, Vue 3 / TypeScript on the frontend.

---

## Reference checklist

These spec/code references are load-bearing for the tasks below. Re-read them when in doubt.

- spec §6.3 action whitelist — `legacy.observe_service` (ReadOnly), `legacy.migrate_service` (HighRisk).
- spec §6.5 trust checks — apply only when a path will end up in a generated unit's `ExecStart`. Migration cutover doesn't generate a new unit, so trust checks do **not** apply to the legacy unit's binary path; they only run later in `service.install_managed`.
- spec §8 — the three modes. v1.0 implements modes 1 & 2 only; "advanced in-place takeover" (mode 3) is explicitly out of scope for this plan.
- spec §5.4 — `/var/lib/boxpilot/backups/units/` for the fragment backup. Root-owned, not world-readable.
- spec §16.14, §16.15 — acceptance criteria for observation + migration.
- `crates/boxpilot-ipc/src/method.rs` — `LegacyObserveService` is `ReadOnly`, `LegacyMigrateService` is `HighRisk`.
- `crates/boxpilotd/src/iface.rs:309-322` — current stubs for the two methods (return `NotImplemented` after `dispatch::authorize`).
- `packaging/linux/polkit-1/actions/app.boxpilot.helper.policy:33-37,112-116` — polkit actions are already declared with the right tiers; do NOT add new polkit IDs.
- `crates/boxpilotd/src/systemd.rs` — `Systemd` trait + `DBusSystemd` + `RecordingSystemd` test mock. Add `fragment_path` here.
- `crates/boxpilot-profile/src/import.rs:69` (`import_local_file`) and `:138` (`import_local_dir`) — user-side entry points the Vue layer will use after the helper returns config bytes from migrate-prepare.

The legacy unit name we probe is fixed at **`sing-box.service`**. `sing-box@*.service` and other variants are out of scope (mentioned nowhere in the spec).

---

## File Structure

### New files

- `crates/boxpilot-ipc/src/legacy.rs` — request/response types (`LegacyObserveServiceResponse`, `LegacyMigrateRequest`/`Response`, `ConfigPathKind`, `MigratedAsset`).
- `crates/boxpilotd/src/legacy/mod.rs` — module barrel.
- `crates/boxpilotd/src/legacy/path_safety.rs` — `classify_config_path` (system / user-or-ephemeral / unknown).
- `crates/boxpilotd/src/legacy/unit_parser.rs` — minimal systemd unit parser to extract `ExecStart=` and `-c <path>` from the fragment.
- `crates/boxpilotd/src/legacy/observe.rs` — orchestrates fragment_path + ExecStart parse + unit_state into `LegacyObserveServiceResponse`.
- `crates/boxpilotd/src/legacy/backup.rs` — `backup_unit_file` (root-owned 0600 copy under `/var/lib/boxpilot/backups/units/`).
- `crates/boxpilotd/src/legacy/migrate.rs` — `prepare` (read config + sibling assets) and `cutover` (stop + disable + backup).
- `docs/superpowers/plans/2026-04-29-legacy-service-handling-smoke-procedure.md` — desktop smoke procedure, twin to prior plans' smoke docs.

### Modified files

- `crates/boxpilot-ipc/src/lib.rs` — `pub mod legacy; pub use legacy::*;`.
- `crates/boxpilot-ipc/src/error.rs` — add `LegacyConfigPathUnsafe { path }`, `LegacyUnitNotFound { unit }`, `LegacyExecStartUnparseable { reason }`, `LegacyStopFailed { unit, message }`, `LegacyDisableFailed { unit, message }`, `LegacyConflictsWithManaged { unit }`, `LegacyAssetTooLarge { path, size, limit }`, `LegacyTooManyAssets { count, limit }`.
- `crates/boxpilotd/src/main.rs` — `mod legacy;`.
- `crates/boxpilotd/src/paths.rs` — add `backups_units_dir()` returning `<root>/var/lib/boxpilot/backups/units`.
- `crates/boxpilotd/src/systemd.rs` — add `Systemd::fragment_path` + `Systemd::unit_file_state` + `SystemdManagerProxy::get_unit_file_state`; wire through `RecordingSystemd` and `FixedSystemd`.
- `crates/boxpilotd/src/iface.rs` — replace the two `do_stub` calls with real bodies that call into `legacy::observe::observe` and `legacy::migrate::run`.
- `crates/boxpilot-tauri/src/helper_client.rs` — `LegacyObserveService` + `LegacyMigrateService` proxy methods + typed wrappers.
- `crates/boxpilot-tauri/src/commands.rs` — `helper_legacy_observe_service`, `helper_legacy_migrate_prepare`, `helper_legacy_migrate_cutover` Tauri commands.
- `crates/boxpilot-tauri/src/lib.rs` — register the new commands in `generate_handler!`.
- `frontend/src/api/types.ts` — TS shapes mirroring the new IPC types.
- `frontend/src/api/helper.ts` — `legacyObserveService`, `legacyMigratePrepare`, `legacyMigrateCutover` invokers.

No frontend Vue page is added in this plan — the GUI surfacing of legacy detection lives in plan #7 (GUI Home/Profiles/Settings).

---

## Wire Contract (decided here, locked for downstream tasks)

```rust
// crates/boxpilot-ipc/src/legacy.rs

pub const LEGACY_UNIT_NAME: &str = "sing-box.service";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConfigPathKind {
    /// Path is under /etc, /usr, /var/lib, /var/cache, /opt, or /srv — safe
    /// to keep referencing as a system service config.
    SystemPath,
    /// Path is under /home, /tmp, /run/user, /var/tmp — refuse migration
    /// (spec §8 / §9.3).
    UserOrEphemeral,
    /// ExecStart did not contain a parseable -c/--config flag, or no path
    /// was extracted. The GUI must prompt the user to pick a profile manually.
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LegacyObserveServiceResponse {
    /// `false` when the unit is not loaded by systemd at all.
    pub detected: bool,
    /// Always `LEGACY_UNIT_NAME` when detected; carried in the response so
    /// future expansions can probe other names without changing the shape.
    pub unit_name: Option<String>,
    /// `org.freedesktop.systemd1.Unit::FragmentPath`. None when the unit is
    /// transient or its fragment was deleted.
    pub fragment_path: Option<String>,
    /// systemctl's `enabled / disabled / static / masked / not-found` view.
    /// None when the manager refused to report it.
    pub unit_file_state: Option<String>,
    /// Raw `ExecStart=` line (first one, after expansion) as read from
    /// `fragment_path`. None when the fragment has no ExecStart.
    pub exec_start_raw: Option<String>,
    /// Path extracted from `-c` / `--config` in `exec_start_raw`.
    pub config_path: Option<String>,
    pub config_path_kind: ConfigPathKind,
    pub unit_state: crate::UnitState,
    /// `true` when `unit_name == cfg.target_service`, i.e. the legacy
    /// "sing-box.service" name happens to coincide with what BoxPilot
    /// already manages. Only relevant if a future deployment changes
    /// `target_service`; today this is always `false`.
    pub conflicts_with_managed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "step", rename_all = "snake_case")]
pub enum LegacyMigrateRequest {
    /// Read fragment + config + sibling assets from disk and return them.
    /// No system mutation. Refused if `config_path` is UserOrEphemeral.
    Prepare,
    /// Stop + disable the legacy unit, back up its fragment. The next
    /// `profile.activate_bundle` will then enable + start
    /// `boxpilot-sing-box.service` as part of the standard pipeline.
    Cutover,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MigratedAsset {
    /// Filename as it appeared next to the legacy config (no nested dirs).
    pub filename: String,
    /// Bytes; serde encodes as a JSON array of u8 — matches the existing
    /// per-file BUNDLE_MAX_FILE_BYTES cap (16 MiB).
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LegacyMigratePrepareResponse {
    pub unit_name: String,
    pub config_path_was: String,
    pub config_filename: String,
    /// The bytes of the legacy config file. The user-side Vue layer hands
    /// these to `boxpilot_profile::import_local_file` (or the dir variant
    /// when `assets` is non-empty).
    pub config_bytes: Vec<u8>,
    pub assets: Vec<MigratedAsset>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LegacyMigrateCutoverResponse {
    pub unit_name: String,
    pub backup_unit_path: String,
    /// Post-cutover state of the legacy unit. Should normally be Inactive
    /// or NotFound (after disable, GetUnit may report NoSuchUnit).
    pub final_unit_state: crate::UnitState,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "step", rename_all = "snake_case")]
pub enum LegacyMigrateResponse {
    Prepare(LegacyMigratePrepareResponse),
    Cutover(LegacyMigrateCutoverResponse),
}
```

Asset enumeration in `Prepare`: walk **only the direct parent directory** of `config_path`, taking every regular file (no recursion, no symlinks, no nested dirs) other than `config_path` itself, capped at `BUNDLE_MAX_FILE_COUNT - 1` siblings and `BUNDLE_MAX_FILE_BYTES` per file. We deliberately do not parse the legacy config to learn which assets it references — sing-box configs reference assets by relative path or absolute path, parsing them correctly is plan #5/#7's territory; for migration we hand the user-side a directory snapshot and let `boxpilot_profile::import_local_dir` (which already does asset enumeration) absorb it. If the directory has no siblings, `assets` is empty and the user-side calls `import_local_file` instead.

---

## Task 1: IPC types — `boxpilot_ipc::legacy`

**Files:**
- Create: `crates/boxpilot-ipc/src/legacy.rs`
- Modify: `crates/boxpilot-ipc/src/lib.rs`

- [ ] **Step 1: Write the failing tests**

Append to `crates/boxpilot-ipc/src/legacy.rs`:

```rust
use crate::UnitState;
use serde::{Deserialize, Serialize};

pub const LEGACY_UNIT_NAME: &str = "sing-box.service";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConfigPathKind {
    SystemPath,
    UserOrEphemeral,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LegacyObserveServiceResponse {
    pub detected: bool,
    #[serde(default)]
    pub unit_name: Option<String>,
    #[serde(default)]
    pub fragment_path: Option<String>,
    #[serde(default)]
    pub unit_file_state: Option<String>,
    #[serde(default)]
    pub exec_start_raw: Option<String>,
    #[serde(default)]
    pub config_path: Option<String>,
    pub config_path_kind: ConfigPathKind,
    pub unit_state: UnitState,
    pub conflicts_with_managed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "step", rename_all = "snake_case")]
pub enum LegacyMigrateRequest {
    Prepare,
    Cutover,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MigratedAsset {
    pub filename: String,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LegacyMigratePrepareResponse {
    pub unit_name: String,
    pub config_path_was: String,
    pub config_filename: String,
    pub config_bytes: Vec<u8>,
    pub assets: Vec<MigratedAsset>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LegacyMigrateCutoverResponse {
    pub unit_name: String,
    pub backup_unit_path: String,
    pub final_unit_state: UnitState,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "step", rename_all = "snake_case")]
pub enum LegacyMigrateResponse {
    Prepare(LegacyMigratePrepareResponse),
    Cutover(LegacyMigrateCutoverResponse),
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn legacy_unit_name_is_fixed() {
        assert_eq!(LEGACY_UNIT_NAME, "sing-box.service");
    }

    #[test]
    fn config_path_kind_uses_snake_case_on_wire() {
        assert_eq!(
            serde_json::to_string(&ConfigPathKind::UserOrEphemeral).unwrap(),
            "\"user_or_ephemeral\""
        );
        assert_eq!(
            serde_json::to_string(&ConfigPathKind::SystemPath).unwrap(),
            "\"system_path\""
        );
    }

    #[test]
    fn observe_response_round_trips() {
        let r = LegacyObserveServiceResponse {
            detected: true,
            unit_name: Some("sing-box.service".into()),
            fragment_path: Some("/etc/systemd/system/sing-box.service".into()),
            unit_file_state: Some("enabled".into()),
            exec_start_raw: Some("/usr/bin/sing-box run -c /etc/sing-box/config.json".into()),
            config_path: Some("/etc/sing-box/config.json".into()),
            config_path_kind: ConfigPathKind::SystemPath,
            unit_state: UnitState::NotFound,
            conflicts_with_managed: false,
        };
        let s = serde_json::to_string(&r).unwrap();
        let back: LegacyObserveServiceResponse = serde_json::from_str(&s).unwrap();
        assert_eq!(back, r);
    }

    #[test]
    fn migrate_request_uses_step_tag() {
        let s = serde_json::to_string(&LegacyMigrateRequest::Prepare).unwrap();
        assert_eq!(s, "{\"step\":\"prepare\"}");
        let s = serde_json::to_string(&LegacyMigrateRequest::Cutover).unwrap();
        assert_eq!(s, "{\"step\":\"cutover\"}");
    }

    #[test]
    fn migrate_response_round_trips_both_arms() {
        let prep = LegacyMigrateResponse::Prepare(LegacyMigratePrepareResponse {
            unit_name: "sing-box.service".into(),
            config_path_was: "/etc/sing-box/config.json".into(),
            config_filename: "config.json".into(),
            config_bytes: vec![1, 2, 3],
            assets: vec![MigratedAsset {
                filename: "geosite.db".into(),
                bytes: vec![4, 5, 6],
            }],
        });
        let s = serde_json::to_string(&prep).unwrap();
        assert!(s.contains("\"step\":\"prepare\""));
        let back: LegacyMigrateResponse = serde_json::from_str(&s).unwrap();
        assert_eq!(back, prep);

        let cut = LegacyMigrateResponse::Cutover(LegacyMigrateCutoverResponse {
            unit_name: "sing-box.service".into(),
            backup_unit_path: "/var/lib/boxpilot/backups/units/sing-box.service-2026-04-29T00-00-00Z"
                .into(),
            final_unit_state: UnitState::NotFound,
        });
        let s = serde_json::to_string(&cut).unwrap();
        assert!(s.contains("\"step\":\"cutover\""));
        let back: LegacyMigrateResponse = serde_json::from_str(&s).unwrap();
        assert_eq!(back, cut);
    }
}
```

- [ ] **Step 2: Add module export in `crates/boxpilot-ipc/src/lib.rs`**

Right after the existing `pub mod profile;` block, add:

```rust
pub mod legacy;
pub use legacy::{
    ConfigPathKind, LegacyMigrateCutoverResponse, LegacyMigratePrepareResponse,
    LegacyMigrateRequest, LegacyMigrateResponse, LegacyObserveServiceResponse, MigratedAsset,
    LEGACY_UNIT_NAME,
};
```

- [ ] **Step 3: Run the tests**

Run: `cargo test -p boxpilot-ipc legacy`
Expected: 5 passes (the four explicit tests plus the existing crate-level smoke).

- [ ] **Step 4: Commit**

```bash
git add crates/boxpilot-ipc/src/legacy.rs crates/boxpilot-ipc/src/lib.rs
git commit -m "feat(ipc): legacy service IPC types"
```

---

## Task 2: New `HelperError` variants for legacy flows

**Files:**
- Modify: `crates/boxpilot-ipc/src/error.rs`
- Modify: `crates/boxpilotd/src/iface.rs:30-51` (the `to_zbus_err` match)

- [ ] **Step 1: Write the failing tests**

Append to the `mod tests` block in `crates/boxpilot-ipc/src/error.rs`:

```rust
    #[test]
    fn legacy_variants_round_trip() {
        use HelperError::*;
        for v in [
            LegacyConfigPathUnsafe {
                path: "/home/alice/sb.json".into(),
            },
            LegacyUnitNotFound {
                unit: "sing-box.service".into(),
            },
            LegacyExecStartUnparseable {
                reason: "no -c flag".into(),
            },
            LegacyStopFailed {
                unit: "sing-box.service".into(),
                message: "still running after 2s".into(),
            },
            LegacyDisableFailed {
                unit: "sing-box.service".into(),
                message: "DisableUnitFiles refused".into(),
            },
            LegacyConflictsWithManaged {
                unit: "sing-box.service".into(),
            },
            LegacyAssetTooLarge {
                path: "/etc/sing-box/huge.db".into(),
                size: 99,
                limit: 50,
            },
            LegacyTooManyAssets { count: 99, limit: 50 },
        ] {
            let s = serde_json::to_string(&v).unwrap();
            let back: HelperError = serde_json::from_str(&s).unwrap();
            assert_eq!(back, v);
        }
    }
```

- [ ] **Step 2: Add the variants to `HelperError`**

In `crates/boxpilot-ipc/src/error.rs`, right before the closing brace of the enum:

```rust
    /// Spec §8 — legacy unit's config path is under /home, /tmp, /run/user, etc.
    #[error("legacy config path is not under a system location: {path}")]
    LegacyConfigPathUnsafe { path: String },

    /// `sing-box.service` is not loaded by systemd.
    #[error("legacy unit not found: {unit}")]
    LegacyUnitNotFound { unit: String },

    /// `ExecStart=` could not be parsed out of the legacy fragment, or no
    /// `-c <path>` / `--config <path>` argument was found.
    #[error("could not parse ExecStart: {reason}")]
    LegacyExecStartUnparseable { reason: String },

    /// `StopUnit` for the legacy service failed or the unit refused to settle.
    #[error("failed to stop {unit}: {message}")]
    LegacyStopFailed { unit: String, message: String },

    /// `DisableUnitFiles` refused after a successful stop.
    #[error("failed to disable {unit}: {message}")]
    LegacyDisableFailed { unit: String, message: String },

    /// Defensive — refuse to migrate a unit whose name happens to equal
    /// `boxpilot.toml::target_service`.
    #[error("legacy unit {unit} is the same as the managed target_service")]
    LegacyConflictsWithManaged { unit: String },

    /// One sibling file in the legacy config dir exceeds BUNDLE_MAX_FILE_BYTES.
    #[error("legacy asset {path} exceeds per-file limit ({size} > {limit})")]
    LegacyAssetTooLarge {
        path: String,
        size: u64,
        limit: u64,
    },

    /// Direct parent of the legacy config has more than BUNDLE_MAX_FILE_COUNT-1
    /// sibling files.
    #[error("legacy config directory has too many siblings ({count} > {limit})")]
    LegacyTooManyAssets { count: u32, limit: u32 },
```

- [ ] **Step 3: Wire the new variants in `to_zbus_err`**

In `crates/boxpilotd/src/iface.rs`, extend the match in `to_zbus_err` (around line 30) — append before the closing brace:

```rust
        HelperError::LegacyConfigPathUnsafe { .. } => "app.boxpilot.Helper1.LegacyConfigPathUnsafe",
        HelperError::LegacyUnitNotFound { .. } => "app.boxpilot.Helper1.LegacyUnitNotFound",
        HelperError::LegacyExecStartUnparseable { .. } => {
            "app.boxpilot.Helper1.LegacyExecStartUnparseable"
        }
        HelperError::LegacyStopFailed { .. } => "app.boxpilot.Helper1.LegacyStopFailed",
        HelperError::LegacyDisableFailed { .. } => "app.boxpilot.Helper1.LegacyDisableFailed",
        HelperError::LegacyConflictsWithManaged { .. } => {
            "app.boxpilot.Helper1.LegacyConflictsWithManaged"
        }
        HelperError::LegacyAssetTooLarge { .. } => "app.boxpilot.Helper1.LegacyAssetTooLarge",
        HelperError::LegacyTooManyAssets { .. } => "app.boxpilot.Helper1.LegacyTooManyAssets",
```

- [ ] **Step 4: Run the tests**

Run: `cargo test -p boxpilot-ipc legacy_variants_round_trip && cargo build -p boxpilotd`
Expected: tests pass, daemon compiles. (`to_zbus_err` is exhaustive so the match would refuse to compile if a variant was missed.)

- [ ] **Step 5: Commit**

```bash
git add crates/boxpilot-ipc/src/error.rs crates/boxpilotd/src/iface.rs
git commit -m "feat(ipc): legacy-flow HelperError variants"
```

---

## Task 3: Add `Paths::backups_units_dir` and `legacy_unit_name`-aware path helpers

**Files:**
- Modify: `crates/boxpilotd/src/paths.rs`

- [ ] **Step 1: Write the failing tests**

In `crates/boxpilotd/src/paths.rs`, append inside `mod tests`:

```rust
    #[test]
    fn backups_units_dir_under_var_lib_boxpilot() {
        let p = Paths::with_root("/tmp/fake");
        assert_eq!(
            p.backups_units_dir(),
            PathBuf::from("/tmp/fake/var/lib/boxpilot/backups/units")
        );
    }

    #[test]
    fn backup_unit_path_includes_timestamp_suffix() {
        let p = Paths::with_root("/tmp/fake");
        let out = p.backup_unit_path("sing-box.service", "2026-04-29T00-00-00Z");
        assert_eq!(
            out,
            PathBuf::from(
                "/tmp/fake/var/lib/boxpilot/backups/units/sing-box.service-2026-04-29T00-00-00Z"
            )
        );
    }
```

- [ ] **Step 2: Add the helpers**

Append two methods to `impl Paths` in `crates/boxpilotd/src/paths.rs`:

```rust
    /// `/var/lib/boxpilot/backups/units` — destination for legacy-unit
    /// fragment backups taken before migrate-cutover. Spec §5.4.
    pub fn backups_units_dir(&self) -> PathBuf {
        self.root.join("var/lib/boxpilot/backups/units")
    }

    /// `/var/lib/boxpilot/backups/units/<unit>-<timestamp>` — full path of
    /// a single backup. Caller supplies the timestamp string (RFC3339 with
    /// `:` replaced by `-` so the filename is shell-friendly).
    pub fn backup_unit_path(&self, unit_name: &str, timestamp: &str) -> PathBuf {
        self.backups_units_dir()
            .join(format!("{unit_name}-{timestamp}"))
    }
```

- [ ] **Step 3: Run the tests**

Run: `cargo test -p boxpilotd paths::tests::backups_units_dir_under_var_lib_boxpilot paths::tests::backup_unit_path_includes_timestamp_suffix`
Expected: 2 passes.

- [ ] **Step 4: Commit**

```bash
git add crates/boxpilotd/src/paths.rs
git commit -m "feat(boxpilotd): paths for /var/lib/boxpilot/backups/units"
```

---

## Task 4: Extend `Systemd` trait with `fragment_path` + `unit_file_state`

**Files:**
- Modify: `crates/boxpilotd/src/systemd.rs`

- [ ] **Step 1: Write the failing tests**

Append to `crates/boxpilotd/src/systemd.rs::testing` (inside the `pub mod testing` block, after the `recording_systemd_records_start_unit` test):

```rust
    #[tokio::test]
    async fn fixed_systemd_returns_canned_fragment_path() {
        let q = FixedSystemd::new_with_fragment(
            UnitState::NotFound,
            Some("/etc/systemd/system/sing-box.service".into()),
            Some("enabled".into()),
        );
        assert_eq!(
            q.fragment_path("sing-box.service").await.unwrap(),
            Some("/etc/systemd/system/sing-box.service".into())
        );
        assert_eq!(
            q.unit_file_state("sing-box.service").await.unwrap(),
            Some("enabled".into())
        );
    }

    #[tokio::test]
    async fn fixed_systemd_default_constructor_keeps_fragment_unset() {
        // Existing tests construct FixedSystemd { answer: ... } directly;
        // check that style still works and returns None for the new methods.
        let q = FixedSystemd {
            answer: UnitState::NotFound,
            fragment_path: None,
            unit_file_state: None,
        };
        assert!(q.fragment_path("u").await.unwrap().is_none());
        assert!(q.unit_file_state("u").await.unwrap().is_none());
    }
```

- [ ] **Step 2: Extend the trait**

In the `pub trait Systemd` definition, add:

```rust
    /// `org.freedesktop.systemd1.Unit::FragmentPath` — the on-disk unit file
    /// for `unit_name`. `None` for transient units or when the fragment has
    /// been deleted from disk.
    async fn fragment_path(&self, unit_name: &str) -> Result<Option<String>, HelperError>;

    /// `systemctl is-enabled` view: `enabled` / `disabled` / `static` /
    /// `masked` / `not-found`. Surfaced as a string because the systemd
    /// vocabulary is itself open-ended; consumers branch on the canonical
    /// values they care about.
    async fn unit_file_state(&self, unit_name: &str) -> Result<Option<String>, HelperError>;
```

- [ ] **Step 3: Add a `GetUnitFileState` proxy method**

In the `trait SystemdManager` zbus proxy block:

```rust
    fn get_unit_file_state(&self, name: &str) -> zbus::Result<String>;
```

- [ ] **Step 4: Implement on `DBusSystemd`**

Append to `impl Systemd for DBusSystemd`:

```rust
    async fn fragment_path(&self, unit_name: &str) -> Result<Option<String>, HelperError> {
        let mgr = SystemdManagerProxy::new(&self.conn)
            .await
            .map_err(systemd_err)?;
        let unit_path = match mgr.get_unit(unit_name).await {
            Ok(p) => p,
            Err(zbus::Error::MethodError(name, _, _))
                if name.as_str() == "org.freedesktop.systemd1.NoSuchUnit" =>
            {
                return Ok(None);
            }
            Err(e) => return Err(systemd_err(e)),
        };
        // FragmentPath lives on the Unit interface; we read it via a generic
        // Properties.Get so we don't have to add yet another typed property.
        let props = zbus::fdo::PropertiesProxy::builder(&self.conn)
            .destination("org.freedesktop.systemd1")
            .map_err(systemd_err)?
            .path(unit_path)
            .map_err(systemd_err)?
            .build()
            .await
            .map_err(systemd_err)?;
        let v = props
            .get("org.freedesktop.systemd1.Unit", "FragmentPath")
            .await
            .map_err(|e| HelperError::Systemd {
                message: format!("FragmentPath: {e}"),
            })?;
        let s: String = v.try_into().map_err(|e| HelperError::Systemd {
            message: format!("FragmentPath decode: {e}"),
        })?;
        if s.is_empty() {
            Ok(None)
        } else {
            Ok(Some(s))
        }
    }

    async fn unit_file_state(&self, unit_name: &str) -> Result<Option<String>, HelperError> {
        let mgr = SystemdManagerProxy::new(&self.conn)
            .await
            .map_err(systemd_err)?;
        match mgr.get_unit_file_state(unit_name).await {
            Ok(s) => Ok(Some(s)),
            Err(zbus::Error::MethodError(_, _, _)) => Ok(None),
            Err(e) => Err(systemd_err(e)),
        }
    }
```

- [ ] **Step 5: Update `FixedSystemd` and `RecordingSystemd`**

Replace the existing `FixedSystemd` definition in `pub mod testing`:

```rust
    pub struct FixedSystemd {
        pub answer: UnitState,
        pub fragment_path: Option<String>,
        pub unit_file_state: Option<String>,
    }

    impl FixedSystemd {
        pub fn new_with_fragment(
            answer: UnitState,
            fragment_path: Option<String>,
            unit_file_state: Option<String>,
        ) -> Self {
            Self {
                answer,
                fragment_path,
                unit_file_state,
            }
        }
    }
```

And extend `impl Systemd for FixedSystemd`:

```rust
        async fn fragment_path(&self, _: &str) -> Result<Option<String>, HelperError> {
            Ok(self.fragment_path.clone())
        }
        async fn unit_file_state(&self, _: &str) -> Result<Option<String>, HelperError> {
            Ok(self.unit_file_state.clone())
        }
```

Add fields + methods to `RecordingSystemd`:

```rust
    pub struct RecordingSystemd {
        pub answer: UnitState,
        pub fragment_path: Mutex<Option<String>>,
        pub unit_file_state: Mutex<Option<String>>,
        pub calls: Mutex<Vec<RecordedCall>>,
    }

    impl RecordingSystemd {
        pub fn new(answer: UnitState) -> Self {
            Self {
                answer,
                fragment_path: Mutex::new(None),
                unit_file_state: Mutex::new(None),
                calls: Mutex::new(Vec::new()),
            }
        }
        pub fn set_fragment_path(&self, path: Option<String>) {
            *self.fragment_path.lock().unwrap() = path;
        }
        pub fn set_unit_file_state(&self, state: Option<String>) {
            *self.unit_file_state.lock().unwrap() = state;
        }
        pub fn calls(&self) -> Vec<RecordedCall> {
            self.calls.lock().unwrap().clone()
        }
    }
```

And the corresponding trait impl methods:

```rust
        async fn fragment_path(&self, _: &str) -> Result<Option<String>, HelperError> {
            Ok(self.fragment_path.lock().unwrap().clone())
        }
        async fn unit_file_state(&self, _: &str) -> Result<Option<String>, HelperError> {
            Ok(self.unit_file_state.lock().unwrap().clone())
        }
```

The two existing direct `FixedSystemd { answer: ... }` literals (in `service::install::tests` and elsewhere) need the two new fields. Use `cargo build` to find them and add `fragment_path: None, unit_file_state: None,` to each.

- [ ] **Step 6: Run the tests**

Run: `cargo test -p boxpilotd systemd::testing && cargo build -p boxpilotd --tests`
Expected: passes.

- [ ] **Step 7: Commit**

```bash
git add crates/boxpilotd/src/systemd.rs crates/boxpilotd/src/service/install.rs
git commit -m "feat(boxpilotd): systemd fragment_path + unit_file_state"
```

(If `cargo build` flagged additional `FixedSystemd { ... }` literals in other files, include them in the commit.)

---

## Task 5: `legacy::path_safety::classify_config_path`

**Files:**
- Create: `crates/boxpilotd/src/legacy/mod.rs`
- Create: `crates/boxpilotd/src/legacy/path_safety.rs`
- Modify: `crates/boxpilotd/src/main.rs`

- [ ] **Step 1: Write the failing tests**

Create `crates/boxpilotd/src/legacy/path_safety.rs`:

```rust
//! Spec §8 / §9.3 — refuse to keep a system-service config reference under
//! `/home`, `/tmp`, `/run/user`, etc.

use boxpilot_ipc::ConfigPathKind;
use std::path::Path;

/// Classify an absolute path. Relative paths are `Unknown` (we don't trust
/// them as system service references in the first place).
pub fn classify_config_path(p: &Path) -> ConfigPathKind {
    if !p.is_absolute() {
        return ConfigPathKind::Unknown;
    }
    let s = p.to_string_lossy();
    const UNSAFE_PREFIXES: &[&str] = &[
        "/home/",
        "/tmp/",
        "/var/tmp/",
        "/run/user/",
        "/dev/",
        "/proc/",
    ];
    for pre in UNSAFE_PREFIXES {
        if s.starts_with(pre) {
            return ConfigPathKind::UserOrEphemeral;
        }
    }
    const SAFE_PREFIXES: &[&str] = &[
        "/etc/", "/usr/", "/var/lib/", "/var/cache/", "/opt/", "/srv/",
    ];
    for pre in SAFE_PREFIXES {
        if s.starts_with(pre) {
            return ConfigPathKind::SystemPath;
        }
    }
    ConfigPathKind::Unknown
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn etc_is_system_path() {
        assert_eq!(
            classify_config_path(Path::new("/etc/sing-box/config.json")),
            ConfigPathKind::SystemPath
        );
    }

    #[test]
    fn home_is_user_or_ephemeral() {
        assert_eq!(
            classify_config_path(Path::new("/home/alice/.config/sing-box/config.json")),
            ConfigPathKind::UserOrEphemeral
        );
    }

    #[test]
    fn tmp_run_user_var_tmp_are_user_or_ephemeral() {
        for p in [
            "/tmp/sb.json",
            "/var/tmp/sb.json",
            "/run/user/1000/sb.json",
            "/dev/shm/sb.json",
            "/proc/self/fd/3",
        ] {
            assert_eq!(
                classify_config_path(Path::new(p)),
                ConfigPathKind::UserOrEphemeral,
                "{p} should be UserOrEphemeral"
            );
        }
    }

    #[test]
    fn relative_is_unknown() {
        assert_eq!(
            classify_config_path(Path::new("config.json")),
            ConfigPathKind::Unknown
        );
    }

    #[test]
    fn other_absolute_is_unknown() {
        assert_eq!(
            classify_config_path(Path::new("/mnt/nfs/sb.json")),
            ConfigPathKind::Unknown
        );
    }

    #[test]
    fn home_prefix_is_not_substring_match() {
        // "/homework" must not be classified as UserOrEphemeral
        // — we only match path component prefixes via the trailing slash.
        assert_eq!(
            classify_config_path(Path::new("/homework/sb.json")),
            ConfigPathKind::Unknown
        );
    }
}
```

Create `crates/boxpilotd/src/legacy/mod.rs`:

```rust
//! Spec §8 — existing-`sing-box.service` handling. Observation (read-only)
//! and migration (prepare + cutover).

pub mod backup;
pub mod migrate;
pub mod observe;
pub mod path_safety;
pub mod unit_parser;
```

(Submodules `backup`, `migrate`, `observe`, `unit_parser` are added in later tasks; the build won't compile yet — Step 2 below stages a placeholder so the compile keeps moving.)

- [ ] **Step 2: Stage placeholders for the not-yet-written submodules**

Replace `mod.rs` content with just `pub mod path_safety;` for now so this task's code compiles independently. Later tasks will append `pub mod unit_parser;` etc. as they land.

So the actual content of `crates/boxpilotd/src/legacy/mod.rs` for this task is:

```rust
//! Spec §8 — existing-`sing-box.service` handling.

pub mod path_safety;
```

- [ ] **Step 3: Wire `mod legacy;` into `main.rs`**

In `crates/boxpilotd/src/main.rs`, alongside the other top-level `mod` declarations (insert in alphabetical order):

```rust
mod legacy;
```

- [ ] **Step 4: Run the tests**

Run: `cargo test -p boxpilotd legacy::path_safety`
Expected: 6 passes.

- [ ] **Step 5: Commit**

```bash
git add crates/boxpilotd/src/legacy/ crates/boxpilotd/src/main.rs
git commit -m "feat(boxpilotd/legacy): config path safety classifier"
```

---

## Task 6: `legacy::unit_parser::parse_exec_start`

**Files:**
- Create: `crates/boxpilotd/src/legacy/unit_parser.rs`
- Modify: `crates/boxpilotd/src/legacy/mod.rs`

- [ ] **Step 1: Write the failing tests**

Create `crates/boxpilotd/src/legacy/unit_parser.rs`:

```rust
//! Minimal systemd unit parser. We only need `[Service] ExecStart=` and the
//! `-c <path>` / `--config <path>` argument — the full systemd grammar (line
//! continuations, expansion, multiple ExecStart=, environment files, etc.)
//! is out of scope.

use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecStart {
    pub raw: String,
    pub config_path: Option<PathBuf>,
}

#[derive(Debug, thiserror::Error)]
pub enum ParseError {
    #[error("[Service] section not found")]
    NoServiceSection,
    #[error("ExecStart= not found in [Service]")]
    NoExecStart,
}

pub fn parse_exec_start(unit_text: &str) -> Result<ExecStart, ParseError> {
    let mut in_service = false;
    let mut exec_start_line: Option<String> = None;

    for raw_line in unit_text.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') {
            in_service = line.eq_ignore_ascii_case("[Service]");
            continue;
        }
        if !in_service {
            continue;
        }
        // Accept the optional `-`, `+`, `@`, `:`, `!` modifiers systemd
        // allows on ExecStart= (ignore-failure / elevated / etc.). We don't
        // care which one was used; we just want the command line.
        if let Some(after) = line.strip_prefix("ExecStart=") {
            let stripped = after.trim_start_matches(['-', '+', '@', ':', '!']);
            exec_start_line = Some(stripped.trim().to_string());
        }
    }
    if !in_service && exec_start_line.is_none() {
        return Err(ParseError::NoServiceSection);
    }
    let raw = exec_start_line.ok_or(ParseError::NoExecStart)?;
    let config_path = extract_config_arg(&raw);
    Ok(ExecStart { raw, config_path })
}

fn extract_config_arg(cmdline: &str) -> Option<PathBuf> {
    // Tokenize on whitespace. We don't try to honor shell-style quoting
    // because sing-box.service in the wild does not use it; if we see
    // single/double quotes around the path, strip them and move on.
    let mut iter = cmdline.split_whitespace().peekable();
    while let Some(tok) = iter.next() {
        let next = || iter.next().map(|s| s.to_string());
        let path = match tok {
            "-c" | "--config" | "-C" => next(),
            _ if tok.starts_with("--config=") => Some(tok["--config=".len()..].to_string()),
            _ if tok.starts_with("-c=") => Some(tok["-c=".len()..].to_string()),
            _ => None,
        };
        if let Some(p) = path {
            let trimmed = p.trim_matches(['"', '\'']).to_string();
            return Some(PathBuf::from(trimmed));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    fn unit(body: &str) -> String {
        format!("[Unit]\nDescription=x\n\n[Service]\n{body}\n\n[Install]\nWantedBy=multi-user.target\n")
    }

    #[test]
    fn parses_simple_exec_start_with_dash_c() {
        let u = unit("ExecStart=/usr/bin/sing-box run -c /etc/sing-box/config.json");
        let r = parse_exec_start(&u).unwrap();
        assert_eq!(r.raw, "/usr/bin/sing-box run -c /etc/sing-box/config.json");
        assert_eq!(
            r.config_path,
            Some(PathBuf::from("/etc/sing-box/config.json"))
        );
    }

    #[test]
    fn parses_long_form_config_flag() {
        let u = unit("ExecStart=/usr/bin/sing-box run --config /etc/sb/c.json");
        let r = parse_exec_start(&u).unwrap();
        assert_eq!(r.config_path, Some(PathBuf::from("/etc/sb/c.json")));
    }

    #[test]
    fn parses_equals_form() {
        let u = unit("ExecStart=/usr/bin/sing-box run --config=/etc/sb/c.json");
        let r = parse_exec_start(&u).unwrap();
        assert_eq!(r.config_path, Some(PathBuf::from("/etc/sb/c.json")));
    }

    #[test]
    fn returns_none_when_no_config_flag() {
        let u = unit("ExecStart=/usr/bin/sing-box run");
        let r = parse_exec_start(&u).unwrap();
        assert_eq!(r.raw, "/usr/bin/sing-box run");
        assert_eq!(r.config_path, None);
    }

    #[test]
    fn skips_comments_and_blank_lines() {
        let u = "
# comment
[Unit]
Description=x

[Service]
; inline comment
ExecStart=/usr/bin/sing-box run -c /etc/sb/c.json
";
        let r = parse_exec_start(u).unwrap();
        assert_eq!(r.config_path, Some(PathBuf::from("/etc/sb/c.json")));
    }

    #[test]
    fn rejects_unit_without_service_section() {
        let u = "[Unit]\nDescription=x\n[Install]\nWantedBy=x\n";
        assert!(matches!(parse_exec_start(u), Err(ParseError::NoServiceSection)));
    }

    #[test]
    fn rejects_service_without_exec_start() {
        let u = "[Service]\nUser=root\n";
        assert!(matches!(parse_exec_start(u), Err(ParseError::NoExecStart)));
    }

    #[test]
    fn handles_dash_modifier_on_exec_start() {
        let u = unit("ExecStart=-/usr/bin/sing-box run -c /etc/sb/c.json");
        let r = parse_exec_start(&u).unwrap();
        assert_eq!(r.config_path, Some(PathBuf::from("/etc/sb/c.json")));
    }
}
```

- [ ] **Step 2: Add the submodule**

In `crates/boxpilotd/src/legacy/mod.rs`, append:

```rust
pub mod unit_parser;
```

- [ ] **Step 3: Run the tests**

Run: `cargo test -p boxpilotd legacy::unit_parser`
Expected: 8 passes.

- [ ] **Step 4: Commit**

```bash
git add crates/boxpilotd/src/legacy/unit_parser.rs crates/boxpilotd/src/legacy/mod.rs
git commit -m "feat(boxpilotd/legacy): minimal ExecStart parser"
```

---

## Task 7: `legacy::observe::observe` orchestrator

**Files:**
- Create: `crates/boxpilotd/src/legacy/observe.rs`
- Modify: `crates/boxpilotd/src/legacy/mod.rs`

- [ ] **Step 1: Write the failing tests**

Create `crates/boxpilotd/src/legacy/observe.rs`:

```rust
//! `legacy.observe_service` (§6.3, ReadOnly): probe `sing-box.service`,
//! return runtime state + on-disk fragment + extracted config path.

use crate::legacy::path_safety::classify_config_path;
use crate::legacy::unit_parser::parse_exec_start;
use crate::systemd::Systemd;
use boxpilot_ipc::{
    BoxpilotConfig, ConfigPathKind, HelperResult, LegacyObserveServiceResponse, UnitState,
    LEGACY_UNIT_NAME,
};

pub struct ObserveDeps<'a> {
    pub systemd: &'a dyn Systemd,
    pub fs_read: &'a dyn FragmentReader,
}

/// Read the contents of an on-disk unit fragment. Defined as a trait so the
/// observe orchestrator can be tested without touching the real filesystem.
pub trait FragmentReader: Send + Sync {
    fn read_to_string(&self, path: &std::path::Path) -> std::io::Result<String>;
}

pub struct StdFsFragmentReader;
impl FragmentReader for StdFsFragmentReader {
    fn read_to_string(&self, path: &std::path::Path) -> std::io::Result<String> {
        std::fs::read_to_string(path)
    }
}

pub async fn observe(
    cfg: &BoxpilotConfig,
    deps: &ObserveDeps<'_>,
) -> HelperResult<LegacyObserveServiceResponse> {
    let unit_state = deps.systemd.unit_state(LEGACY_UNIT_NAME).await?;
    let detected = !matches!(unit_state, UnitState::NotFound);

    if !detected {
        return Ok(LegacyObserveServiceResponse {
            detected: false,
            unit_name: None,
            fragment_path: None,
            unit_file_state: None,
            exec_start_raw: None,
            config_path: None,
            config_path_kind: ConfigPathKind::Unknown,
            unit_state,
            conflicts_with_managed: false,
        });
    }

    let fragment_path = deps.systemd.fragment_path(LEGACY_UNIT_NAME).await?;
    let unit_file_state = deps.systemd.unit_file_state(LEGACY_UNIT_NAME).await?;

    let (exec_start_raw, config_path) = match fragment_path.as_deref() {
        Some(p) => match deps.fs_read.read_to_string(std::path::Path::new(p)) {
            Ok(text) => match parse_exec_start(&text) {
                Ok(es) => (
                    Some(es.raw),
                    es.config_path.map(|p| p.to_string_lossy().into_owned()),
                ),
                Err(_) => (None, None),
            },
            Err(_) => (None, None),
        },
        None => (None, None),
    };

    let kind = match config_path.as_deref() {
        Some(p) => classify_config_path(std::path::Path::new(p)),
        None => ConfigPathKind::Unknown,
    };

    Ok(LegacyObserveServiceResponse {
        detected: true,
        unit_name: Some(LEGACY_UNIT_NAME.to_string()),
        fragment_path,
        unit_file_state,
        exec_start_raw,
        config_path,
        config_path_kind: kind,
        unit_state,
        conflicts_with_managed: cfg.target_service == LEGACY_UNIT_NAME,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::systemd::testing::FixedSystemd;
    use boxpilot_ipc::{BoxpilotConfig, CoreState};
    use std::cell::RefCell;
    use std::collections::HashMap;
    use std::path::Path;

    struct MapFsReader {
        files: RefCell<HashMap<String, String>>,
    }
    impl MapFsReader {
        fn new(files: &[(&str, &str)]) -> Self {
            let mut m = HashMap::new();
            for (k, v) in files {
                m.insert((*k).to_string(), (*v).to_string());
            }
            MapFsReader {
                files: RefCell::new(m),
            }
        }
    }
    impl FragmentReader for MapFsReader {
        fn read_to_string(&self, path: &Path) -> std::io::Result<String> {
            self.files
                .borrow()
                .get(path.to_string_lossy().as_ref())
                .cloned()
                .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "no such file"))
        }
    }

    fn empty_cfg() -> BoxpilotConfig {
        BoxpilotConfig {
            schema_version: 1,
            target_service: "boxpilot-sing-box.service".into(),
            core_path: None,
            core_state: Some(CoreState::External),
            controller_uid: None,
            active_profile_id: None,
            active_profile_name: None,
            active_profile_sha256: None,
            active_release_id: None,
            activated_at: None,
            previous_release_id: None,
            previous_profile_id: None,
            previous_profile_sha256: None,
            previous_activated_at: None,
        }
    }

    #[tokio::test]
    async fn detected_false_when_unit_not_loaded() {
        let sd = FixedSystemd::new_with_fragment(UnitState::NotFound, None, None);
        let fs = MapFsReader::new(&[]);
        let cfg = empty_cfg();
        let deps = ObserveDeps {
            systemd: &sd,
            fs_read: &fs,
        };
        let r = observe(&cfg, &deps).await.unwrap();
        assert!(!r.detected);
        assert_eq!(r.config_path_kind, ConfigPathKind::Unknown);
    }

    #[tokio::test]
    async fn detected_true_with_system_path_config() {
        let sd = FixedSystemd::new_with_fragment(
            UnitState::Known {
                active_state: "active".into(),
                sub_state: "running".into(),
                load_state: "loaded".into(),
                n_restarts: 0,
                exec_main_status: 0,
            },
            Some("/etc/systemd/system/sing-box.service".into()),
            Some("enabled".into()),
        );
        let fs = MapFsReader::new(&[(
            "/etc/systemd/system/sing-box.service",
            "[Service]\nExecStart=/usr/bin/sing-box run -c /etc/sing-box/config.json\n",
        )]);
        let cfg = empty_cfg();
        let deps = ObserveDeps {
            systemd: &sd,
            fs_read: &fs,
        };
        let r = observe(&cfg, &deps).await.unwrap();
        assert!(r.detected);
        assert_eq!(r.unit_name.as_deref(), Some("sing-box.service"));
        assert_eq!(
            r.config_path.as_deref(),
            Some("/etc/sing-box/config.json")
        );
        assert_eq!(r.config_path_kind, ConfigPathKind::SystemPath);
        assert_eq!(r.unit_file_state.as_deref(), Some("enabled"));
    }

    #[tokio::test]
    async fn user_path_config_is_flagged_unsafe() {
        let sd = FixedSystemd::new_with_fragment(
            UnitState::Known {
                active_state: "inactive".into(),
                sub_state: "dead".into(),
                load_state: "loaded".into(),
                n_restarts: 0,
                exec_main_status: 0,
            },
            Some("/etc/systemd/system/sing-box.service".into()),
            Some("disabled".into()),
        );
        let fs = MapFsReader::new(&[(
            "/etc/systemd/system/sing-box.service",
            "[Service]\nExecStart=/usr/bin/sing-box run -c /home/alice/sb/c.json\n",
        )]);
        let cfg = empty_cfg();
        let deps = ObserveDeps {
            systemd: &sd,
            fs_read: &fs,
        };
        let r = observe(&cfg, &deps).await.unwrap();
        assert_eq!(r.config_path_kind, ConfigPathKind::UserOrEphemeral);
    }

    #[tokio::test]
    async fn unparseable_exec_start_falls_back_to_unknown() {
        let sd = FixedSystemd::new_with_fragment(
            UnitState::Known {
                active_state: "active".into(),
                sub_state: "running".into(),
                load_state: "loaded".into(),
                n_restarts: 0,
                exec_main_status: 0,
            },
            Some("/etc/systemd/system/sing-box.service".into()),
            Some("enabled".into()),
        );
        let fs = MapFsReader::new(&[(
            "/etc/systemd/system/sing-box.service",
            "[Unit]\nDescription=broken\n",
        )]);
        let cfg = empty_cfg();
        let deps = ObserveDeps {
            systemd: &sd,
            fs_read: &fs,
        };
        let r = observe(&cfg, &deps).await.unwrap();
        assert!(r.detected);
        assert_eq!(r.config_path_kind, ConfigPathKind::Unknown);
        assert_eq!(r.exec_start_raw, None);
    }

    #[tokio::test]
    async fn detects_conflict_when_target_service_matches_legacy_name() {
        let sd = FixedSystemd::new_with_fragment(
            UnitState::Known {
                active_state: "active".into(),
                sub_state: "running".into(),
                load_state: "loaded".into(),
                n_restarts: 0,
                exec_main_status: 0,
            },
            Some("/etc/systemd/system/sing-box.service".into()),
            Some("enabled".into()),
        );
        let fs = MapFsReader::new(&[(
            "/etc/systemd/system/sing-box.service",
            "[Service]\nExecStart=/usr/bin/sing-box run -c /etc/sing-box/config.json\n",
        )]);
        let mut cfg = empty_cfg();
        cfg.target_service = "sing-box.service".into();
        let deps = ObserveDeps {
            systemd: &sd,
            fs_read: &fs,
        };
        let r = observe(&cfg, &deps).await.unwrap();
        assert!(r.conflicts_with_managed);
    }
}
```

- [ ] **Step 2: Register the submodule**

In `crates/boxpilotd/src/legacy/mod.rs`, append `pub mod observe;`.

- [ ] **Step 3: Run the tests**

Run: `cargo test -p boxpilotd legacy::observe`
Expected: 5 passes.

- [ ] **Step 4: Commit**

```bash
git add crates/boxpilotd/src/legacy/observe.rs crates/boxpilotd/src/legacy/mod.rs
git commit -m "feat(boxpilotd/legacy): observe orchestrator"
```

---

## Task 8: Wire `legacy.observe_service` into `iface.rs`

**Files:**
- Modify: `crates/boxpilotd/src/iface.rs`
- Modify: `crates/boxpilotd/src/context.rs` (add `fs_fragment_reader`)
- Modify: `crates/boxpilotd/src/main.rs` (instantiate `StdFsFragmentReader`)

- [ ] **Step 1: Read `context.rs` to find where dependencies are wired**

Run: `cargo expand -p boxpilotd context::HelperContext` is not necessary; just open `crates/boxpilotd/src/context.rs` and find the `pub struct HelperContext { ... }` definition + the `pub fn new(...)` constructor. Identify the slot pattern used for `journal: Arc<dyn JournalReader>`.

- [ ] **Step 2: Add a `fs_fragment_reader` slot**

In `HelperContext`:

```rust
pub fs_fragment_reader: Arc<dyn crate::legacy::observe::FragmentReader>,
```

In `HelperContext::new`, add a parameter `fs_fragment_reader: Arc<dyn crate::legacy::observe::FragmentReader>` between the existing slots in the same position used by other `Arc<dyn ...>` deps (kept stable across plans).

In the test helper `ctx_with` (and `ctx_with_recording`, `ctx_with_journal_lines`), add a default fragment reader. Use a closure-driven test fake or reuse a small `MapFsReader` helper exported from `crate::legacy::observe::tests` — easiest is to add a `pub struct InMemoryFragmentReader { pub files: HashMap<String, String> }` in `legacy::observe` (gated on `pub(crate)` visibility, no `#[cfg(test)]` so the context test helpers compile in non-test builds too).

- [ ] **Step 3: Instantiate the real reader in `main.rs`**

In `crates/boxpilotd/src/main.rs::main`, between the existing `Arc::new(...)` lines for `fs_meta` and `version_checker`, add:

```rust
    let fragment_reader = Arc::new(crate::legacy::observe::StdFsFragmentReader);
```

Then pass `fragment_reader` as the new argument to `HelperContext::new`.

- [ ] **Step 4: Replace the stub in `iface.rs`**

Find `legacy_observe_service` in `crates/boxpilotd/src/iface.rs` (around line 309). Replace its body with:

```rust
    async fn legacy_observe_service(
        &self,
        #[zbus(header)] header: zbus::message::Header<'_>,
    ) -> zbus::fdo::Result<String> {
        let sender = extract_sender(&header)?;
        let resp = self
            .do_legacy_observe_service(&sender)
            .await
            .map_err(to_zbus_err)?;
        serde_json::to_string(&resp).map_err(|e| {
            zbus::fdo::Error::Failed(format!("app.boxpilot.Helper1.Ipc: serialize: {e}"))
        })
    }
```

And add the inner method to `impl Helper`:

```rust
    async fn do_legacy_observe_service(
        &self,
        sender: &str,
    ) -> Result<boxpilot_ipc::LegacyObserveServiceResponse, HelperError> {
        let _call =
            dispatch::authorize(&self.ctx, sender, HelperMethod::LegacyObserveService).await?;
        let cfg = self.ctx.load_config().await?;
        let deps = crate::legacy::observe::ObserveDeps {
            systemd: &*self.ctx.systemd,
            fs_read: &*self.ctx.fs_fragment_reader,
        };
        crate::legacy::observe::observe(&cfg, &deps).await
    }
```

- [ ] **Step 5: Add an iface-level test that mirrors `service_status_*`**

Append to `iface::tests`:

```rust
    #[tokio::test]
    async fn legacy_observe_returns_not_detected_when_unit_absent() {
        let tmp = tempdir().unwrap();
        let ctx = Arc::new(ctx_with(
            &tmp,
            None,
            CannedAuthority::allowing(&["app.boxpilot.helper.legacy.observe-service"]),
            UnitState::NotFound,
            &[(":1.42", 1000)],
        ));
        let h = Helper::new(ctx);
        let resp = h.do_legacy_observe_service(":1.42").await.unwrap();
        assert!(!resp.detected);
    }

    #[tokio::test]
    async fn legacy_observe_denied_returns_not_authorized() {
        let tmp = tempdir().unwrap();
        let ctx = Arc::new(ctx_with(
            &tmp,
            None,
            CannedAuthority::denying(&["app.boxpilot.helper.legacy.observe-service"]),
            UnitState::NotFound,
            &[(":1.42", 1000)],
        ));
        let h = Helper::new(ctx);
        let r = h.do_legacy_observe_service(":1.42").await;
        assert!(matches!(r, Err(HelperError::NotAuthorized)));
    }
```

- [ ] **Step 6: Run the tests**

Run: `cargo test -p boxpilotd legacy_observe`
Expected: 2 passes; full `cargo test -p boxpilotd` still green.

- [ ] **Step 7: Commit**

```bash
git add crates/boxpilotd/src/iface.rs crates/boxpilotd/src/context.rs crates/boxpilotd/src/main.rs crates/boxpilotd/src/legacy/observe.rs
git commit -m "feat(boxpilotd): wire legacy.observe_service end-to-end"
```

---

## Task 9: `legacy::backup::backup_unit_file`

**Files:**
- Create: `crates/boxpilotd/src/legacy/backup.rs`
- Modify: `crates/boxpilotd/src/legacy/mod.rs`

- [ ] **Step 1: Write the failing tests**

Create `crates/boxpilotd/src/legacy/backup.rs`:

```rust
//! Spec §5.4 — copy a unit fragment under `/var/lib/boxpilot/backups/units/`
//! before mutating it. Used by migrate-cutover.

use boxpilot_ipc::{HelperError, HelperResult};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

/// Copies `src` to `<backups_units_dir>/<unit_name>-<timestamp>` with mode
/// 0600 so a non-root reader can't see a config that may reference secret
/// material. Returns the absolute backup path. Caller is responsible for
/// supplying a timestamp string that's unique within this backup directory.
pub async fn backup_unit_file(
    src: &Path,
    backups_units_dir: &Path,
    unit_name: &str,
    timestamp: &str,
) -> HelperResult<PathBuf> {
    tokio::fs::create_dir_all(backups_units_dir)
        .await
        .map_err(|e| HelperError::Ipc {
            message: format!("mkdir backups: {e}"),
        })?;
    let dst = backups_units_dir.join(format!("{unit_name}-{timestamp}"));
    let bytes = tokio::fs::read(src).await.map_err(|e| HelperError::Ipc {
        message: format!("read fragment {}: {e}", src.display()),
    })?;
    let tmp = dst.with_extension("part");
    tokio::fs::write(&tmp, &bytes)
        .await
        .map_err(|e| HelperError::Ipc {
            message: format!("write backup tmp: {e}"),
        })?;
    let perm = std::fs::Permissions::from_mode(0o600);
    tokio::fs::set_permissions(&tmp, perm)
        .await
        .map_err(|e| HelperError::Ipc {
            message: format!("chmod backup tmp: {e}"),
        })?;
    let f = tokio::fs::OpenOptions::new()
        .read(true)
        .open(&tmp)
        .await
        .map_err(|e| HelperError::Ipc {
            message: format!("open backup for fsync: {e}"),
        })?;
    f.sync_all().await.map_err(|e| HelperError::Ipc {
        message: format!("fsync backup: {e}"),
    })?;
    tokio::fs::rename(&tmp, &dst)
        .await
        .map_err(|e| HelperError::Ipc {
            message: format!("rename backup: {e}"),
        })?;
    Ok(dst)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    use tempfile::tempdir;

    #[tokio::test]
    async fn backup_copies_bytes_and_sets_0600() {
        let tmp = tempdir().unwrap();
        let src = tmp.path().join("sing-box.service");
        tokio::fs::write(&src, b"[Service]\nExecStart=foo\n")
            .await
            .unwrap();
        let backups = tmp.path().join("backups/units");
        let out = backup_unit_file(&src, &backups, "sing-box.service", "2026-04-29T00-00-00Z")
            .await
            .unwrap();
        assert_eq!(
            tokio::fs::read(&out).await.unwrap(),
            b"[Service]\nExecStart=foo\n"
        );
        let mode = std::fs::metadata(&out).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        assert!(out.starts_with(&backups));
    }

    #[tokio::test]
    async fn backup_fails_when_source_missing() {
        let tmp = tempdir().unwrap();
        let r = backup_unit_file(
            &tmp.path().join("nope"),
            &tmp.path().join("b/u"),
            "x.service",
            "ts",
        )
        .await;
        assert!(matches!(r, Err(HelperError::Ipc { .. })));
    }
}
```

- [ ] **Step 2: Register the submodule**

In `crates/boxpilotd/src/legacy/mod.rs`, append `pub mod backup;`.

- [ ] **Step 3: Run the tests**

Run: `cargo test -p boxpilotd legacy::backup`
Expected: 2 passes.

- [ ] **Step 4: Commit**

```bash
git add crates/boxpilotd/src/legacy/backup.rs crates/boxpilotd/src/legacy/mod.rs
git commit -m "feat(boxpilotd/legacy): unit-fragment backup writer"
```

---

## Task 10: `legacy::migrate::prepare`

**Files:**
- Create: `crates/boxpilotd/src/legacy/migrate.rs`
- Modify: `crates/boxpilotd/src/legacy/mod.rs`

- [ ] **Step 1: Write the failing tests**

Create `crates/boxpilotd/src/legacy/migrate.rs`:

```rust
//! `legacy.migrate_service` (§6.3, HighRisk): two phases.
//!
//! - `Prepare` (read-only with respect to system state, but privileged so we
//!   can read root-owned configs): read fragment + ExecStart + config bytes
//!   + sibling assets, return them to the user-side.
//! - `Cutover` (mutating): stop + disable the legacy unit, back up its
//!   fragment under `/var/lib/boxpilot/backups/units/`. Atomically replaces
//!   the "two services running concurrently" risk with "neither running";
//!   the standard activation pipeline (plan #5) then enables + starts
//!   `boxpilot-sing-box.service` from the imported profile.

use crate::legacy::observe::FragmentReader;
use crate::legacy::path_safety::classify_config_path;
use crate::legacy::unit_parser::parse_exec_start;
use boxpilot_ipc::{
    BoxpilotConfig, ConfigPathKind, HelperError, HelperResult, LegacyMigratePrepareResponse,
    MigratedAsset, BUNDLE_MAX_FILE_BYTES, BUNDLE_MAX_FILE_COUNT, LEGACY_UNIT_NAME,
};
use std::path::{Path, PathBuf};

pub struct PrepareDeps<'a> {
    pub systemd: &'a dyn crate::systemd::Systemd,
    pub fs_read: &'a dyn FragmentReader,
    pub config_reader: &'a dyn ConfigReader,
}

/// Read root-owned config + sibling files. Trait so the migrate logic can be
/// tested without running as root.
pub trait ConfigReader: Send + Sync {
    fn read_file(&self, path: &Path) -> std::io::Result<Vec<u8>>;
    fn read_dir(&self, path: &Path) -> std::io::Result<Vec<DirEntry>>;
    fn metadata_len(&self, path: &Path) -> std::io::Result<u64>;
}

#[derive(Debug, Clone)]
pub struct DirEntry {
    pub path: PathBuf,
    pub is_file: bool,
    pub is_symlink: bool,
}

pub struct StdConfigReader;
impl ConfigReader for StdConfigReader {
    fn read_file(&self, path: &Path) -> std::io::Result<Vec<u8>> {
        std::fs::read(path)
    }
    fn read_dir(&self, path: &Path) -> std::io::Result<Vec<DirEntry>> {
        let mut out = Vec::new();
        for e in std::fs::read_dir(path)? {
            let e = e?;
            let ft = std::fs::symlink_metadata(e.path())?.file_type();
            out.push(DirEntry {
                path: e.path(),
                is_file: ft.is_file(),
                is_symlink: ft.is_symlink(),
            });
        }
        Ok(out)
    }
    fn metadata_len(&self, path: &Path) -> std::io::Result<u64> {
        Ok(std::fs::symlink_metadata(path)?.len())
    }
}

pub async fn prepare(
    cfg: &BoxpilotConfig,
    deps: &PrepareDeps<'_>,
) -> HelperResult<LegacyMigratePrepareResponse> {
    if cfg.target_service == LEGACY_UNIT_NAME {
        return Err(HelperError::LegacyConflictsWithManaged {
            unit: LEGACY_UNIT_NAME.to_string(),
        });
    }
    let unit_state = deps.systemd.unit_state(LEGACY_UNIT_NAME).await?;
    if matches!(unit_state, boxpilot_ipc::UnitState::NotFound) {
        return Err(HelperError::LegacyUnitNotFound {
            unit: LEGACY_UNIT_NAME.to_string(),
        });
    }
    let fragment_path = deps
        .systemd
        .fragment_path(LEGACY_UNIT_NAME)
        .await?
        .ok_or_else(|| HelperError::LegacyExecStartUnparseable {
            reason: "unit has no FragmentPath (transient unit?)".into(),
        })?;
    let unit_text = deps
        .fs_read
        .read_to_string(Path::new(&fragment_path))
        .map_err(|e| HelperError::Ipc {
            message: format!("read fragment {fragment_path}: {e}"),
        })?;
    let exec = parse_exec_start(&unit_text).map_err(|e| HelperError::LegacyExecStartUnparseable {
        reason: e.to_string(),
    })?;
    let config_path = exec
        .config_path
        .ok_or_else(|| HelperError::LegacyExecStartUnparseable {
            reason: "ExecStart had no -c/--config argument".into(),
        })?;
    let kind = classify_config_path(&config_path);
    if matches!(kind, ConfigPathKind::UserOrEphemeral) {
        return Err(HelperError::LegacyConfigPathUnsafe {
            path: config_path.to_string_lossy().into_owned(),
        });
    }
    let cfg_bytes = deps
        .config_reader
        .read_file(&config_path)
        .map_err(|e| HelperError::Ipc {
            message: format!("read legacy config {}: {e}", config_path.display()),
        })?;
    if cfg_bytes.len() as u64 > BUNDLE_MAX_FILE_BYTES {
        return Err(HelperError::LegacyAssetTooLarge {
            path: config_path.to_string_lossy().into_owned(),
            size: cfg_bytes.len() as u64,
            limit: BUNDLE_MAX_FILE_BYTES,
        });
    }

    // Enumerate siblings.
    let parent = config_path.parent().ok_or_else(|| HelperError::Ipc {
        message: "legacy config path has no parent".into(),
    })?;
    let mut assets = Vec::new();
    let entries = deps
        .config_reader
        .read_dir(parent)
        .map_err(|e| HelperError::Ipc {
            message: format!("read_dir {}: {e}", parent.display()),
        })?;
    let mut count = 0u32;
    for e in entries {
        if e.is_symlink || !e.is_file {
            continue;
        }
        if e.path == config_path {
            continue;
        }
        if count >= BUNDLE_MAX_FILE_COUNT - 1 {
            return Err(HelperError::LegacyTooManyAssets {
                count: count + 1,
                limit: BUNDLE_MAX_FILE_COUNT - 1,
            });
        }
        let size = deps
            .config_reader
            .metadata_len(&e.path)
            .map_err(|err| HelperError::Ipc {
                message: format!("stat {}: {err}", e.path.display()),
            })?;
        if size > BUNDLE_MAX_FILE_BYTES {
            return Err(HelperError::LegacyAssetTooLarge {
                path: e.path.to_string_lossy().into_owned(),
                size,
                limit: BUNDLE_MAX_FILE_BYTES,
            });
        }
        let bytes = deps
            .config_reader
            .read_file(&e.path)
            .map_err(|err| HelperError::Ipc {
                message: format!("read {}: {err}", e.path.display()),
            })?;
        let filename = e
            .path
            .file_name()
            .and_then(|s| s.to_str())
            .ok_or_else(|| HelperError::Ipc {
                message: format!("non-utf8 filename under {}", parent.display()),
            })?
            .to_string();
        assets.push(MigratedAsset { filename, bytes });
        count += 1;
    }

    let config_filename = config_path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("config.json")
        .to_string();

    Ok(LegacyMigratePrepareResponse {
        unit_name: LEGACY_UNIT_NAME.to_string(),
        config_path_was: config_path.to_string_lossy().into_owned(),
        config_filename,
        config_bytes: cfg_bytes,
        assets,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::systemd::testing::FixedSystemd;
    use std::cell::RefCell;
    use std::collections::HashMap;

    struct MapFs {
        files: RefCell<HashMap<String, Vec<u8>>>,
        dirs: RefCell<HashMap<String, Vec<DirEntry>>>,
    }
    impl MapFs {
        fn new() -> Self {
            Self {
                files: Default::default(),
                dirs: Default::default(),
            }
        }
        fn add_file(&self, p: &str, bytes: &[u8]) {
            self.files
                .borrow_mut()
                .insert(p.to_string(), bytes.to_vec());
        }
        fn set_dir(&self, p: &str, entries: Vec<DirEntry>) {
            self.dirs.borrow_mut().insert(p.to_string(), entries);
        }
    }
    impl crate::legacy::observe::FragmentReader for MapFs {
        fn read_to_string(&self, path: &Path) -> std::io::Result<String> {
            self.files
                .borrow()
                .get(path.to_string_lossy().as_ref())
                .map(|v| String::from_utf8_lossy(v).into_owned())
                .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "no fragment"))
        }
    }
    impl ConfigReader for MapFs {
        fn read_file(&self, path: &Path) -> std::io::Result<Vec<u8>> {
            self.files
                .borrow()
                .get(path.to_string_lossy().as_ref())
                .cloned()
                .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "no file"))
        }
        fn read_dir(&self, path: &Path) -> std::io::Result<Vec<DirEntry>> {
            self.dirs
                .borrow()
                .get(path.to_string_lossy().as_ref())
                .cloned()
                .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "no dir"))
        }
        fn metadata_len(&self, path: &Path) -> std::io::Result<u64> {
            self.files
                .borrow()
                .get(path.to_string_lossy().as_ref())
                .map(|v| v.len() as u64)
                .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "no file"))
        }
    }

    fn cfg() -> BoxpilotConfig {
        BoxpilotConfig {
            schema_version: 1,
            target_service: "boxpilot-sing-box.service".into(),
            core_path: None,
            core_state: None,
            controller_uid: None,
            active_profile_id: None,
            active_profile_name: None,
            active_profile_sha256: None,
            active_release_id: None,
            activated_at: None,
            previous_release_id: None,
            previous_profile_id: None,
            previous_profile_sha256: None,
            previous_activated_at: None,
        }
    }

    fn systemd_with_fragment(state: boxpilot_ipc::UnitState) -> FixedSystemd {
        FixedSystemd::new_with_fragment(
            state,
            Some("/etc/systemd/system/sing-box.service".into()),
            Some("enabled".into()),
        )
    }

    #[tokio::test]
    async fn prepare_returns_config_and_assets_for_safe_path() {
        let fs = MapFs::new();
        fs.add_file(
            "/etc/systemd/system/sing-box.service",
            b"[Service]\nExecStart=/usr/bin/sing-box run -c /etc/sing-box/config.json\n",
        );
        fs.add_file("/etc/sing-box/config.json", br#"{"log":{}}"#);
        fs.add_file("/etc/sing-box/geosite.db", b"asset-bytes");
        fs.set_dir(
            "/etc/sing-box",
            vec![
                DirEntry {
                    path: PathBuf::from("/etc/sing-box/config.json"),
                    is_file: true,
                    is_symlink: false,
                },
                DirEntry {
                    path: PathBuf::from("/etc/sing-box/geosite.db"),
                    is_file: true,
                    is_symlink: false,
                },
            ],
        );
        let sd = systemd_with_fragment(boxpilot_ipc::UnitState::Known {
            active_state: "active".into(),
            sub_state: "running".into(),
            load_state: "loaded".into(),
            n_restarts: 0,
            exec_main_status: 0,
        });
        let deps = PrepareDeps {
            systemd: &sd,
            fs_read: &fs,
            config_reader: &fs,
        };
        let r = prepare(&cfg(), &deps).await.unwrap();
        assert_eq!(r.config_filename, "config.json");
        assert_eq!(r.config_bytes, br#"{"log":{}}"#);
        assert_eq!(r.assets.len(), 1);
        assert_eq!(r.assets[0].filename, "geosite.db");
        assert_eq!(r.assets[0].bytes, b"asset-bytes");
    }

    #[tokio::test]
    async fn prepare_refuses_user_path() {
        let fs = MapFs::new();
        fs.add_file(
            "/etc/systemd/system/sing-box.service",
            b"[Service]\nExecStart=/usr/bin/sing-box run -c /home/alice/sb/c.json\n",
        );
        let sd = systemd_with_fragment(boxpilot_ipc::UnitState::Known {
            active_state: "active".into(),
            sub_state: "running".into(),
            load_state: "loaded".into(),
            n_restarts: 0,
            exec_main_status: 0,
        });
        let deps = PrepareDeps {
            systemd: &sd,
            fs_read: &fs,
            config_reader: &fs,
        };
        let r = prepare(&cfg(), &deps).await;
        assert!(matches!(r, Err(HelperError::LegacyConfigPathUnsafe { .. })));
    }

    #[tokio::test]
    async fn prepare_refuses_when_unit_not_found() {
        let fs = MapFs::new();
        let sd = systemd_with_fragment(boxpilot_ipc::UnitState::NotFound);
        let deps = PrepareDeps {
            systemd: &sd,
            fs_read: &fs,
            config_reader: &fs,
        };
        let r = prepare(&cfg(), &deps).await;
        assert!(matches!(r, Err(HelperError::LegacyUnitNotFound { .. })));
    }

    #[tokio::test]
    async fn prepare_refuses_when_target_service_collides_with_legacy_name() {
        let fs = MapFs::new();
        let sd = systemd_with_fragment(boxpilot_ipc::UnitState::Known {
            active_state: "active".into(),
            sub_state: "running".into(),
            load_state: "loaded".into(),
            n_restarts: 0,
            exec_main_status: 0,
        });
        let mut c = cfg();
        c.target_service = "sing-box.service".into();
        let deps = PrepareDeps {
            systemd: &sd,
            fs_read: &fs,
            config_reader: &fs,
        };
        let r = prepare(&c, &deps).await;
        assert!(matches!(r, Err(HelperError::LegacyConflictsWithManaged { .. })));
    }

    #[tokio::test]
    async fn prepare_skips_symlinks_and_subdirs() {
        let fs = MapFs::new();
        fs.add_file(
            "/etc/systemd/system/sing-box.service",
            b"[Service]\nExecStart=/usr/bin/sing-box run -c /etc/sing-box/config.json\n",
        );
        fs.add_file("/etc/sing-box/config.json", b"{}");
        fs.set_dir(
            "/etc/sing-box",
            vec![
                DirEntry {
                    path: PathBuf::from("/etc/sing-box/config.json"),
                    is_file: true,
                    is_symlink: false,
                },
                DirEntry {
                    path: PathBuf::from("/etc/sing-box/symlink-to-secret"),
                    is_file: false,
                    is_symlink: true,
                },
                DirEntry {
                    path: PathBuf::from("/etc/sing-box/subdir"),
                    is_file: false,
                    is_symlink: false,
                },
            ],
        );
        let sd = systemd_with_fragment(boxpilot_ipc::UnitState::Known {
            active_state: "active".into(),
            sub_state: "running".into(),
            load_state: "loaded".into(),
            n_restarts: 0,
            exec_main_status: 0,
        });
        let deps = PrepareDeps {
            systemd: &sd,
            fs_read: &fs,
            config_reader: &fs,
        };
        let r = prepare(&cfg(), &deps).await.unwrap();
        assert!(r.assets.is_empty());
    }
}
```

- [ ] **Step 2: Register the submodule**

In `crates/boxpilotd/src/legacy/mod.rs`, append `pub mod migrate;`.

- [ ] **Step 3: Run the tests**

Run: `cargo test -p boxpilotd legacy::migrate::tests`
Expected: 5 passes.

- [ ] **Step 4: Commit**

```bash
git add crates/boxpilotd/src/legacy/migrate.rs crates/boxpilotd/src/legacy/mod.rs
git commit -m "feat(boxpilotd/legacy): migrate prepare phase"
```

---

## Task 11: `legacy::migrate::cutover` (stop + disable + backup)

**Files:**
- Modify: `crates/boxpilotd/src/legacy/migrate.rs`

- [ ] **Step 1: Write the failing tests**

Append to `mod tests` in `crates/boxpilotd/src/legacy/migrate.rs`:

```rust
    use crate::systemd::testing::{RecordedCall, RecordingSystemd};
    use tempfile::tempdir;

    #[tokio::test]
    async fn cutover_stops_then_disables_then_backs_up() {
        let tmp = tempdir().unwrap();
        // Stage a fragment file so backup_unit_file has something to copy.
        let fragments = tmp.path().join("etc/systemd/system");
        tokio::fs::create_dir_all(&fragments).await.unwrap();
        let fragment = fragments.join("sing-box.service");
        tokio::fs::write(&fragment, b"[Service]\nExecStart=foo\n")
            .await
            .unwrap();

        let recording = RecordingSystemd::new(boxpilot_ipc::UnitState::NotFound);
        recording.set_fragment_path(Some(fragment.to_string_lossy().into_owned()));

        let backups = tmp.path().join("var/lib/boxpilot/backups/units");
        let resp = cutover(
            &CutoverDeps {
                systemd: &recording,
                backups_units_dir: &backups,
                now_iso: || "2026-04-29T00-00-00Z".into(),
            },
            "sing-box.service",
        )
        .await
        .unwrap();

        // Order: StopUnit before DisableUnitFiles. backup arrives after.
        let calls = recording.calls();
        let stop_idx = calls
            .iter()
            .position(|c| matches!(c, RecordedCall::StopUnit(u) if u == "sing-box.service"))
            .expect("stop call");
        let disable_idx = calls
            .iter()
            .position(|c| matches!(c, RecordedCall::DisableUnitFiles(v) if v == &vec!["sing-box.service".to_string()]))
            .expect("disable call");
        assert!(stop_idx < disable_idx, "stop must precede disable");

        assert!(resp.backup_unit_path.starts_with(&backups.to_string_lossy().into_owned()));
        assert!(tokio::fs::metadata(&resp.backup_unit_path).await.is_ok());
    }

    #[tokio::test]
    async fn cutover_aborts_when_stop_returns_systemd_error() {
        // FixedSystemd returns Ok(()) for stop, so use a small wrapper
        // that fails StopUnit; reuse the test struct.
        struct StopFails;
        #[async_trait::async_trait]
        impl crate::systemd::Systemd for StopFails {
            async fn unit_state(&self, _: &str) -> Result<boxpilot_ipc::UnitState, HelperError> {
                Ok(boxpilot_ipc::UnitState::NotFound)
            }
            async fn start_unit(&self, _: &str) -> Result<(), HelperError> {
                Ok(())
            }
            async fn stop_unit(&self, _: &str) -> Result<(), HelperError> {
                Err(HelperError::Systemd {
                    message: "EBUSY".into(),
                })
            }
            async fn restart_unit(&self, _: &str) -> Result<(), HelperError> {
                Ok(())
            }
            async fn enable_unit_files(&self, _: &[String]) -> Result<(), HelperError> {
                Ok(())
            }
            async fn disable_unit_files(&self, _: &[String]) -> Result<(), HelperError> {
                Ok(())
            }
            async fn reload(&self) -> Result<(), HelperError> {
                Ok(())
            }
            async fn fragment_path(&self, _: &str) -> Result<Option<String>, HelperError> {
                Ok(None)
            }
            async fn unit_file_state(&self, _: &str) -> Result<Option<String>, HelperError> {
                Ok(None)
            }
        }
        let tmp = tempdir().unwrap();
        let r = cutover(
            &CutoverDeps {
                systemd: &StopFails,
                backups_units_dir: &tmp.path().join("b/u"),
                now_iso: || "ts".into(),
            },
            "sing-box.service",
        )
        .await;
        assert!(matches!(r, Err(HelperError::LegacyStopFailed { .. })));
    }
```

- [ ] **Step 2: Add the `cutover` function**

Append to `crates/boxpilotd/src/legacy/migrate.rs`:

```rust
pub struct CutoverDeps<'a> {
    pub systemd: &'a dyn crate::systemd::Systemd,
    pub backups_units_dir: &'a Path,
    pub now_iso: fn() -> String,
}

pub async fn cutover(
    deps: &CutoverDeps<'_>,
    unit_name: &str,
) -> HelperResult<boxpilot_ipc::LegacyMigrateCutoverResponse> {
    deps.systemd
        .stop_unit(unit_name)
        .await
        .map_err(|e| HelperError::LegacyStopFailed {
            unit: unit_name.to_string(),
            message: match e {
                HelperError::Systemd { message } => message,
                other => format!("{other}"),
            },
        })?;

    deps.systemd
        .disable_unit_files(&[unit_name.to_string()])
        .await
        .map_err(|e| HelperError::LegacyDisableFailed {
            unit: unit_name.to_string(),
            message: match e {
                HelperError::Systemd { message } => message,
                other => format!("{other}"),
            },
        })?;

    let fragment_path = deps
        .systemd
        .fragment_path(unit_name)
        .await
        .ok()
        .flatten();
    let backup_path = match fragment_path {
        Some(p) => crate::legacy::backup::backup_unit_file(
            Path::new(&p),
            deps.backups_units_dir,
            unit_name,
            &(deps.now_iso)(),
        )
        .await
        .map(|pb| pb.to_string_lossy().into_owned())?,
        None => String::new(), // unit had no on-disk fragment; backup is a no-op
    };

    let final_unit_state = deps.systemd.unit_state(unit_name).await?;

    Ok(boxpilot_ipc::LegacyMigrateCutoverResponse {
        unit_name: unit_name.to_string(),
        backup_unit_path: backup_path,
        final_unit_state,
    })
}

/// Single entry point that dispatches `LegacyMigrateRequest::Prepare` /
/// `Cutover` to the right helper. Used by `iface::do_legacy_migrate_service`.
pub async fn run(
    cfg: &BoxpilotConfig,
    req: boxpilot_ipc::LegacyMigrateRequest,
    prep_deps: &PrepareDeps<'_>,
    cut_deps: &CutoverDeps<'_>,
) -> HelperResult<boxpilot_ipc::LegacyMigrateResponse> {
    match req {
        boxpilot_ipc::LegacyMigrateRequest::Prepare => {
            let r = prepare(cfg, prep_deps).await?;
            Ok(boxpilot_ipc::LegacyMigrateResponse::Prepare(r))
        }
        boxpilot_ipc::LegacyMigrateRequest::Cutover => {
            if cfg.target_service == LEGACY_UNIT_NAME {
                return Err(HelperError::LegacyConflictsWithManaged {
                    unit: LEGACY_UNIT_NAME.to_string(),
                });
            }
            let r = cutover(cut_deps, LEGACY_UNIT_NAME).await?;
            Ok(boxpilot_ipc::LegacyMigrateResponse::Cutover(r))
        }
    }
}
```

- [ ] **Step 3: Run the tests**

Run: `cargo test -p boxpilotd legacy::migrate`
Expected: 7 passes total (5 from prepare + 2 from cutover).

- [ ] **Step 4: Commit**

```bash
git add crates/boxpilotd/src/legacy/migrate.rs
git commit -m "feat(boxpilotd/legacy): cutover + run dispatcher"
```

---

## Task 12: Wire `legacy.migrate_service` into `iface.rs`

**Files:**
- Modify: `crates/boxpilotd/src/iface.rs`
- Modify: `crates/boxpilotd/src/context.rs` (add `Arc<dyn migrate::ConfigReader>`)
- Modify: `crates/boxpilotd/src/main.rs` (instantiate `StdConfigReader`)

- [ ] **Step 1: Wire a `config_reader` slot through `HelperContext`**

In `context.rs`, add:

```rust
pub config_reader: Arc<dyn crate::legacy::migrate::ConfigReader>,
```

Pass it through `HelperContext::new` (next to `fs_fragment_reader`). In `main.rs`, instantiate `Arc::new(crate::legacy::migrate::StdConfigReader)` and forward it. In `context::testing` helpers, default it to a no-op `MapFs` shim (an empty in-memory reader is fine for the existing tests; legacy-specific tests provide their own).

- [ ] **Step 2: Add the wire helper to `iface.rs`**

Append to `impl Helper`:

```rust
    async fn do_legacy_migrate_service(
        &self,
        sender: &str,
        req: boxpilot_ipc::LegacyMigrateRequest,
    ) -> Result<boxpilot_ipc::LegacyMigrateResponse, HelperError> {
        let _call =
            dispatch::authorize(&self.ctx, sender, HelperMethod::LegacyMigrateService).await?;
        let cfg = self.ctx.load_config().await?;
        let prep = crate::legacy::migrate::PrepareDeps {
            systemd: &*self.ctx.systemd,
            fs_read: &*self.ctx.fs_fragment_reader,
            config_reader: &*self.ctx.config_reader,
        };
        let cut = crate::legacy::migrate::CutoverDeps {
            systemd: &*self.ctx.systemd,
            backups_units_dir: &self.ctx.paths.backups_units_dir(),
            now_iso: || chrono::Utc::now().format("%Y-%m-%dT%H-%M-%SZ").to_string(),
        };
        crate::legacy::migrate::run(&cfg, req, &prep, &cut).await
    }
```

Replace the stub:

```rust
    async fn legacy_migrate_service(
        &self,
        #[zbus(header)] header: zbus::message::Header<'_>,
        request_json: String,
    ) -> zbus::fdo::Result<String> {
        let sender = extract_sender(&header)?;
        let req: boxpilot_ipc::LegacyMigrateRequest =
            serde_json::from_str(&request_json).map_err(|e| {
                zbus::fdo::Error::Failed(format!("app.boxpilot.Helper1.Ipc: parse: {e}"))
            })?;
        let resp = self
            .do_legacy_migrate_service(&sender, req)
            .await
            .map_err(to_zbus_err)?;
        serde_json::to_string(&resp).map_err(|e| {
            zbus::fdo::Error::Failed(format!("app.boxpilot.Helper1.Ipc: serialize: {e}"))
        })
    }
```

(The new `request_json` parameter is added because the wire stub had no payload; this matches the pattern of `service_logs` / `core_install_managed`.)

- [ ] **Step 2.5: Verify the polkit policy already declares the renamed action arity**

Run: `grep -n 'legacy.migrate-service' packaging/linux/polkit-1/actions/app.boxpilot.helper.policy`
Expected: one action declaration (already present at line ~112).

- [ ] **Step 3: Iface-level test**

Append to `iface::tests`:

```rust
    #[tokio::test]
    async fn legacy_migrate_prepare_passes_dispatch_then_returns_unit_not_found_when_absent() {
        let tmp = tempdir().unwrap();
        let ctx = Arc::new(ctx_with(
            &tmp,
            None,
            CannedAuthority::allowing(&["app.boxpilot.helper.legacy.migrate-service"]),
            UnitState::NotFound,
            &[(":1.42", 1000)],
        ));
        let h = Helper::new(ctx);
        let r = h
            .do_legacy_migrate_service(":1.42", boxpilot_ipc::LegacyMigrateRequest::Prepare)
            .await;
        assert!(matches!(r, Err(HelperError::LegacyUnitNotFound { .. })));
    }
```

- [ ] **Step 4: Run the tests**

Run: `cargo test -p boxpilotd legacy_migrate && cargo test -p boxpilotd`
Expected: new test passes, full suite still green.

- [ ] **Step 5: Commit**

```bash
git add crates/boxpilotd/src/iface.rs crates/boxpilotd/src/context.rs crates/boxpilotd/src/main.rs
git commit -m "feat(boxpilotd): wire legacy.migrate_service end-to-end"
```

---

## Task 13: Tauri commands + `helper_client` proxies

**Files:**
- Modify: `crates/boxpilot-tauri/src/helper_client.rs`
- Modify: `crates/boxpilot-tauri/src/commands.rs`
- Modify: `crates/boxpilot-tauri/src/lib.rs`

- [ ] **Step 1: Add proxy methods to `helper_client.rs`**

In the `trait Helper` proxy block:

```rust
    #[zbus(name = "LegacyObserveService")]
    fn legacy_observe_service(&self) -> zbus::Result<String>;
    #[zbus(name = "LegacyMigrateService")]
    fn legacy_migrate_service(&self, request_json: &str) -> zbus::Result<String>;
```

Add typed wrappers:

```rust
    pub async fn legacy_observe_service(
        &self,
    ) -> Result<boxpilot_ipc::LegacyObserveServiceResponse, ClientError> {
        let proxy = HelperProxy::new(&self.conn).await?;
        let json = proxy.legacy_observe_service().await?;
        serde_json::from_str(&json).map_err(|e| ClientError::Decode(e.to_string()))
    }

    pub async fn legacy_migrate_service(
        &self,
        req: &boxpilot_ipc::LegacyMigrateRequest,
    ) -> Result<boxpilot_ipc::LegacyMigrateResponse, ClientError> {
        let proxy = HelperProxy::new(&self.conn).await?;
        let json = proxy
            .legacy_migrate_service(&serde_json::to_string(req).unwrap())
            .await?;
        serde_json::from_str(&json).map_err(|e| ClientError::Decode(e.to_string()))
    }
```

- [ ] **Step 2: Add Tauri commands in `commands.rs`**

```rust
#[tauri::command]
pub async fn helper_legacy_observe_service(
) -> Result<boxpilot_ipc::LegacyObserveServiceResponse, CommandError> {
    let c = HelperClient::connect().await?;
    Ok(c.legacy_observe_service().await?)
}

#[tauri::command]
pub async fn helper_legacy_migrate_prepare(
) -> Result<boxpilot_ipc::LegacyMigratePrepareResponse, CommandError> {
    let c = HelperClient::connect().await?;
    let r = c
        .legacy_migrate_service(&boxpilot_ipc::LegacyMigrateRequest::Prepare)
        .await?;
    match r {
        boxpilot_ipc::LegacyMigrateResponse::Prepare(p) => Ok(p),
        boxpilot_ipc::LegacyMigrateResponse::Cutover(_) => Err(CommandError {
            code: "decode".into(),
            message: "expected Prepare response, got Cutover".into(),
        }),
    }
}

#[tauri::command]
pub async fn helper_legacy_migrate_cutover(
) -> Result<boxpilot_ipc::LegacyMigrateCutoverResponse, CommandError> {
    let c = HelperClient::connect().await?;
    let r = c
        .legacy_migrate_service(&boxpilot_ipc::LegacyMigrateRequest::Cutover)
        .await?;
    match r {
        boxpilot_ipc::LegacyMigrateResponse::Cutover(p) => Ok(p),
        boxpilot_ipc::LegacyMigrateResponse::Prepare(_) => Err(CommandError {
            code: "decode".into(),
            message: "expected Cutover response, got Prepare".into(),
        }),
    }
}
```

- [ ] **Step 3: Register commands in `lib.rs`**

Add to the `tauri::generate_handler![...]` list, between `commands::helper_service_logs` and `profile_cmds::profile_list`:

```rust
            commands::helper_legacy_observe_service,
            commands::helper_legacy_migrate_prepare,
            commands::helper_legacy_migrate_cutover,
```

- [ ] **Step 4: Run the build**

Run: `cargo build -p boxpilot-tauri`
Expected: clean build.

- [ ] **Step 5: Commit**

```bash
git add crates/boxpilot-tauri/
git commit -m "feat(tauri): legacy observe / migrate-prepare / migrate-cutover commands"
```

---

## Task 14: Frontend TypeScript wrappers

**Files:**
- Modify: `frontend/src/api/types.ts`
- Modify: `frontend/src/api/helper.ts`

- [ ] **Step 1: Add the new types**

Append to `frontend/src/api/types.ts`:

```typescript
export type ConfigPathKind = "system_path" | "user_or_ephemeral" | "unknown";

export interface LegacyObserveServiceResponse {
  detected: boolean;
  unit_name: string | null;
  fragment_path: string | null;
  unit_file_state: string | null;
  exec_start_raw: string | null;
  config_path: string | null;
  config_path_kind: ConfigPathKind;
  unit_state: UnitState;
  conflicts_with_managed: boolean;
}

export interface MigratedAsset {
  filename: string;
  bytes: number[];
}

export interface LegacyMigratePrepareResponse {
  unit_name: string;
  config_path_was: string;
  config_filename: string;
  config_bytes: number[];
  assets: MigratedAsset[];
}

export interface LegacyMigrateCutoverResponse {
  unit_name: string;
  backup_unit_path: string;
  final_unit_state: UnitState;
}
```

- [ ] **Step 2: Add invoker functions to `helper.ts`**

Append:

```typescript
import type {
  LegacyObserveServiceResponse,
  LegacyMigratePrepareResponse,
  LegacyMigrateCutoverResponse,
} from "./types";

export async function legacyObserveService(): Promise<LegacyObserveServiceResponse> {
  return await invoke<LegacyObserveServiceResponse>("helper_legacy_observe_service");
}

export async function legacyMigratePrepare(): Promise<LegacyMigratePrepareResponse> {
  return await invoke<LegacyMigratePrepareResponse>("helper_legacy_migrate_prepare");
}

export async function legacyMigrateCutover(): Promise<LegacyMigrateCutoverResponse> {
  return await invoke<LegacyMigrateCutoverResponse>("helper_legacy_migrate_cutover");
}
```

The existing top-of-file `import` block already pulls from `./types`; either consolidate the new imports into it, or leave the trailing `import type {...}` line (TS allows it). Match the existing style.

- [ ] **Step 3: Run the frontend type-check**

Run: `cd frontend && npm run build`
Expected: clean build (or whichever check command the existing plans use; check `frontend/package.json` if unsure).

- [ ] **Step 4: Commit**

```bash
git add frontend/src/api/
git commit -m "feat(frontend): legacy observe / migrate API wrappers"
```

---

## Task 15: End-to-end `cargo fmt` + `cargo clippy` + full test pass

**Files:** (no edits unless lints fire)

- [ ] **Step 1: Run formatters and linters**

Run: `cargo fmt --all -- --check && cargo clippy --workspace --all-targets -- -D warnings`
Expected: clean. Fix any complaints inline (typically fallout from new `match` arms; clippy may want `if let` over single-arm matches). Re-run until clean.

- [ ] **Step 2: Run the full test suite**

Run: `cargo test --workspace`
Expected: all tests pass. Test count should be ~277 (current main) + ≥ 30 from the new tests in this plan.

- [ ] **Step 3: Commit if anything changed**

```bash
git add -A
git commit -m "chore: cargo fmt + clippy follow-up for plan #6"
```

If nothing changed, skip the commit.

---

## Task 16: Smoke procedure document

**Files:**
- Create: `docs/superpowers/plans/2026-04-29-legacy-service-handling-smoke-procedure.md`

- [ ] **Step 1: Write the document**

Create the file with this content (mirrors the format of prior smoke docs in `docs/superpowers/plans/`):

```markdown
# Plan #6 — Legacy `sing-box.service` Handling — Smoke Procedure

Smoke target: a Linux desktop with a hand-rolled `sing-box.service` already installed (e.g. via the upstream `.deb` or a manual unit file) and an installed `boxpilotd` from this branch.

## Preconditions

- `boxpilotd` running, registered on the system bus.
- An existing `/etc/systemd/system/sing-box.service` unit with an `ExecStart=/usr/bin/sing-box run -c /etc/sing-box/config.json` line and a real config + at least one sibling asset (e.g. `geosite.db`) in `/etc/sing-box/`.
- The legacy unit is NOT named `boxpilot-sing-box.service`.
- `boxpilot.toml::target_service` is `boxpilot-sing-box.service` (default).

## 1. Observation — happy path

```sh
busctl --system call app.boxpilot.Helper /app/boxpilot/Helper app.boxpilot.Helper1 LegacyObserveService
```

Expected JSON (fields):

- `detected: true`
- `unit_name: "sing-box.service"`
- `fragment_path: "/etc/systemd/system/sing-box.service"`
- `config_path: "/etc/sing-box/config.json"`
- `config_path_kind: "system_path"`
- `unit_state.active_state` reflects current systemctl state.
- `conflicts_with_managed: false`

## 2. Observation — user-path config

Edit the legacy fragment to point at `/home/<you>/sb.json`, then `systemctl daemon-reload && systemctl restart sing-box.service`. Re-run LegacyObserveService.

Expected: `config_path_kind: "user_or_ephemeral"`. The GUI should refuse migration; the `LegacyConfigPathUnsafe` error appears only on `LegacyMigrateService prepare`, not on observe — observe is informational.

Restore the original fragment before continuing.

## 3. Migration prepare

```sh
busctl --system call app.boxpilot.Helper /app/boxpilot/Helper app.boxpilot.Helper1 LegacyMigrateService s '{"step":"prepare"}'
```

Expected:

- `step: "prepare"`
- `config_filename: "config.json"`
- `config_bytes` = the bytes of the legacy config (verify via `sha256sum /etc/sing-box/config.json` and compare to a manual hash of the response).
- `assets` contains exactly the regular files in `/etc/sing-box/` other than `config.json` and not symlinks.

## 4. User-side import

In the running GUI, exercise `profile_import_file` (or `profile_import_dir` if assets present) feeding the prepare response. Confirm the new profile appears in `~/.local/share/boxpilot/profiles/`.

## 5. Migration cutover

Before cutover, capture: `systemctl is-active sing-box.service` and `systemctl is-enabled sing-box.service`.

```sh
busctl --system call app.boxpilot.Helper /app/boxpilot/Helper app.boxpilot.Helper1 LegacyMigrateService s '{"step":"cutover"}'
```

Expected:

- After call, `systemctl is-active sing-box.service` reports `inactive` (or unit absent).
- `systemctl is-enabled sing-box.service` reports `disabled` (or `not-found`).
- `/var/lib/boxpilot/backups/units/sing-box.service-<timestamp>` exists, mode `0600`, root-owned, contents identical to the original fragment.

## 6. Activation

Click "Activate this profile" in the GUI for the imported profile. Confirm the standard plan-#5 activation pipeline runs and `boxpilot-sing-box.service` is enabled + started without error. The two services never run concurrently because cutover stopped the legacy unit before activation began.

## 7. Recovery if cutover fails

If `LegacyStopFailed` or `LegacyDisableFailed` is returned, the legacy unit is **not** torn down. Verify `systemctl` still shows the legacy unit running (or whatever it was). The user has no data loss — they can retry, or pursue advanced takeover (out of scope for v1.0).

## 8. Authorization gating

As a non-controller user, run the bus call from §1. Expected: `LegacyObserveService` returns successfully (it's read-only with `allow_any: yes`).

As a non-controller user, run the bus call from §3. Expected: polkit prompt for admin auth (`auth_admin`, no caching).
```

- [ ] **Step 2: Commit**

```bash
git add docs/superpowers/plans/2026-04-29-legacy-service-handling-smoke-procedure.md
git commit -m "docs: plan #6 smoke procedure"
```

---

## Task 17: Update memory with plan #6 status

**Files:**
- Modify: `~/.claude/projects/-home-connor-johnson-workspace-BoxPilot/memory/project_plan_progress.md`

- [ ] **Step 1: Update the progress table**

Edit the row for plan #6 in `project_plan_progress.md` to reflect the merged commit (use the actual commit short SHA after the squash-merge to main; placeholder `TBD` until then). Keep the table format consistent with rows #2–#5.

Add a new follow-up note section if any of the following were left for later:
- The `legacy.observe_service` GUI surfacing (lives in plan #7).
- `sing-box@*.service` template instances (out of scope; document if it ever blocks a smoke).
- Advanced in-place takeover (spec §8 mode 3) — explicitly deferred.

- [ ] **Step 2: No commit needed (memory is local-only)**

The memory file is gitignored. Just save and move on.

---

## Self-Review Notes

- **Spec coverage** — §6.3 actions: ✅ both `legacy.observe_service` and `legacy.migrate_service` (Tasks 8 + 12). §8 mode 1 (observation): ✅ Tasks 5–8. §8 mode 2 (migration): ✅ Tasks 10–12 — copy config + sibling assets (prepare), atomic stop+disable+backup (cutover); the "enable + start `boxpilot-sing-box.service`" half is intentionally delegated to plan #5's activation pipeline so each plan stays single-subsystem. §8 step "warns before migration if path is under /home/tmp/run-user" → `LegacyConfigPathUnsafe` in prepare. §8 step "the two services must never run concurrently" → cutover stops legacy synchronously before any subsequent activation, and `service.install_managed` (already plan #3) doesn't enable/start, so the order is enforced. §5.4 backups → Task 9. §16.14 + §16.15 acceptance → covered by smoke procedure (Task 16).
- **Out of scope (deliberate)** — Mode 3 (advanced in-place takeover with diff + drop-in editor): spec §8 calls it "Hidden under advanced settings" and "Not part of the default path"; not implemented in v1.0. Template-unit detection (`sing-box@*.service`): no spec language. Auto-asset-graph extraction from config JSON: plan #5/#7's territory.
- **Type consistency** — `ConfigPathKind` snake-case wire form is asserted in Task 1's tests and re-used unchanged through observe/prepare/frontend. `LegacyMigrateRequest`/`Response` use the same `step` discriminator across all four touch points (IPC, helper, Tauri, frontend). `LEGACY_UNIT_NAME` is the single source of truth.
- **No placeholders** — every code step shows the actual code; every test shows the actual assertions; every command is runnable. Task 12 leaves the `context.rs` test-helper update as "default it to a no-op `MapFs` shim" without literal source: this is because `ctx_with` already exists with five+ args and the precise signature edit depends on the order chosen during Task 8's slot insertion. The instruction is concrete enough that the engineer can read `context.rs` and make the right tweak without further design work; no behavior is left undefined.
