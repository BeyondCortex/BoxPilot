# Activation Pipeline (plan #5) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement spec §9.2 / §10 / §7.2 — fd-passing activation transport, atomic rename pipeline, auto + manual rollback, crash recovery, and GC — replacing the `profile.activate_bundle` and `profile.rollback_release` stubs in `boxpilotd`.

**Architecture:** User-side `boxpilot-profile` tars a staging directory into a sealed `memfd_create` and hands the fd over D-Bus UNIX_FD. `boxpilotd` mmap-walks the tar with strict §9.2 entry filters, runs `<core_path> check`, swaps `/etc/boxpilot/active` via `rename(2)` on `active.new`, restarts the unit, and verifies through plan #3's `service::verify`. Auto-rollback is symmetric — same swap helpers re-aim at the previous release. State surfaces via four explicit outcomes: `Active`, `RolledBack`, `RollbackTargetMissing`, `RollbackUnstartable`.

**Tech Stack:** Rust 2021 / `zbus` 5 (with UNIX_FD) / `tar` 0.4 / `nix` 0.29 (`memfd_create`, fcntl seals) / `tokio` / `tempfile`. No new workspace deps.

**Spec reference:** `docs/superpowers/specs/2026-04-30-activation-pipeline-design.md`.

---

## File map

**Create:**
- `crates/boxpilotd/src/profile/mod.rs`
- `crates/boxpilotd/src/profile/activate.rs`
- `crates/boxpilotd/src/profile/rollback.rs`
- `crates/boxpilotd/src/profile/unpack.rs`
- `crates/boxpilotd/src/profile/release.rs`
- `crates/boxpilotd/src/profile/gc.rs`
- `crates/boxpilotd/src/profile/recovery.rs`
- `crates/boxpilotd/src/profile/checker.rs` (sing-box check trait + impl)
- `crates/boxpilotd/src/profile/verifier.rs` (ServiceVerifier trait wrapping verify::wait_for_running)
- `crates/boxpilotd/tests/activate_pipeline.rs`
- `docs/superpowers/plans/2026-04-30-activation-pipeline-smoke-procedure.md`

**Modify:**
- `crates/boxpilotd/src/main.rs` — wire `profile::recovery::reconcile`, declare new `profile` module.
- `crates/boxpilotd/src/paths.rs` — add `releases_dir`, `staging_dir`, `active_symlink`, `release_dir`, `staging_subdir`.
- `crates/boxpilotd/src/iface.rs` — replace `profile_activate_bundle` and `profile_rollback_release` stubs.
- `crates/boxpilotd/src/context.rs` — add `checker`, `verifier` deps.
- `crates/boxpilotd/src/service/verify.rs` — drop `#[allow(dead_code)]`.
- `crates/boxpilotd/src/core/commit.rs` — extend `StateCommit::TomlUpdates` with active/previous fields.
- `crates/boxpilot-ipc/src/error.rs` — add new variants.
- `crates/boxpilot-ipc/src/config.rs` — add `previous_*` fields.
- `crates/boxpilot-ipc/src/profile.rs` — add `ActivateBundleRequest`, `ActivateBundleResponse`, `RollbackRequest`, `ActivateOutcome`, `VerifySummary`.
- `crates/boxpilot-ipc/src/lib.rs` — re-export new types.
- `crates/boxpilot-profile/src/bundle.rs` — produce sealed memfd alongside staging dir.
- `crates/boxpilot-profile/src/lib.rs` — re-export memfd helper.
- `crates/boxpilot-tauri/src/lib.rs` — add `profile_activate` Tauri command.
- `crates/boxpilot-tauri/Cargo.toml` — add `boxpilot-profile`, `nix` deps if missing.

---

## Task 1: IPC types — request/response/outcome/error variants

**Files:**
- Modify: `crates/boxpilot-ipc/src/profile.rs`
- Modify: `crates/boxpilot-ipc/src/error.rs`
- Modify: `crates/boxpilot-ipc/src/config.rs`
- Modify: `crates/boxpilot-ipc/src/lib.rs`

- [ ] **Step 1: Add failing test for `ActivateBundleRequest` / `ActivateBundleResponse` round-trip**

Append to `crates/boxpilot-ipc/src/profile.rs` (inside `mod tests`):

```rust
#[test]
fn activate_request_round_trip() {
    let r = ActivateBundleRequest { verify_window_secs: Some(5), expected_total_bytes: Some(12345) };
    let s = serde_json::to_string(&r).unwrap();
    let back: ActivateBundleRequest = serde_json::from_str(&s).unwrap();
    assert_eq!(back, r);
}

#[test]
fn activate_request_defaults_when_fields_missing() {
    let r: ActivateBundleRequest = serde_json::from_str("{}").unwrap();
    assert_eq!(r.verify_window_secs, None);
    assert_eq!(r.expected_total_bytes, None);
}

#[test]
fn activate_outcome_serializes_snake_case() {
    assert_eq!(serde_json::to_string(&ActivateOutcome::Active).unwrap(), "\"active\"");
    assert_eq!(serde_json::to_string(&ActivateOutcome::RolledBack).unwrap(), "\"rolled_back\"");
    assert_eq!(
        serde_json::to_string(&ActivateOutcome::RollbackTargetMissing).unwrap(),
        "\"rollback_target_missing\""
    );
    assert_eq!(
        serde_json::to_string(&ActivateOutcome::RollbackUnstartable).unwrap(),
        "\"rollback_unstartable\""
    );
}

#[test]
fn activate_response_round_trip() {
    let r = ActivateBundleResponse {
        outcome: ActivateOutcome::Active,
        activation_id: "id-1".into(),
        previous_activation_id: Some("id-0".into()),
        verify: VerifySummary {
            window_used_ms: 4321,
            n_restarts_pre: 2,
            n_restarts_post: 2,
            final_unit_state: None,
        },
    };
    let s = serde_json::to_string(&r).unwrap();
    let back: ActivateBundleResponse = serde_json::from_str(&s).unwrap();
    assert_eq!(back, r);
}

#[test]
fn rollback_request_round_trip() {
    let r = RollbackRequest { target_activation_id: "id-0".into(), verify_window_secs: Some(5) };
    let s = serde_json::to_string(&r).unwrap();
    let back: RollbackRequest = serde_json::from_str(&s).unwrap();
    assert_eq!(back, r);
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p boxpilot-ipc`
Expected: compile error (unknown types).

- [ ] **Step 3: Add types to `crates/boxpilot-ipc/src/profile.rs`**

Append to the same file (above the existing `#[cfg(test)] mod tests`):

```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ActivateBundleRequest {
    /// 1..=30 seconds; `None` means take the daemon default (5 s).
    #[serde(default)]
    pub verify_window_secs: Option<u32>,
    /// Soft hint to short-circuit oversized bundles before mmap. Daemon
    /// still enforces hard `BUNDLE_MAX_TOTAL_BYTES` while walking.
    #[serde(default)]
    pub expected_total_bytes: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActivateOutcome {
    Active,
    RolledBack,
    RollbackTargetMissing,
    RollbackUnstartable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VerifySummary {
    pub window_used_ms: u64,
    pub n_restarts_pre: u32,
    pub n_restarts_post: u32,
    /// `None` when verify never read state (e.g. early failure path).
    #[serde(default)]
    pub final_unit_state: Option<crate::UnitState>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActivateBundleResponse {
    pub outcome: ActivateOutcome,
    pub activation_id: String,
    #[serde(default)]
    pub previous_activation_id: Option<String>,
    pub verify: VerifySummary,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RollbackRequest {
    pub target_activation_id: String,
    #[serde(default)]
    pub verify_window_secs: Option<u32>,
}
```

- [ ] **Step 4: Re-export from `crates/boxpilot-ipc/src/lib.rs`**

Find the existing `pub use profile::{...}` line and extend:

```rust
pub use profile::{
    ActivateBundleRequest, ActivateBundleResponse, ActivateOutcome, ActivationManifest,
    AssetEntry, RollbackRequest, SourceKind, VerifySummary, ACTIVATION_MANIFEST_SCHEMA_VERSION,
    BUNDLE_MAX_FILE_BYTES, BUNDLE_MAX_FILE_COUNT, BUNDLE_MAX_NESTING_DEPTH,
    BUNDLE_MAX_TOTAL_BYTES,
};
```

(If the existing re-export is wildcard-style, this step is a no-op; check `lib.rs` first.)

- [ ] **Step 5: Add failing test for new error variants in `crates/boxpilot-ipc/src/error.rs`**

Append inside `mod tests`:

```rust
#[test]
fn new_variants_round_trip() {
    use HelperError::*;
    for v in [
        BundleTooLarge { total: 100, limit: 50 },
        BundleEntryRejected { reason: "abs path".into() },
        BundleAssetMismatch { path: "geosite.db".into() },
        SingboxCheckFailed { exit: 1, stderr_tail: "bad rule".into() },
        ActivationVerifyStuck { final_state: format!("{:?}", crate::UnitState::NotFound) },
        RollbackTargetMissing,
        RollbackUnstartable { final_state: format!("{:?}", crate::UnitState::NotFound) },
        ActiveCorrupt,
        ReleaseAlreadyActive,
        ReleaseNotFound { activation_id: "id".into() },
    ] {
        let s = serde_json::to_string(&v).unwrap();
        let back: HelperError = serde_json::from_str(&s).unwrap();
        assert_eq!(back, v);
    }
}
```

- [ ] **Step 6: Add the variants to `HelperError`**

Insert after `Ipc { message: String },` in `crates/boxpilot-ipc/src/error.rs`:

```rust
    /// §9.2: total bundle size exceeded the cap.
    #[error("bundle exceeds total size {total} > {limit}")]
    BundleTooLarge { total: u64, limit: u64 },

    /// §9.2: a tar entry violated one of the structural rejection rules.
    #[error("bundle entry rejected: {reason}")]
    BundleEntryRejected { reason: String },

    /// §9.2: an asset's content sha256 did not match the manifest.
    #[error("asset {path} sha256 mismatch vs manifest")]
    BundleAssetMismatch { path: String },

    /// §10 step 7: `<core> check -c config.json` exited non-zero.
    #[error("sing-box check failed (exit {exit}): {stderr_tail}")]
    SingboxCheckFailed { exit: i32, stderr_tail: String },

    /// §7.2: the unit did not reach active/running within the window.
    #[error("activation verify stuck; final state {final_state}")]
    ActivationVerifyStuck { final_state: String },

    /// §10 step 14: rollback path entered but no previous release exists on disk.
    #[error("rollback target missing on disk")]
    RollbackTargetMissing,

    /// §10 step 15: rollback succeeded structurally but the previous release also fails to start.
    #[error("rollback target unstartable; final state {final_state}")]
    RollbackUnstartable { final_state: String },

    /// Daemon startup recovery flagged `/etc/boxpilot/active` as corrupt.
    #[error("/etc/boxpilot/active is corrupt; refusing activation")]
    ActiveCorrupt,

    /// Manual rollback target equals the current active release.
    #[error("requested release is already active")]
    ReleaseAlreadyActive,

    /// Manual rollback target is not present under `/etc/boxpilot/releases/`.
    #[error("release {activation_id} not found")]
    ReleaseNotFound { activation_id: String },
```

- [ ] **Step 7: Add failing test for `BoxpilotConfig` previous_* fields**

Append to `crates/boxpilot-ipc/src/config.rs` inside `mod tests`:

```rust
#[test]
fn parses_previous_release_fields() {
    let cfg = BoxpilotConfig::parse(
        "schema_version = 1\nprevious_release_id = \"id-0\"\nprevious_profile_id = \"p-0\"\nprevious_profile_sha256 = \"abc\"\nprevious_activated_at = \"2026-04-29T00:00:00-07:00\"\n",
    )
    .unwrap();
    assert_eq!(cfg.previous_release_id.as_deref(), Some("id-0"));
    assert_eq!(cfg.previous_profile_id.as_deref(), Some("p-0"));
    assert_eq!(cfg.previous_profile_sha256.as_deref(), Some("abc"));
    assert_eq!(cfg.previous_activated_at.as_deref(), Some("2026-04-29T00:00:00-07:00"));
}

#[test]
fn previous_fields_default_to_none() {
    let cfg = BoxpilotConfig::parse("schema_version = 1\n").unwrap();
    assert_eq!(cfg.previous_release_id, None);
    assert_eq!(cfg.previous_profile_id, None);
    assert_eq!(cfg.previous_profile_sha256, None);
    assert_eq!(cfg.previous_activated_at, None);
}
```

- [ ] **Step 8: Add `previous_*` fields to `BoxpilotConfig`**

Find the existing `pub activated_at: Option<String>,` in `crates/boxpilot-ipc/src/config.rs` and insert AFTER it:

```rust
    #[serde(default)]
    pub previous_release_id: Option<String>,
    #[serde(default)]
    pub previous_profile_id: Option<String>,
    #[serde(default)]
    pub previous_profile_sha256: Option<String>,
    #[serde(default)]
    pub previous_activated_at: Option<String>,
```

Then update `context::HelperContext::load_config` in `crates/boxpilotd/src/context.rs` — find the explicit `BoxpilotConfig { ... }` literal in the missing-file branch and add the four new fields with `None`.

- [ ] **Step 9: Run all boxpilot-ipc tests to verify pass**

Run: `cargo test -p boxpilot-ipc`
Expected: all green, +9 tests.

- [ ] **Step 10: Run boxpilotd tests to verify nothing regressed**

Run: `cargo test -p boxpilotd`
Expected: pre-existing 148+ tests still green.

- [ ] **Step 11: Commit**

```bash
git add crates/boxpilot-ipc crates/boxpilotd/src/context.rs
git commit -m "feat(plan-5): IPC types + error variants + boxpilot.toml previous_*

T1 of plan #5. Adds ActivateBundleRequest/Response, ActivateOutcome,
VerifySummary, RollbackRequest, ten new HelperError variants, and
optional previous_release_id/profile_id/sha256/activated_at on
BoxpilotConfig (schema unchanged — fields are #[serde(default)]).

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

## Task 2: Path extensions + `profile/` module skeleton

**Files:**
- Modify: `crates/boxpilotd/src/paths.rs`
- Create: `crates/boxpilotd/src/profile/mod.rs`
- Modify: `crates/boxpilotd/src/main.rs`

- [ ] **Step 1: Add failing test for new `Paths` methods**

Append to `mod tests` in `crates/boxpilotd/src/paths.rs`:

```rust
#[test]
fn release_paths_under_etc_boxpilot() {
    let p = Paths::with_root("/tmp/fake");
    assert_eq!(p.releases_dir(), PathBuf::from("/tmp/fake/etc/boxpilot/releases"));
    assert_eq!(p.staging_dir(), PathBuf::from("/tmp/fake/etc/boxpilot/.staging"));
    assert_eq!(p.active_symlink(), PathBuf::from("/tmp/fake/etc/boxpilot/active"));
    assert_eq!(
        p.release_dir("2026-04-30T00-00-00Z-abc"),
        PathBuf::from("/tmp/fake/etc/boxpilot/releases/2026-04-30T00-00-00Z-abc"),
    );
    assert_eq!(
        p.staging_subdir("2026-04-30T00-00-00Z-abc"),
        PathBuf::from("/tmp/fake/etc/boxpilot/.staging/2026-04-30T00-00-00Z-abc"),
    );
}
```

- [ ] **Step 2: Run, verify failure**

Run: `cargo test -p boxpilotd paths::tests`
Expected: compile error (methods undefined).

- [ ] **Step 3: Add the methods to `Paths`**

Insert in `crates/boxpilotd/src/paths.rs` between `polkit_controller_dropin_path` and the `#[cfg(test)]` block:

```rust
    pub fn releases_dir(&self) -> PathBuf {
        self.root.join("etc/boxpilot/releases")
    }

    pub fn staging_dir(&self) -> PathBuf {
        self.root.join("etc/boxpilot/.staging")
    }

    pub fn active_symlink(&self) -> PathBuf {
        self.root.join("etc/boxpilot/active")
    }

    pub fn release_dir(&self, activation_id: &str) -> PathBuf {
        self.releases_dir().join(activation_id)
    }

    pub fn staging_subdir(&self, activation_id: &str) -> PathBuf {
        self.staging_dir().join(activation_id)
    }
```

- [ ] **Step 4: Create `profile/mod.rs` with empty submodule list**

Write `crates/boxpilotd/src/profile/mod.rs`:

```rust
//! Plan #5: activation pipeline. Implements spec §9.2 (bundle transport
//! + safety filters), §10 (atomic rename + rollback), §7.2 (verify
//! window), and §13 startup-side drift hooks.
//!
//! Submodules are added by subsequent tasks of plan #5.
```

- [ ] **Step 5: Wire module into `main.rs`**

In `crates/boxpilotd/src/main.rs`, add `mod profile;` to the existing `mod` list (alphabetic order, between `paths` and `service`).

- [ ] **Step 6: Run tests**

Run: `cargo test -p boxpilotd paths`
Expected: pass.

Run: `cargo build -p boxpilotd`
Expected: compiles clean (empty module is fine).

- [ ] **Step 7: Commit**

```bash
git add crates/boxpilotd/src/paths.rs crates/boxpilotd/src/profile/mod.rs crates/boxpilotd/src/main.rs
git commit -m "feat(plan-5): paths::Paths releases/staging/active accessors + profile module

T2 of plan #5. Adds the path accessors the activation pipeline needs
and an empty profile/ module wired into boxpilotd's main mod list.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

## Task 3: `profile/unpack.rs` — sealed-memfd → tar walk → §9.2 enforcement

**Files:**
- Create: `crates/boxpilotd/src/profile/unpack.rs`
- Modify: `crates/boxpilotd/src/profile/mod.rs`

This is a long task; a single `unpack` function is easier to keep in
context than splitting and re-importing. Use TDD: write each rejection
test, watch it fail, add the matching guard, repeat.

- [ ] **Step 1: Declare module in `profile/mod.rs`**

Append:

```rust
pub mod unpack;
```

- [ ] **Step 2: Write skeleton + happy-path test**

Create `crates/boxpilotd/src/profile/unpack.rs`:

```rust
//! Spec §9.2: walk a tarball arriving as a passed file descriptor and
//! materialize it into `dest_dir` under strict structural rules. Every
//! rejected case maps to a `HelperError` variant introduced in plan #5
//! task 1. The unpacker NEVER follows symlinks; it refuses both symlink
//! and hardlink entries up-front.

use boxpilot_ipc::{
    ActivationManifest, HelperError, HelperResult, BUNDLE_MAX_FILE_BYTES, BUNDLE_MAX_FILE_COUNT,
    BUNDLE_MAX_NESTING_DEPTH, BUNDLE_MAX_TOTAL_BYTES,
};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom};
use std::os::fd::{AsRawFd, OwnedFd};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use tar::{Archive, EntryType};

/// Outcome of a successful unpack. `manifest` is parsed from
/// `manifest.json`; `bytes_written` is the running total enforced
/// against `BUNDLE_MAX_TOTAL_BYTES`.
#[derive(Debug)]
pub struct UnpackReport {
    pub manifest: ActivationManifest,
    pub bytes_written: u64,
    pub file_count: u32,
}

/// Read the full bundle out of `fd` and materialize into `dest_dir`,
/// which must NOT pre-exist. The directory is created with mode 0o700.
pub fn unpack_into(
    fd: OwnedFd,
    dest_dir: &Path,
    expected_total_bytes: Option<u64>,
) -> HelperResult<UnpackReport> {
    if dest_dir.exists() {
        return Err(HelperError::Ipc {
            message: format!("staging dest already exists: {}", dest_dir.display()),
        });
    }

    let mut file = File::from(fd);
    let total_size = file
        .seek(SeekFrom::End(0))
        .map_err(|e| HelperError::Ipc { message: format!("seek bundle fd: {e}") })?;
    file.seek(SeekFrom::Start(0))
        .map_err(|e| HelperError::Ipc { message: format!("rewind bundle fd: {e}") })?;
    if total_size > BUNDLE_MAX_TOTAL_BYTES {
        return Err(HelperError::BundleTooLarge { total: total_size, limit: BUNDLE_MAX_TOTAL_BYTES });
    }
    if let Some(hint) = expected_total_bytes {
        if hint != total_size {
            return Err(HelperError::Ipc {
                message: format!("expected_total_bytes {hint} != actual {total_size}"),
            });
        }
    }

    create_dir_0700(dest_dir)?;

    let mut archive = Archive::new(&mut file);
    archive.set_preserve_permissions(false);
    archive.set_preserve_mtime(false);

    let mut total_bytes: u64 = 0;
    let mut file_count: u32 = 0;
    let mut manifest_bytes: Option<Vec<u8>> = None;
    let mut on_disk_sha: BTreeMap<String, String> = BTreeMap::new();

    for entry in archive.entries().map_err(io_to_helper)? {
        let mut entry = entry.map_err(io_to_helper)?;
        let header_size = entry.header().size().map_err(io_to_helper)?;

        // Pre-body checks.
        let entry_path = entry.path().map_err(io_to_helper)?.into_owned();
        check_entry_path(&entry_path)?;
        let entry_type = entry.header().entry_type();
        if !is_allowed_entry(entry_type) {
            return Err(HelperError::BundleEntryRejected {
                reason: format!("unsupported entry type {:?} for {}", entry_type, entry_path.display()),
            });
        }
        if header_size > BUNDLE_MAX_FILE_BYTES {
            return Err(HelperError::BundleEntryRejected {
                reason: format!(
                    "{} exceeds per-file size {} > {}",
                    entry_path.display(),
                    header_size,
                    BUNDLE_MAX_FILE_BYTES
                ),
            });
        }
        let depth = entry_path.iter().count() as u32;
        if depth > BUNDLE_MAX_NESTING_DEPTH {
            return Err(HelperError::BundleEntryRejected {
                reason: format!("{} nesting depth {} > {}", entry_path.display(), depth, BUNDLE_MAX_NESTING_DEPTH),
            });
        }

        let dst = safe_join(dest_dir, &entry_path)?;

        match entry_type {
            EntryType::Directory => {
                create_dir_0700(&dst)?;
                continue;
            }
            EntryType::Regular => {
                if let Some(parent) = dst.parent() {
                    if parent != dest_dir && !parent.exists() {
                        create_dir_all_0700(parent, dest_dir)?;
                    }
                }
                file_count = file_count.saturating_add(1);
                if file_count > BUNDLE_MAX_FILE_COUNT {
                    return Err(HelperError::BundleEntryRejected {
                        reason: format!("file count {} > {}", file_count, BUNDLE_MAX_FILE_COUNT),
                    });
                }

                let mut buf = Vec::with_capacity(header_size as usize);
                entry.read_to_end(&mut buf).map_err(io_to_helper)?;
                let actual_size = buf.len() as u64;
                if actual_size > BUNDLE_MAX_FILE_BYTES {
                    return Err(HelperError::BundleEntryRejected {
                        reason: format!("{} body exceeds per-file size", entry_path.display()),
                    });
                }
                total_bytes = total_bytes.saturating_add(actual_size);
                if total_bytes > BUNDLE_MAX_TOTAL_BYTES {
                    return Err(HelperError::BundleTooLarge {
                        total: total_bytes,
                        limit: BUNDLE_MAX_TOTAL_BYTES,
                    });
                }

                if entry_path == Path::new("manifest.json") {
                    manifest_bytes = Some(buf.clone());
                }

                let mut h = Sha256::new();
                h.update(&buf);
                let sha = hex::encode(h.finalize());
                let key = relpath_string(&entry_path);
                on_disk_sha.insert(key, sha);

                let mut f = OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .mode(0o600)
                    .open(&dst)
                    .map_err(io_to_helper)?;
                std::io::Write::write_all(&mut f, &buf).map_err(io_to_helper)?;
            }
            _ => unreachable!("filtered above by is_allowed_entry"),
        }
    }

    let manifest_bytes = manifest_bytes.ok_or_else(|| HelperError::BundleEntryRejected {
        reason: "manifest.json missing from bundle".into(),
    })?;
    let manifest: ActivationManifest = serde_json::from_slice(&manifest_bytes).map_err(|e| {
        HelperError::BundleEntryRejected { reason: format!("manifest.json parse: {e}") }
    })?;
    if manifest.schema_version != boxpilot_ipc::ACTIVATION_MANIFEST_SCHEMA_VERSION {
        return Err(HelperError::UnsupportedSchemaVersion { got: manifest.schema_version });
    }

    // §9.2: every asset listed in the manifest must match the on-disk
    // sha after unpacking. This catches a hostile bundle where the
    // manifest claims clean assets but the tar body is poisoned, and
    // an honest bundle where prepare_bundle disagreed with itself.
    for asset in &manifest.assets {
        let key = format!("assets/{}", asset.path.trim_start_matches('/'));
        match on_disk_sha.get(&key) {
            Some(actual) if actual == &asset.sha256 => {}
            Some(_) => return Err(HelperError::BundleAssetMismatch { path: asset.path.clone() }),
            None => return Err(HelperError::BundleAssetMismatch { path: asset.path.clone() }),
        }
    }

    Ok(UnpackReport { manifest, bytes_written: total_bytes, file_count })
}

fn is_allowed_entry(t: EntryType) -> bool {
    matches!(t, EntryType::Regular | EntryType::Directory)
}

fn check_entry_path(p: &Path) -> HelperResult<()> {
    if p.is_absolute() {
        return Err(HelperError::BundleEntryRejected {
            reason: format!("absolute path: {}", p.display()),
        });
    }
    let s = p.to_string_lossy();
    for ch in s.chars() {
        // Refuse NUL, ASCII control, backslash, division/fullwidth slashes.
        if ch == '\0' || (ch.is_ascii_control()) || ch == '\\' || ch == '\u{2215}' || ch == '\u{FF0F}' {
            return Err(HelperError::BundleEntryRejected {
                reason: format!("forbidden character in path: {}", p.display()),
            });
        }
    }
    for comp in p.iter() {
        if comp.to_string_lossy() == ".." {
            return Err(HelperError::BundleEntryRejected {
                reason: format!("path traversal: {}", p.display()),
            });
        }
    }
    Ok(())
}

fn safe_join(root: &Path, rel: &Path) -> HelperResult<PathBuf> {
    let joined = root.join(rel);
    // Use lexical containment because `joined` doesn't exist yet for
    // most leaves. We've already rejected `..` at component level so
    // string-prefix containment of root is sound.
    let root_canon = root
        .canonicalize()
        .map_err(|e| HelperError::Ipc { message: format!("canonicalize root: {e}") })?;
    let joined_lex = lexical_normalize(&root_canon, &joined.strip_prefix(root).unwrap_or(rel));
    if !joined_lex.starts_with(&root_canon) {
        return Err(HelperError::BundleEntryRejected {
            reason: format!("escapes staging root: {}", rel.display()),
        });
    }
    Ok(joined_lex)
}

fn lexical_normalize(root: &Path, rel: &Path) -> PathBuf {
    let mut out = root.to_path_buf();
    for comp in rel.iter() {
        out.push(comp);
    }
    out
}

fn relpath_string(p: &Path) -> String {
    p.iter().map(|c| c.to_string_lossy().into_owned()).collect::<Vec<_>>().join("/")
}

fn create_dir_0700(p: &Path) -> HelperResult<()> {
    std::fs::DirBuilder::new()
        .mode(0o700)
        .recursive(false)
        .create(p)
        .map_err(io_to_helper)
}

fn create_dir_all_0700(p: &Path, root: &Path) -> HelperResult<()> {
    if p == root || p.exists() {
        return Ok(());
    }
    if let Some(parent) = p.parent() {
        if parent != root {
            create_dir_all_0700(parent, root)?;
        }
    }
    create_dir_0700(p)
}

fn io_to_helper(e: impl std::fmt::Display) -> HelperError {
    HelperError::Ipc { message: format!("unpack: {e}") }
}

#[cfg(test)]
mod tests;
```

- [ ] **Step 3: Create `crates/boxpilotd/src/profile/unpack/tests.rs` skeleton**

Wait — the `mod tests` is inline; create a sibling file by changing the line in step 2 to:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use boxpilot_ipc::{
        ActivationManifest, AssetEntry, SourceKind, ACTIVATION_MANIFEST_SCHEMA_VERSION,
    };
    use std::io::Write;
    use std::os::fd::AsFd;
    use tar::Header;
    use tempfile::tempdir;

    fn make_manifest(profile_id: &str, assets: Vec<AssetEntry>) -> Vec<u8> {
        let m = ActivationManifest {
            schema_version: ACTIVATION_MANIFEST_SCHEMA_VERSION,
            activation_id: "test-id".into(),
            profile_id: profile_id.into(),
            profile_sha256: "deadbeef".into(),
            config_sha256: "cafebabe".into(),
            source_kind: SourceKind::Local,
            source_url_redacted: None,
            core_path_at_activation: "/var/lib/boxpilot/cores/current/sing-box".into(),
            core_version_at_activation: "1.10.0".into(),
            created_at: "2026-04-30T00:00:00-07:00".into(),
            assets,
        };
        serde_json::to_vec_pretty(&m).unwrap()
    }

    /// Build a tar-in-memfd containing the listed (path, type, body) tuples.
    fn tar_memfd(entries: Vec<(&str, tar::EntryType, Vec<u8>)>) -> OwnedFd {
        let raw = nix::sys::memfd::memfd_create(
            std::ffi::CString::new("test-bundle").unwrap().as_c_str(),
            nix::sys::memfd::MemFdCreateFlag::MFD_CLOEXEC,
        )
        .expect("memfd_create");
        let fd: OwnedFd = raw;

        {
            let mut f = File::from(fd.try_clone().unwrap());
            let mut builder = tar::Builder::new(&mut f);
            for (path, ty, body) in &entries {
                let mut h = Header::new_ustar();
                h.set_size(body.len() as u64);
                h.set_entry_type(*ty);
                h.set_mode(0o600);
                h.set_cksum();
                builder.append_data(&mut h, path, body.as_slice()).unwrap();
            }
            builder.finish().unwrap();
        }
        fd
    }

    #[test]
    fn happy_path_unpacks_config_assets_and_manifest() {
        let manifest = make_manifest(
            "p",
            vec![AssetEntry {
                path: "geosite.db".into(),
                sha256: hex::encode(Sha256::digest(b"GEO")),
                size: 3,
            }],
        );
        let fd = tar_memfd(vec![
            ("config.json", tar::EntryType::Regular, br#"{"log":{}}"#.to_vec()),
            ("assets", tar::EntryType::Directory, Vec::new()),
            ("assets/geosite.db", tar::EntryType::Regular, b"GEO".to_vec()),
            ("manifest.json", tar::EntryType::Regular, manifest),
        ]);
        let dir = tempdir().unwrap();
        let dest = dir.path().join("staging-id");
        let report = unpack_into(fd, &dest, None).unwrap();
        assert!(dest.join("config.json").exists());
        assert!(dest.join("assets/geosite.db").exists());
        assert_eq!(report.file_count, 3);
        assert_eq!(report.bytes_written, 10 + 3 + report.bytes_written - 13);
    }

    #[test]
    fn refuses_absolute_path_entry() {
        let fd = tar_memfd(vec![
            ("/etc/passwd", tar::EntryType::Regular, b"x".to_vec()),
        ]);
        let dir = tempdir().unwrap();
        let dest = dir.path().join("s");
        let err = unpack_into(fd, &dest, None).unwrap_err();
        assert!(matches!(err, HelperError::BundleEntryRejected { .. }));
    }

    #[test]
    fn refuses_dotdot_traversal() {
        let fd = tar_memfd(vec![
            ("../escape.txt", tar::EntryType::Regular, b"x".to_vec()),
        ]);
        let dir = tempdir().unwrap();
        let dest = dir.path().join("s");
        let err = unpack_into(fd, &dest, None).unwrap_err();
        assert!(matches!(err, HelperError::BundleEntryRejected { .. }));
    }

    #[test]
    fn refuses_symlink_entry() {
        let fd = tar_memfd(vec![
            ("link", tar::EntryType::Symlink, b"".to_vec()),
        ]);
        let dir = tempdir().unwrap();
        let dest = dir.path().join("s");
        let err = unpack_into(fd, &dest, None).unwrap_err();
        assert!(matches!(err, HelperError::BundleEntryRejected { reason } if reason.contains("Symlink")));
    }

    #[test]
    fn refuses_hardlink_entry() {
        let fd = tar_memfd(vec![
            ("link", tar::EntryType::Link, b"".to_vec()),
        ]);
        let dir = tempdir().unwrap();
        let dest = dir.path().join("s");
        let err = unpack_into(fd, &dest, None).unwrap_err();
        assert!(matches!(err, HelperError::BundleEntryRejected { .. }));
    }

    #[test]
    fn refuses_fifo_entry() {
        let fd = tar_memfd(vec![("p", tar::EntryType::Fifo, b"".to_vec())]);
        let dir = tempdir().unwrap();
        let dest = dir.path().join("s");
        assert!(matches!(
            unpack_into(fd, &dest, None).unwrap_err(),
            HelperError::BundleEntryRejected { .. }
        ));
    }

    #[test]
    fn refuses_char_device_entry() {
        let fd = tar_memfd(vec![("c", tar::EntryType::Char, b"".to_vec())]);
        let dir = tempdir().unwrap();
        let dest = dir.path().join("s");
        assert!(matches!(
            unpack_into(fd, &dest, None).unwrap_err(),
            HelperError::BundleEntryRejected { .. }
        ));
    }

    #[test]
    fn refuses_block_device_entry() {
        let fd = tar_memfd(vec![("b", tar::EntryType::Block, b"".to_vec())]);
        let dir = tempdir().unwrap();
        let dest = dir.path().join("s");
        assert!(matches!(
            unpack_into(fd, &dest, None).unwrap_err(),
            HelperError::BundleEntryRejected { .. }
        ));
    }

    #[test]
    fn refuses_path_with_backslash() {
        let fd = tar_memfd(vec![("a\\b", tar::EntryType::Regular, b"x".to_vec())]);
        let dir = tempdir().unwrap();
        let dest = dir.path().join("s");
        assert!(matches!(
            unpack_into(fd, &dest, None).unwrap_err(),
            HelperError::BundleEntryRejected { .. }
        ));
    }

    #[test]
    fn refuses_path_with_division_slash() {
        let fd = tar_memfd(vec![("a\u{2215}b", tar::EntryType::Regular, b"x".to_vec())]);
        let dir = tempdir().unwrap();
        let dest = dir.path().join("s");
        assert!(matches!(
            unpack_into(fd, &dest, None).unwrap_err(),
            HelperError::BundleEntryRejected { .. }
        ));
    }

    #[test]
    fn refuses_path_with_fullwidth_solidus() {
        let fd = tar_memfd(vec![("a\u{FF0F}b", tar::EntryType::Regular, b"x".to_vec())]);
        let dir = tempdir().unwrap();
        let dest = dir.path().join("s");
        assert!(matches!(
            unpack_into(fd, &dest, None).unwrap_err(),
            HelperError::BundleEntryRejected { .. }
        ));
    }

    #[test]
    fn rejects_too_large_file() {
        let big = vec![0u8; (BUNDLE_MAX_FILE_BYTES + 1) as usize];
        // Build via a Vec rather than memfd — same effect, less RAM peak risk.
        // We still need an OwnedFd; use a real memfd.
        let fd = tar_memfd(vec![("config.json", tar::EntryType::Regular, big)]);
        let dir = tempdir().unwrap();
        let dest = dir.path().join("s");
        assert!(matches!(
            unpack_into(fd, &dest, None).unwrap_err(),
            HelperError::BundleEntryRejected { .. } | HelperError::BundleTooLarge { .. }
        ));
    }

    #[test]
    fn rejects_too_many_files() {
        let mut entries: Vec<(&str, tar::EntryType, Vec<u8>)> = Vec::new();
        let names: Vec<String> = (0..(BUNDLE_MAX_FILE_COUNT + 1))
            .map(|i| format!("f{i}.txt"))
            .collect();
        for n in &names {
            entries.push((n.as_str(), tar::EntryType::Regular, b"x".to_vec()));
        }
        let fd = tar_memfd(entries);
        let dir = tempdir().unwrap();
        let dest = dir.path().join("s");
        let err = unpack_into(fd, &dest, None).unwrap_err();
        assert!(matches!(err, HelperError::BundleEntryRejected { .. }));
    }

    #[test]
    fn rejects_too_deep_nesting() {
        let depth = (BUNDLE_MAX_NESTING_DEPTH + 1) as usize;
        let path: String = std::iter::repeat("d").take(depth).collect::<Vec<_>>().join("/")
            + "/leaf.txt";
        let fd = tar_memfd(vec![(path.as_str(), tar::EntryType::Regular, b"x".to_vec())]);
        let dir = tempdir().unwrap();
        let dest = dir.path().join("s");
        let err = unpack_into(fd, &dest, None).unwrap_err();
        assert!(matches!(err, HelperError::BundleEntryRejected { .. }));
    }

    #[test]
    fn rejects_bundle_total_over_limit() {
        // Approximate: one entry whose body is total cap + 1.
        // Build with shrinking body since unit tests must not OOM.
        // Strategy: shave to (BUNDLE_MAX_TOTAL_BYTES / 2 + 1) twice.
        let half = (BUNDLE_MAX_TOTAL_BYTES as usize) / 2 + 1;
        let body = vec![0u8; half];
        let manifest = make_manifest("p", vec![]);
        let fd = tar_memfd(vec![
            ("config.json", tar::EntryType::Regular, body.clone()),
            ("assets", tar::EntryType::Directory, Vec::new()),
            ("assets/big.bin", tar::EntryType::Regular, body),
            ("manifest.json", tar::EntryType::Regular, manifest),
        ]);
        let dir = tempdir().unwrap();
        let dest = dir.path().join("s");
        let err = unpack_into(fd, &dest, None).unwrap_err();
        assert!(matches!(err, HelperError::BundleTooLarge { .. }));
    }

    #[test]
    fn rejects_missing_manifest() {
        let fd = tar_memfd(vec![
            ("config.json", tar::EntryType::Regular, b"{}".to_vec()),
        ]);
        let dir = tempdir().unwrap();
        let dest = dir.path().join("s");
        let err = unpack_into(fd, &dest, None).unwrap_err();
        assert!(matches!(err, HelperError::BundleEntryRejected { reason } if reason.contains("manifest")));
    }

    #[test]
    fn rejects_manifest_asset_sha_mismatch() {
        let manifest = make_manifest(
            "p",
            vec![AssetEntry {
                path: "geosite.db".into(),
                sha256: "0000".into(), // wrong
                size: 3,
            }],
        );
        let fd = tar_memfd(vec![
            ("config.json", tar::EntryType::Regular, b"{}".to_vec()),
            ("assets", tar::EntryType::Directory, Vec::new()),
            ("assets/geosite.db", tar::EntryType::Regular, b"GEO".to_vec()),
            ("manifest.json", tar::EntryType::Regular, manifest),
        ]);
        let dir = tempdir().unwrap();
        let dest = dir.path().join("s");
        let err = unpack_into(fd, &dest, None).unwrap_err();
        assert!(matches!(err, HelperError::BundleAssetMismatch { .. }));
    }

    #[test]
    fn rejects_manifest_unknown_schema_version() {
        let mut m_bytes = make_manifest("p", vec![]);
        // Patch schema_version to 99.
        let s = String::from_utf8(m_bytes).unwrap().replace("\"schema_version\": 1", "\"schema_version\": 99");
        m_bytes = s.into_bytes();
        let fd = tar_memfd(vec![
            ("config.json", tar::EntryType::Regular, b"{}".to_vec()),
            ("manifest.json", tar::EntryType::Regular, m_bytes),
        ]);
        let dir = tempdir().unwrap();
        let dest = dir.path().join("s");
        let err = unpack_into(fd, &dest, None).unwrap_err();
        assert!(matches!(err, HelperError::UnsupportedSchemaVersion { got: 99 }));
    }

    #[test]
    fn rejects_when_dest_exists() {
        let fd = tar_memfd(vec![]);
        let dir = tempdir().unwrap();
        let dest = dir.path().join("already-here");
        std::fs::create_dir(&dest).unwrap();
        let err = unpack_into(fd, &dest, None).unwrap_err();
        assert!(matches!(err, HelperError::Ipc { .. }));
    }

    #[test]
    fn expected_total_bytes_mismatch_aborts_early() {
        let fd = tar_memfd(vec![
            ("config.json", tar::EntryType::Regular, b"{}".to_vec()),
            ("manifest.json", tar::EntryType::Regular, make_manifest("p", vec![])),
        ]);
        // Get actual size by stat.
        let actual = nix::sys::stat::fstat(fd.as_raw_fd()).unwrap().st_size as u64;
        let dir = tempdir().unwrap();
        let dest = dir.path().join("s");
        let err = unpack_into(fd, &dest, Some(actual + 1)).unwrap_err();
        assert!(matches!(err, HelperError::Ipc { .. }));
    }

    #[test]
    fn happy_path_creates_dest_with_0700() {
        let fd = tar_memfd(vec![
            ("config.json", tar::EntryType::Regular, b"{}".to_vec()),
            ("manifest.json", tar::EntryType::Regular, make_manifest("p", vec![])),
        ]);
        let dir = tempdir().unwrap();
        let dest = dir.path().join("s");
        unpack_into(fd, &dest, None).unwrap();
        let mode = std::fs::metadata(&dest).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o700);
    }

    #[test]
    fn unpacked_files_are_0600() {
        let fd = tar_memfd(vec![
            ("config.json", tar::EntryType::Regular, b"{}".to_vec()),
            ("manifest.json", tar::EntryType::Regular, make_manifest("p", vec![])),
        ]);
        let dir = tempdir().unwrap();
        let dest = dir.path().join("s");
        unpack_into(fd, &dest, None).unwrap();
        let mode = std::fs::metadata(dest.join("config.json")).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }
}
```

Note: the `use std::os::unix::fs::PermissionsExt as _;` and `use std::os::fd::AsRawFd as _;` imports may be needed depending on where you place the test module. Add them at the top of `mod tests` if compile complains.

- [ ] **Step 4: Run all unpack tests, verify pass**

Run: `cargo test -p boxpilotd profile::unpack -- --nocapture`
Expected: 20 tests pass.

If `Symlink` reason match fails, change the assertion to `reason.contains("Symlink") || reason.contains("symlink")`.

- [ ] **Step 5: Commit**

```bash
git add crates/boxpilotd/src/profile/
git commit -m "feat(plan-5): bundle unpacker enforcing all §9.2 rejection rules

T3 of plan #5. profile::unpack::unpack_into reads a sealed memfd of
plain tar, walks entries with strict structural filters (absolute /
traversal / symlink / hardlink / device / fifo / unicode aliasing /
size / count / depth caps), and writes 0600 files into a 0700 staging
dir. Manifest sha256 must match every assets/* it lists.

20 unit tests cover the happy path plus every rejection vector from
spec §9.2.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

## Task 4: `profile/release.rs` — atomic symlink swap + rename helpers

**Files:**
- Create: `crates/boxpilotd/src/profile/release.rs`
- Modify: `crates/boxpilotd/src/profile/mod.rs`

- [ ] **Step 1: Declare module**

In `profile/mod.rs` add:

```rust
pub mod release;
```

- [ ] **Step 2: Write tests**

Create `crates/boxpilotd/src/profile/release.rs`:

```rust
//! Spec §10 step 8 (`rename(2)` staging→releases) and step 9 (atomic
//! symlink swap of `/etc/boxpilot/active` via `rename(2)` on
//! `active.new`). The `ln -sfn` path is intentionally not used — it
//! unlinks first, leaving a window where `active` does not exist.

use boxpilot_ipc::{HelperError, HelperResult};
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};

/// Move staging into releases. Both paths must live on the same
/// filesystem (they do: `/etc/boxpilot` is one mount in production).
pub fn promote_staging(staging: &Path, target: &Path) -> HelperResult<()> {
    if !staging.exists() {
        return Err(HelperError::Ipc {
            message: format!("staging {} missing", staging.display()),
        });
    }
    if target.exists() {
        return Err(HelperError::Ipc {
            message: format!("release dir {} already exists", target.display()),
        });
    }
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent).map_err(|e| HelperError::Ipc {
            message: format!("mkdir releases parent: {e}"),
        })?;
    }
    std::fs::rename(staging, target).map_err(|e| HelperError::Ipc {
        message: format!("promote {} -> {}: {e}", staging.display(), target.display()),
    })?;
    Ok(())
}

/// Atomic symlink replace via `rename(2)` on `active.new`. The kernel
/// guarantees `active` resolves at every instant.
pub fn swap_active_symlink(active: &Path, new_target: &Path) -> HelperResult<()> {
    let new_link = active.with_extension("new");
    // Remove any leftover `active.new` from a crashed prior run.
    if new_link.exists() || new_link.symlink_metadata().is_ok() {
        std::fs::remove_file(&new_link).map_err(|e| HelperError::Ipc {
            message: format!("remove stale active.new: {e}"),
        })?;
    }
    symlink(new_target, &new_link).map_err(|e| HelperError::Ipc {
        message: format!("create active.new -> {}: {e}", new_target.display()),
    })?;
    std::fs::rename(&new_link, active).map_err(|e| HelperError::Ipc {
        message: format!("rename active.new -> active: {e}"),
    })?;
    Ok(())
}

/// Resolve `active` to its target. Returns `None` when `active` is
/// missing, dangling, or not a symlink.
pub fn read_active_target(active: &Path) -> Option<PathBuf> {
    let target = std::fs::read_link(active).ok()?;
    let resolved = if target.is_absolute() {
        target
    } else {
        active.parent()?.join(target)
    };
    if resolved.exists() {
        Some(resolved)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn promote_staging_moves_dir_atomically() {
        let dir = tempdir().unwrap();
        let staging = dir.path().join(".staging/abc");
        std::fs::create_dir_all(&staging).unwrap();
        std::fs::write(staging.join("config.json"), b"{}").unwrap();
        let target = dir.path().join("releases/abc");
        promote_staging(&staging, &target).unwrap();
        assert!(target.join("config.json").exists());
        assert!(!staging.exists());
    }

    #[test]
    fn promote_staging_refuses_existing_target() {
        let dir = tempdir().unwrap();
        let staging = dir.path().join(".staging/abc");
        std::fs::create_dir_all(&staging).unwrap();
        let target = dir.path().join("releases/abc");
        std::fs::create_dir_all(&target).unwrap();
        assert!(promote_staging(&staging, &target).is_err());
    }

    #[test]
    fn swap_active_symlink_creates_then_replaces() {
        let dir = tempdir().unwrap();
        let active = dir.path().join("active");
        let r1 = dir.path().join("releases/r1");
        let r2 = dir.path().join("releases/r2");
        std::fs::create_dir_all(&r1).unwrap();
        std::fs::create_dir_all(&r2).unwrap();
        swap_active_symlink(&active, &r1).unwrap();
        assert_eq!(std::fs::read_link(&active).unwrap(), r1);
        swap_active_symlink(&active, &r2).unwrap();
        assert_eq!(std::fs::read_link(&active).unwrap(), r2);
    }

    #[test]
    fn swap_active_symlink_clears_stale_active_new() {
        let dir = tempdir().unwrap();
        let active = dir.path().join("active");
        let stale = dir.path().join("active.new");
        let r1 = dir.path().join("r1");
        std::fs::create_dir_all(&r1).unwrap();
        // Plant a stale leftover from a crashed prior run.
        std::os::unix::fs::symlink(&r1, &stale).unwrap();
        swap_active_symlink(&active, &r1).unwrap();
        assert!(!stale.exists());
        assert_eq!(std::fs::read_link(&active).unwrap(), r1);
    }

    #[test]
    fn read_active_target_returns_resolved_dir() {
        let dir = tempdir().unwrap();
        let active = dir.path().join("active");
        let r = dir.path().join("r");
        std::fs::create_dir_all(&r).unwrap();
        std::os::unix::fs::symlink(&r, &active).unwrap();
        assert_eq!(read_active_target(&active), Some(r));
    }

    #[test]
    fn read_active_target_returns_none_when_dangling() {
        let dir = tempdir().unwrap();
        let active = dir.path().join("active");
        std::os::unix::fs::symlink(dir.path().join("nope"), &active).unwrap();
        assert_eq!(read_active_target(&active), None);
    }

    #[test]
    fn read_active_target_returns_none_when_missing() {
        let dir = tempdir().unwrap();
        let active = dir.path().join("active");
        assert_eq!(read_active_target(&active), None);
    }
}
```

- [ ] **Step 3: Run tests**

Run: `cargo test -p boxpilotd profile::release`
Expected: 6 tests pass.

- [ ] **Step 4: Commit**

```bash
git add crates/boxpilotd/src/profile/
git commit -m "feat(plan-5): release rename + atomic active-symlink swap

T4 of plan #5. profile::release exposes promote_staging (rename(2)
.staging/<id> -> releases/<id>), swap_active_symlink (rename(2) on
active.new — never ln -sfn, never an unlink-then-symlink window), and
read_active_target. 6 tests including stale active.new cleanup.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

## Task 5: `profile/recovery.rs` — startup `.staging/` sweep + active validation

**Files:**
- Create: `crates/boxpilotd/src/profile/recovery.rs`
- Modify: `crates/boxpilotd/src/profile/mod.rs`
- Modify: `crates/boxpilotd/src/main.rs`

- [ ] **Step 1: Declare module**

`profile/mod.rs`:

```rust
pub mod recovery;
```

- [ ] **Step 2: Write tests + impl**

Create `crates/boxpilotd/src/profile/recovery.rs`:

```rust
//! Spec §10 crash recovery. On every daemon startup, before binding
//! the D-Bus interface, sweep `.staging/*` (always invalid mid-call)
//! and validate `active` resolves under `releases/`.

use crate::paths::Paths;
use std::path::{Path, PathBuf};
use tracing::{info, warn};

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RecoveryReport {
    pub staging_dirs_swept: u32,
    pub active_corrupt: bool,
    pub active_target: Option<PathBuf>,
}

pub async fn reconcile(paths: &Paths) -> RecoveryReport {
    let mut report = RecoveryReport::default();

    let staging = paths.staging_dir();
    if staging.exists() {
        match tokio::fs::read_dir(&staging).await {
            Ok(mut entries) => loop {
                match entries.next_entry().await {
                    Ok(Some(e)) => {
                        let p = e.path();
                        match tokio::fs::remove_dir_all(&p).await {
                            Ok(()) => {
                                report.staging_dirs_swept = report.staging_dirs_swept.saturating_add(1);
                                info!(path = %p.display(), "swept stale activation staging dir");
                            }
                            Err(e) => warn!(path = %p.display(), "stage sweep failed: {e}"),
                        }
                    }
                    Ok(None) => break,
                    Err(e) => {
                        warn!("read_dir entry: {e}");
                        break;
                    }
                }
            },
            Err(e) => warn!("read_dir staging: {e}"),
        }
    }

    let active = paths.active_symlink();
    if active.symlink_metadata().is_ok() {
        match tokio::fs::read_link(&active).await {
            Ok(target) => {
                let resolved = if target.is_absolute() {
                    target
                } else {
                    active.parent().unwrap_or(Path::new("/")).join(target)
                };
                let resolves_under_releases = resolved.starts_with(paths.releases_dir());
                let target_exists = tokio::fs::metadata(&resolved).await.is_ok();
                if resolves_under_releases && target_exists {
                    report.active_target = Some(resolved);
                } else {
                    warn!(target = %resolved.display(), "active symlink corrupt");
                    report.active_corrupt = true;
                }
            }
            Err(e) => {
                warn!("read_link active: {e}");
                report.active_corrupt = true;
            }
        }
    }
    report
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn no_staging_dir_means_zero_sweeps() {
        let tmp = tempdir().unwrap();
        let paths = Paths::with_root(tmp.path());
        let r = reconcile(&paths).await;
        assert_eq!(r.staging_dirs_swept, 0);
        assert!(!r.active_corrupt);
    }

    #[tokio::test]
    async fn sweeps_stale_staging_subdirs() {
        let tmp = tempdir().unwrap();
        let paths = Paths::with_root(tmp.path());
        std::fs::create_dir_all(paths.staging_subdir("old1")).unwrap();
        std::fs::create_dir_all(paths.staging_subdir("old2")).unwrap();
        let r = reconcile(&paths).await;
        assert_eq!(r.staging_dirs_swept, 2);
        assert!(!paths.staging_subdir("old1").exists());
    }

    #[tokio::test]
    async fn active_pointing_under_releases_is_ok() {
        let tmp = tempdir().unwrap();
        let paths = Paths::with_root(tmp.path());
        let r1 = paths.release_dir("r1");
        std::fs::create_dir_all(&r1).unwrap();
        std::fs::create_dir_all(paths.active_symlink().parent().unwrap()).unwrap();
        std::os::unix::fs::symlink(&r1, paths.active_symlink()).unwrap();
        let r = reconcile(&paths).await;
        assert!(!r.active_corrupt);
        assert_eq!(r.active_target.as_deref(), Some(r1.as_path()));
    }

    #[tokio::test]
    async fn active_pointing_outside_releases_is_corrupt() {
        let tmp = tempdir().unwrap();
        let paths = Paths::with_root(tmp.path());
        let elsewhere = tmp.path().join("elsewhere");
        std::fs::create_dir_all(&elsewhere).unwrap();
        std::fs::create_dir_all(paths.active_symlink().parent().unwrap()).unwrap();
        std::os::unix::fs::symlink(&elsewhere, paths.active_symlink()).unwrap();
        let r = reconcile(&paths).await;
        assert!(r.active_corrupt);
    }

    #[tokio::test]
    async fn active_pointing_at_missing_target_is_corrupt() {
        let tmp = tempdir().unwrap();
        let paths = Paths::with_root(tmp.path());
        let missing = paths.release_dir("ghost");
        std::fs::create_dir_all(paths.active_symlink().parent().unwrap()).unwrap();
        std::os::unix::fs::symlink(&missing, paths.active_symlink()).unwrap();
        let r = reconcile(&paths).await;
        assert!(r.active_corrupt);
    }
}
```

- [ ] **Step 3: Wire into `main.rs`**

Find the existing `run_startup_recovery` function in `crates/boxpilotd/src/main.rs` and add a call to `profile::recovery::reconcile` AFTER the polkit drop-in backfill block, BEFORE returning Ok:

```rust
    let activation_recovery = crate::profile::recovery::reconcile(paths).await;
    if activation_recovery.staging_dirs_swept > 0 {
        info!(
            count = activation_recovery.staging_dirs_swept,
            "swept stale activation .staging entries"
        );
    }
    if activation_recovery.active_corrupt {
        warn!("/etc/boxpilot/active is corrupt; activation/rollback will refuse until repaired");
    }
```

The `RecoveryReport` is logged here; it does not need to flow into HelperContext for plan #5 — the activation handler re-checks `active` corruption itself before each operation (cheaper than threading state).

- [ ] **Step 4: Run tests**

Run: `cargo test -p boxpilotd profile::recovery`
Expected: 5 tests pass.

Run: `cargo build -p boxpilotd`
Expected: clean.

- [ ] **Step 5: Commit**

```bash
git add crates/boxpilotd/src/profile/recovery.rs crates/boxpilotd/src/profile/mod.rs crates/boxpilotd/src/main.rs
git commit -m "feat(plan-5): startup .staging/ sweep + active symlink validation

T5 of plan #5. profile::recovery::reconcile runs before D-Bus binding
on every boxpilotd start: sweeps every .staging/<id> subdir (always
invalid mid-call), then verifies /etc/boxpilot/active resolves under
/etc/boxpilot/releases. Logs warning when active is corrupt; does not
prevent startup.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

## Task 6: `profile/verifier.rs` — `ServiceVerifier` trait + drop dead_code

**Files:**
- Create: `crates/boxpilotd/src/profile/verifier.rs`
- Modify: `crates/boxpilotd/src/service/verify.rs`
- Modify: `crates/boxpilotd/src/profile/mod.rs`

- [ ] **Step 1: Declare module**

`profile/mod.rs`:

```rust
pub mod verifier;
```

- [ ] **Step 2: Write trait + production impl + tests**

Create `crates/boxpilotd/src/profile/verifier.rs`:

```rust
//! Indirection over `service::verify::wait_for_running`. Plan #5
//! introduces this trait so `activate.rs` can be unit-tested with a
//! deterministic verifier instead of polling real systemd.

use crate::service::verify::{self, VerifyOutcome};
use crate::systemd::Systemd;
use async_trait::async_trait;
use boxpilot_ipc::HelperResult;
use std::time::Duration;

#[async_trait]
pub trait ServiceVerifier: Send + Sync {
    async fn wait_for_running(
        &self,
        unit_name: &str,
        pre_n_restarts: u32,
        window: Duration,
        systemd: &dyn Systemd,
    ) -> HelperResult<VerifyOutcome>;
}

pub struct DefaultVerifier;

#[async_trait]
impl ServiceVerifier for DefaultVerifier {
    async fn wait_for_running(
        &self,
        unit_name: &str,
        pre_n_restarts: u32,
        window: Duration,
        systemd: &dyn Systemd,
    ) -> HelperResult<VerifyOutcome> {
        verify::wait_for_running(unit_name, pre_n_restarts, window, systemd).await
    }
}

#[cfg(test)]
pub mod testing {
    use super::*;
    use std::sync::Mutex;

    pub struct ScriptedVerifier {
        pub answers: Mutex<Vec<VerifyOutcome>>,
    }

    impl ScriptedVerifier {
        pub fn new(answers: Vec<VerifyOutcome>) -> Self {
            Self { answers: Mutex::new(answers) }
        }
    }

    #[async_trait]
    impl ServiceVerifier for ScriptedVerifier {
        async fn wait_for_running(
            &self,
            _unit_name: &str,
            _pre_n_restarts: u32,
            _window: Duration,
            _systemd: &dyn Systemd,
        ) -> HelperResult<VerifyOutcome> {
            let mut g = self.answers.lock().unwrap();
            assert!(!g.is_empty(), "ScriptedVerifier exhausted");
            Ok(g.remove(0))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::testing::ScriptedVerifier;
    use super::*;

    #[tokio::test]
    async fn scripted_returns_in_order() {
        let v = ScriptedVerifier::new(vec![
            VerifyOutcome::Running,
            VerifyOutcome::Stuck { final_state: boxpilot_ipc::UnitState::NotFound },
        ]);
        let s = crate::systemd::testing::FixedSystemd { answer: boxpilot_ipc::UnitState::NotFound };
        let r1 = v.wait_for_running("u", 0, Duration::from_millis(1), &s).await.unwrap();
        assert_eq!(r1, VerifyOutcome::Running);
        let r2 = v.wait_for_running("u", 0, Duration::from_millis(1), &s).await.unwrap();
        assert!(matches!(r2, VerifyOutcome::Stuck { .. }));
    }
}
```

- [ ] **Step 3: Drop `#[allow(dead_code)]` on verify**

In `crates/boxpilotd/src/service/verify.rs`, remove the four `#[allow(dead_code)]` annotations on `DEFAULT_WINDOW`, `MAX_WINDOW`, `POLL_INTERVAL`, the `VerifyOutcome` enum, and `wait_for_running`. Also remove the inline comment `// Plan #3 ships this helper; plan #5 wires it…` since plan #5 now does.

- [ ] **Step 4: Run tests**

Run: `cargo test -p boxpilotd profile::verifier service::verify`
Expected: pre-existing 4 verify tests + 1 new = 5 pass; no `dead_code` warnings.

Run: `cargo clippy -p boxpilotd --all-targets -- -D warnings`
Expected: clean.

- [ ] **Step 5: Commit**

```bash
git add crates/boxpilotd/src/profile/ crates/boxpilotd/src/service/verify.rs
git commit -m "feat(plan-5): ServiceVerifier trait + retire verify dead_code

T6 of plan #5. profile::verifier wraps service::verify::wait_for_running
behind a trait so activate.rs can be tested with ScriptedVerifier.
Removes the #[allow(dead_code)] annotations plan #3 left on verify.rs.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

## Task 7: `profile/checker.rs` — `SingboxChecker` trait + process impl

**Files:**
- Create: `crates/boxpilotd/src/profile/checker.rs`
- Modify: `crates/boxpilotd/src/profile/mod.rs`

- [ ] **Step 1: Declare module**

`profile/mod.rs`:

```rust
pub mod checker;
```

- [ ] **Step 2: Write trait + impl + tests**

Create `crates/boxpilotd/src/profile/checker.rs`:

```rust
//! Spec §10 step 7: `<core_path> check -c config.json` from inside the
//! release working directory. Trait-wrapped so activate.rs can run a
//! deterministic checker in unit tests.

use async_trait::async_trait;
use boxpilot_ipc::{HelperError, HelperResult};
use std::path::Path;
use tokio::process::Command;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckOutcome {
    pub exit: i32,
    pub stderr_tail: String,
}

#[async_trait]
pub trait SingboxChecker: Send + Sync {
    async fn check(&self, core_path: &Path, working_dir: &Path) -> HelperResult<CheckOutcome>;
}

pub struct ProcessChecker;

#[async_trait]
impl SingboxChecker for ProcessChecker {
    async fn check(&self, core_path: &Path, working_dir: &Path) -> HelperResult<CheckOutcome> {
        let output = Command::new(core_path)
            .arg("check")
            .arg("-c")
            .arg("config.json")
            .current_dir(working_dir)
            .output()
            .await
            .map_err(|e| HelperError::SingboxCheckFailed { exit: -1, stderr_tail: format!("spawn: {e}") })?;
        let exit = output.status.code().unwrap_or(-1);
        let stderr = String::from_utf8_lossy(&output.stderr);
        let tail: String = stderr.chars().rev().take(256).collect::<String>().chars().rev().collect();
        Ok(CheckOutcome { exit, stderr_tail: redact_secrets(&tail) })
    }
}

/// Best-effort scrub of the stderr tail before we hand it back to the
/// caller. Plan #8 will replace this with §14 schema-aware redaction;
/// for now any line containing one of the known sensitive substrings
/// is dropped wholesale.
fn redact_secrets(s: &str) -> String {
    s.lines()
        .filter(|line| {
            let lower = line.to_ascii_lowercase();
            !(lower.contains("password")
                || lower.contains("uuid")
                || lower.contains("private_key"))
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
pub mod testing {
    use super::*;
    use std::sync::Mutex;

    pub struct FakeChecker {
        outcomes: Mutex<Vec<CheckOutcome>>,
    }

    impl FakeChecker {
        pub fn new(outcomes: Vec<CheckOutcome>) -> Self {
            Self { outcomes: Mutex::new(outcomes) }
        }
        pub fn ok() -> Self {
            Self::new(vec![CheckOutcome { exit: 0, stderr_tail: String::new() }])
        }
        pub fn fail() -> Self {
            Self::new(vec![CheckOutcome { exit: 1, stderr_tail: "bad rule".into() }])
        }
    }

    #[async_trait]
    impl SingboxChecker for FakeChecker {
        async fn check(&self, _core: &Path, _wd: &Path) -> HelperResult<CheckOutcome> {
            let mut g = self.outcomes.lock().unwrap();
            assert!(!g.is_empty(), "FakeChecker exhausted");
            Ok(g.remove(0))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redact_drops_password_lines() {
        let s = "ok line 1\npassword=hunter2\nok line 2";
        assert_eq!(redact_secrets(s), "ok line 1\nok line 2");
    }

    #[test]
    fn redact_drops_uuid_lines() {
        let s = "uuid=abc-def\ngood";
        assert_eq!(redact_secrets(s), "good");
    }

    #[test]
    fn redact_drops_private_key_lines() {
        let s = "private_key:foo\nstuff";
        assert_eq!(redact_secrets(s), "stuff");
    }
}
```

- [ ] **Step 3: Run tests**

Run: `cargo test -p boxpilotd profile::checker`
Expected: 3 tests pass.

- [ ] **Step 4: Commit**

```bash
git add crates/boxpilotd/src/profile/
git commit -m "feat(plan-5): SingboxChecker trait + ProcessChecker impl

T7 of plan #5. profile::checker exposes \`SingboxChecker\` so
activate.rs can run a FakeChecker in unit tests; ProcessChecker shells
out to \`<core> check -c config.json\` from the release working dir
and tail-trims+redacts stderr for the wire surface.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

## Task 8: `profile/gc.rs` — release retention policy

**Files:**
- Create: `crates/boxpilotd/src/profile/gc.rs`
- Modify: `crates/boxpilotd/src/profile/mod.rs`

- [ ] **Step 1: Declare module**

`profile/mod.rs`:

```rust
pub mod gc;
```

- [ ] **Step 2: Tests + impl**

Create `crates/boxpilotd/src/profile/gc.rs`:

```rust
//! Spec §10 retention policy:
//!  - always keep `active`
//!  - always keep `previous`
//!  - keep ≤10 most recent AND total ≤ 2 GiB; whichever bound hits first wins
//!  - delete oldest first
//!  - skip the active and previous targets always

use crate::paths::Paths;
use std::path::PathBuf;
use std::time::SystemTime;
use tracing::{info, warn};

const KEEP_COUNT: usize = 10;
pub const KEEP_BYTES: u64 = 2 * 1024 * 1024 * 1024; // 2 GiB

#[derive(Debug, Default, PartialEq, Eq)]
pub struct GcReport {
    pub deleted: Vec<String>,
    pub errors: u32,
}

pub fn run(paths: &Paths, keep_active: Option<&str>, keep_previous: Option<&str>) -> GcReport {
    let mut report = GcReport::default();
    let releases = paths.releases_dir();
    if !releases.exists() {
        return report;
    }
    let mut entries: Vec<(String, PathBuf, SystemTime, u64)> = Vec::new();
    let dir = match std::fs::read_dir(&releases) {
        Ok(d) => d,
        Err(e) => {
            warn!("read_dir releases: {e}");
            return report;
        }
    };
    for entry in dir.flatten() {
        let path = entry.path();
        let name = match entry.file_name().into_string() {
            Ok(n) => n,
            Err(_) => continue,
        };
        let mtime = entry
            .metadata()
            .and_then(|m| m.modified())
            .unwrap_or(SystemTime::UNIX_EPOCH);
        let size = dir_size(&path);
        entries.push((name, path, mtime, size));
    }
    // Oldest first.
    entries.sort_by(|a, b| a.2.cmp(&b.2));

    // Compute total of "deletable" set (excludes active+previous).
    let is_kept = |name: &str| {
        keep_active.map(|k| k == name).unwrap_or(false)
            || keep_previous.map(|k| k == name).unwrap_or(false)
    };
    let mut total: u64 = entries.iter().map(|e| e.3).sum();
    let mut deletable_count = entries.iter().filter(|e| !is_kept(&e.0)).count();

    for (name, path, _, size) in &entries {
        if is_kept(name) {
            continue;
        }
        // Keep 10 most recent NON-kept directories.
        let must_delete_for_count = deletable_count > KEEP_COUNT;
        let must_delete_for_size = total > KEEP_BYTES;
        if !must_delete_for_count && !must_delete_for_size {
            break;
        }
        match std::fs::remove_dir_all(path) {
            Ok(()) => {
                report.deleted.push(name.clone());
                deletable_count = deletable_count.saturating_sub(1);
                total = total.saturating_sub(*size);
                info!(release = %name, bytes = size, "gc deleted release");
            }
            Err(e) => {
                report.errors = report.errors.saturating_add(1);
                warn!(release = %name, "gc delete failed: {e}");
            }
        }
    }
    report
}

fn dir_size(p: &std::path::Path) -> u64 {
    let mut total: u64 = 0;
    if let Ok(rd) = std::fs::read_dir(p) {
        for entry in rd.flatten() {
            let path = entry.path();
            let md = match std::fs::symlink_metadata(&path) {
                Ok(m) => m,
                Err(_) => continue,
            };
            if md.is_dir() {
                total = total.saturating_add(dir_size(&path));
            } else {
                total = total.saturating_add(md.len());
            }
        }
    }
    total
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    use tempfile::tempdir;

    fn synth_release(paths: &Paths, name: &str, body_bytes: usize, mtime_offset: i64) {
        let dir = paths.release_dir(name);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("config.json"), vec![0u8; body_bytes]).unwrap();
        let new_mtime = std::time::SystemTime::UNIX_EPOCH
            + Duration::from_secs((1_700_000_000_i64 + mtime_offset) as u64);
        let _ = filetime::set_file_mtime(
            &dir,
            filetime::FileTime::from_system_time(new_mtime),
        );
    }

    #[test]
    fn keeps_active_and_previous_even_if_count_exceeded() {
        let tmp = tempdir().unwrap();
        let paths = Paths::with_root(tmp.path());
        std::fs::create_dir_all(paths.releases_dir()).unwrap();
        for i in 0..15 {
            synth_release(&paths, &format!("r{i:02}"), 100, i);
        }
        let report = run(&paths, Some("r00"), Some("r01"));
        assert!(paths.release_dir("r00").exists());
        assert!(paths.release_dir("r01").exists());
        assert!(report.deleted.contains(&"r02".to_string()));
    }

    #[test]
    fn caps_count_to_10_among_non_kept() {
        let tmp = tempdir().unwrap();
        let paths = Paths::with_root(tmp.path());
        std::fs::create_dir_all(paths.releases_dir()).unwrap();
        for i in 0..14 {
            synth_release(&paths, &format!("r{i:02}"), 100, i);
        }
        let report = run(&paths, Some("r13"), Some("r12"));
        // 14 total - 2 kept = 12 candidates; cap = 10 → delete 2 oldest non-kept.
        assert_eq!(report.deleted.len(), 2);
        assert!(report.deleted.contains(&"r00".to_string()));
        assert!(report.deleted.contains(&"r01".to_string()));
    }

    #[test]
    fn skips_when_under_caps() {
        let tmp = tempdir().unwrap();
        let paths = Paths::with_root(tmp.path());
        std::fs::create_dir_all(paths.releases_dir()).unwrap();
        for i in 0..3 {
            synth_release(&paths, &format!("r{i}"), 100, i);
        }
        let report = run(&paths, Some("r2"), Some("r1"));
        assert!(report.deleted.is_empty());
    }

    #[test]
    fn no_releases_dir_is_a_noop() {
        let tmp = tempdir().unwrap();
        let paths = Paths::with_root(tmp.path());
        let report = run(&paths, None, None);
        assert!(report.deleted.is_empty());
        assert_eq!(report.errors, 0);
    }
}
```

The tests rely on `filetime` to backdate mtimes deterministically. Add to `crates/boxpilotd/Cargo.toml` `[dev-dependencies]`:

```toml
filetime = "0.2"
```

- [ ] **Step 3: Run tests**

Run: `cargo test -p boxpilotd profile::gc`
Expected: 4 tests pass.

- [ ] **Step 4: Commit**

```bash
git add crates/boxpilotd/src/profile/ crates/boxpilotd/Cargo.toml
git commit -m "feat(plan-5): release retention GC

T8 of plan #5. profile::gc::run honors §10 retention: keep active +
previous always, then keep up to 10 newest deletable AND ≤2 GiB total.
Best-effort: errors are tallied in GcReport, not propagated.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

## Task 9: Extend `StateCommit` for active/previous boxpilot.toml fields

**Files:**
- Modify: `crates/boxpilotd/src/core/commit.rs`

The existing `TomlUpdates` only carries `core_path` / `core_state`. Plan #5 needs to set `active_*` and `previous_*` atomically alongside.

- [ ] **Step 1: Add failing test**

Append to `mod tests` in `crates/boxpilotd/src/core/commit.rs`:

```rust
#[tokio::test]
async fn state_commit_writes_active_and_previous_fields() {
    let tmp = tempfile::tempdir().unwrap();
    let paths = crate::paths::Paths::with_root(tmp.path());
    std::fs::create_dir_all(paths.etc_dir()).unwrap();
    std::fs::write(
        paths.boxpilot_toml(),
        "schema_version = 1\ncontroller_uid = 1000\nactive_release_id = \"old-id\"\nactive_profile_id = \"old-p\"\nactive_profile_sha256 = \"old-sha\"\nactivated_at = \"2026-04-29T00:00:00-07:00\"\n",
    )
    .unwrap();

    let commit = StateCommit {
        paths: paths.clone(),
        toml_updates: TomlUpdates {
            core_path: None,
            core_state: None,
            active: Some(ActiveFields {
                release_id: "new-id".into(),
                profile_id: "new-p".into(),
                profile_name: Some("New".into()),
                profile_sha256: "new-sha".into(),
                activated_at: "2026-04-30T00:00:00-07:00".into(),
            }),
            previous: Some(PreviousFields {
                release_id: "old-id".into(),
                profile_id: "old-p".into(),
                profile_sha256: "old-sha".into(),
                activated_at: "2026-04-29T00:00:00-07:00".into(),
            }),
        },
        controller: None,
        install_state: boxpilot_ipc::InstallState::empty(),
        current_symlink_target: None,
    };
    commit.apply().await.unwrap();

    let cfg = boxpilot_ipc::BoxpilotConfig::parse(
        &tokio::fs::read_to_string(paths.boxpilot_toml()).await.unwrap(),
    )
    .unwrap();
    assert_eq!(cfg.active_release_id.as_deref(), Some("new-id"));
    assert_eq!(cfg.active_profile_id.as_deref(), Some("new-p"));
    assert_eq!(cfg.active_profile_name.as_deref(), Some("New"));
    assert_eq!(cfg.active_profile_sha256.as_deref(), Some("new-sha"));
    assert_eq!(cfg.activated_at.as_deref(), Some("2026-04-30T00:00:00-07:00"));
    assert_eq!(cfg.previous_release_id.as_deref(), Some("old-id"));
    assert_eq!(cfg.previous_profile_id.as_deref(), Some("old-p"));
    assert_eq!(cfg.previous_profile_sha256.as_deref(), Some("old-sha"));
    assert_eq!(cfg.previous_activated_at.as_deref(), Some("2026-04-29T00:00:00-07:00"));
}
```

- [ ] **Step 2: Run, expect failure**

Run: `cargo test -p boxpilotd state_commit_writes_active_and_previous_fields`
Expected: compile error (`ActiveFields`, `PreviousFields` missing).

- [ ] **Step 3: Add the structs and extend `TomlUpdates`**

In `crates/boxpilotd/src/core/commit.rs`, add near the top (above `TomlUpdates`):

```rust
#[derive(Debug, Clone)]
pub struct ActiveFields {
    pub release_id: String,
    pub profile_id: String,
    pub profile_name: Option<String>,
    pub profile_sha256: String,
    pub activated_at: String,
}

#[derive(Debug, Clone)]
pub struct PreviousFields {
    pub release_id: String,
    pub profile_id: String,
    pub profile_sha256: String,
    pub activated_at: String,
}
```

Replace the existing `TomlUpdates` definition with:

```rust
#[derive(Debug, Clone, Default)]
pub struct TomlUpdates {
    pub core_path: Option<String>,
    pub core_state: Option<CoreState>,
    pub active: Option<ActiveFields>,
    pub previous: Option<PreviousFields>,
}
```

- [ ] **Step 4: Apply the new fields in `StateCommit::apply`**

Find the section in `apply` that mutates `BoxpilotConfig` from `toml_updates`. After the existing `if let Some(p) = &self.toml_updates.core_path { cfg.core_path = Some(p.clone()); }` block, add:

```rust
        if let Some(active) = &self.toml_updates.active {
            cfg.active_release_id = Some(active.release_id.clone());
            cfg.active_profile_id = Some(active.profile_id.clone());
            cfg.active_profile_name = active.profile_name.clone();
            cfg.active_profile_sha256 = Some(active.profile_sha256.clone());
            cfg.activated_at = Some(active.activated_at.clone());
        }
        if let Some(prev) = &self.toml_updates.previous {
            cfg.previous_release_id = Some(prev.release_id.clone());
            cfg.previous_profile_id = Some(prev.profile_id.clone());
            cfg.previous_profile_sha256 = Some(prev.profile_sha256.clone());
            cfg.previous_activated_at = Some(prev.activated_at.clone());
        }
```

(If `apply` constructs `cfg` differently — e.g. parses then mutates — the locations are the same; the assignments just need to happen once on the local `cfg` before it is serialized to `boxpilot.toml.new`.)

- [ ] **Step 5: Re-export the structs from the module**

Ensure `pub struct ActiveFields` and `pub struct PreviousFields` are visible to `profile::activate` later. They live in `crate::core::commit`, which is already used by `iface.rs:419`, so no module changes are needed.

- [ ] **Step 6: Run tests**

Run: `cargo test -p boxpilotd commit::tests`
Expected: existing tests + 1 new = pass.

Run: `cargo test -p boxpilotd`
Expected: workspace still green.

- [ ] **Step 7: Commit**

```bash
git add crates/boxpilotd/src/core/commit.rs
git commit -m "feat(plan-5): StateCommit ActiveFields/PreviousFields

T9 of plan #5. Extends TomlUpdates with optional active+previous so
the activation orchestrator can flip both atomically alongside
existing core_path/core_state writes.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

## Task 10: `profile/activate.rs` — orchestrator (§10 state machine)

**Files:**
- Create: `crates/boxpilotd/src/profile/activate.rs`
- Modify: `crates/boxpilotd/src/profile/mod.rs`

This is the largest task. Read the spec §10 mapping in
`docs/superpowers/specs/2026-04-30-activation-pipeline-design.md` §5
before starting.

- [ ] **Step 1: Declare module**

`profile/mod.rs`:

```rust
pub mod activate;
```

- [ ] **Step 2: Write skeleton + happy-path test**

Create `crates/boxpilotd/src/profile/activate.rs` (the file is long; copy as-is):

```rust
//! Spec §10 activation pipeline state machine. Orchestrates: lock,
//! unpack, sing-box check, atomic rename to releases/<id>, atomic
//! active-symlink swap, service restart, verify, toml commit. On
//! verify failure: rollback to previous release with second verify.
//! Surfaces four explicit terminal outcomes.

use crate::core::commit::{ActiveFields, PreviousFields, StateCommit, TomlUpdates};
use crate::lock;
use crate::paths::Paths;
use crate::profile::checker::{CheckOutcome, SingboxChecker};
use crate::profile::gc;
use crate::profile::recovery;
use crate::profile::release::{promote_staging, read_active_target, swap_active_symlink};
use crate::profile::unpack::unpack_into;
use crate::profile::verifier::ServiceVerifier;
use crate::service::control::{self, Verb};
use crate::service::verify::{VerifyOutcome, DEFAULT_WINDOW, MAX_WINDOW};
use crate::systemd::Systemd;
use boxpilot_ipc::{
    ActivateBundleRequest, ActivateBundleResponse, ActivateOutcome, BoxpilotConfig, HelperError,
    HelperResult, UnitState, VerifySummary,
};
use chrono::Utc;
use std::os::fd::OwnedFd;
use std::path::Path;
use std::time::{Duration, Instant};
use tracing::{info, warn};

pub struct ActivateDeps<'a> {
    pub paths: Paths,
    pub systemd: &'a dyn Systemd,
    pub verifier: &'a dyn ServiceVerifier,
    pub checker: &'a dyn SingboxChecker,
}

pub async fn activate_bundle(
    req: ActivateBundleRequest,
    fd: OwnedFd,
    deps: &ActivateDeps<'_>,
) -> HelperResult<ActivateBundleResponse> {
    // Step 5: lock
    let _guard = lock::try_acquire(&deps.paths.run_lock())?;

    // Refuse if active is corrupt — startup recovery flagged it; activation
    // would just push another release on top of an unresolved problem.
    let pre_recovery = recovery::reconcile(&deps.paths).await;
    if pre_recovery.active_corrupt {
        return Err(HelperError::ActiveCorrupt);
    }

    // Need core_path + target_service from boxpilot.toml.
    let cfg_text = tokio::fs::read_to_string(deps.paths.boxpilot_toml())
        .await
        .map_err(|e| HelperError::Ipc { message: format!("read toml: {e}") })?;
    let cfg = BoxpilotConfig::parse(&cfg_text)?;
    let core_path = cfg
        .core_path
        .clone()
        .ok_or_else(|| HelperError::Ipc { message: "core_path not set; install_managed first".into() })?;
    let target_service = cfg.target_service.clone();

    // Step 6: unpack
    // We pick the staging subdir name from manifest.activation_id AFTER unpack
    // — but unpack needs a staging path up-front. Use a temporary nonce and
    // rename to .staging/<activation_id> after we read the manifest.
    let nonce = format!(".unpack-{}", Utc::now().format("%Y%m%dT%H%M%S%fZ"));
    let staging_root = deps.paths.staging_dir();
    tokio::fs::create_dir_all(&staging_root)
        .await
        .map_err(|e| HelperError::Ipc { message: format!("mkdir staging root: {e}") })?;
    let temp_staging = staging_root.join(&nonce);

    let report = unpack_into(fd, &temp_staging, req.expected_total_bytes)?;
    let activation_id = report.manifest.activation_id.clone();
    let staging_path = deps.paths.staging_subdir(&activation_id);
    if staging_path.exists() {
        // Collision with an existing crashed staging; remove and retry.
        tokio::fs::remove_dir_all(&staging_path)
            .await
            .map_err(|e| HelperError::Ipc { message: format!("clean staging collision: {e}") })?;
    }
    tokio::fs::rename(&temp_staging, &staging_path)
        .await
        .map_err(|e| HelperError::Ipc { message: format!("rename staging nonce: {e}") })?;

    // Step 7: sing-box check
    let check = deps.checker.check(Path::new(&core_path), &staging_path).await?;
    if check.exit != 0 {
        let _ = tokio::fs::remove_dir_all(&staging_path).await;
        return Err(HelperError::SingboxCheckFailed {
            exit: check.exit,
            stderr_tail: check.stderr_tail,
        });
    }

    // Step 8: rename(2) staging -> releases/<id>
    let release_dir = deps.paths.release_dir(&activation_id);
    promote_staging(&staging_path, &release_dir)?;

    // Capture pre-restart state for verify.
    let pre_state = deps.systemd.unit_state(&target_service).await?;
    let n_restarts_pre = match &pre_state {
        UnitState::Known { n_restarts, .. } => *n_restarts,
        UnitState::NotFound => 0,
    };

    // Step 9: swap active
    let prev_active_target = read_active_target(&deps.paths.active_symlink());
    swap_active_symlink(&deps.paths.active_symlink(), &release_dir)?;

    // Step 10: restart service
    if let Err(e) = control::run(Verb::Restart, &target_service, deps.systemd).await {
        warn!("restart after swap failed: {e:?}");
        return rollback_path(deps, &target_service, &activation_id, prev_active_target.as_deref(), req.verify_window_secs).await;
    }

    // Step 11–12: verify
    let window = window_from_request(req.verify_window_secs);
    let started = Instant::now();
    let verify_outcome = deps
        .verifier
        .wait_for_running(&target_service, n_restarts_pre, window, deps.systemd)
        .await?;
    let elapsed_ms = started.elapsed().as_millis() as u64;

    let post_state = deps.systemd.unit_state(&target_service).await.ok();
    let n_restarts_post = match post_state.as_ref() {
        Some(UnitState::Known { n_restarts, .. }) => *n_restarts,
        _ => n_restarts_pre,
    };
    let verify_summary = VerifySummary {
        window_used_ms: elapsed_ms,
        n_restarts_pre,
        n_restarts_post,
        final_unit_state: post_state.clone(),
    };

    match verify_outcome {
        VerifyOutcome::Running => {
            // Step 13: toml commit
            let manifest = report.manifest.clone();
            let active = ActiveFields {
                release_id: activation_id.clone(),
                profile_id: manifest.profile_id.clone(),
                profile_name: None,
                profile_sha256: manifest.profile_sha256.clone(),
                activated_at: manifest.created_at.clone(),
            };
            let previous = build_previous_from_cfg(&cfg);
            let commit = StateCommit {
                paths: deps.paths.clone(),
                toml_updates: TomlUpdates {
                    core_path: None,
                    core_state: None,
                    active: Some(active),
                    previous,
                },
                controller: None,
                install_state: boxpilot_ipc::InstallState::empty(),
                current_symlink_target: None,
            };
            commit.apply().await?;

            // GC: best-effort.
            let new_cfg_text = tokio::fs::read_to_string(deps.paths.boxpilot_toml())
                .await
                .ok();
            let new_cfg = new_cfg_text.as_deref().and_then(|t| BoxpilotConfig::parse(t).ok());
            let report_gc = gc::run(
                &deps.paths,
                Some(&activation_id),
                new_cfg.as_ref().and_then(|c| c.previous_release_id.as_deref()),
            );
            if !report_gc.deleted.is_empty() {
                info!(deleted = ?report_gc.deleted, "gc completed after activation");
            }
            if report_gc.errors > 0 {
                warn!(errors = report_gc.errors, "gc had errors after activation");
            }

            Ok(ActivateBundleResponse {
                outcome: ActivateOutcome::Active,
                activation_id,
                previous_activation_id: cfg.active_release_id.clone(),
                verify: verify_summary,
            })
        }
        VerifyOutcome::Stuck { .. } | VerifyOutcome::NotFound => {
            rollback_after_verify_failure(
                deps,
                &target_service,
                &activation_id,
                prev_active_target.as_deref(),
                cfg.active_release_id.clone(),
                verify_summary,
                req.verify_window_secs,
            )
            .await
        }
    }
}

fn window_from_request(secs: Option<u32>) -> Duration {
    match secs {
        None => DEFAULT_WINDOW,
        Some(0) => DEFAULT_WINDOW,
        Some(s) => Duration::from_secs(s as u64).min(MAX_WINDOW),
    }
}

fn build_previous_from_cfg(cfg: &BoxpilotConfig) -> Option<PreviousFields> {
    let release_id = cfg.active_release_id.clone()?;
    let profile_id = cfg.active_profile_id.clone()?;
    let profile_sha256 = cfg.active_profile_sha256.clone()?;
    let activated_at = cfg.activated_at.clone()?;
    Some(PreviousFields { release_id, profile_id, profile_sha256, activated_at })
}

/// Used when the restart itself failed — same as verify-failure rollback
/// but without a verify summary from the failed window.
async fn rollback_path(
    deps: &ActivateDeps<'_>,
    target_service: &str,
    new_id: &str,
    prev_target: Option<&Path>,
    window_secs: Option<u32>,
) -> HelperResult<ActivateBundleResponse> {
    let summary = VerifySummary {
        window_used_ms: 0,
        n_restarts_pre: 0,
        n_restarts_post: 0,
        final_unit_state: None,
    };
    rollback_after_verify_failure(
        deps,
        target_service,
        new_id,
        prev_target,
        None,
        summary,
        window_secs,
    )
    .await
}

async fn rollback_after_verify_failure(
    deps: &ActivateDeps<'_>,
    target_service: &str,
    failed_id: &str,
    prev_target: Option<&Path>,
    prev_release_id: Option<String>,
    failed_verify_summary: VerifySummary,
    window_secs: Option<u32>,
) -> HelperResult<ActivateBundleResponse> {
    let prev_target = match prev_target {
        Some(p) => p,
        None => {
            // §10 step 14: no previous → leave active at failed; stop service.
            let _ = control::run(Verb::Stop, target_service, deps.systemd).await;
            return Ok(ActivateBundleResponse {
                outcome: ActivateOutcome::RollbackTargetMissing,
                activation_id: failed_id.into(),
                previous_activation_id: None,
                verify: failed_verify_summary,
            });
        }
    };

    swap_active_symlink(&deps.paths.active_symlink(), prev_target)?;
    let n_restarts_pre = match deps.systemd.unit_state(target_service).await? {
        UnitState::Known { n_restarts, .. } => n_restarts,
        UnitState::NotFound => 0,
    };
    let _ = control::run(Verb::Restart, target_service, deps.systemd).await;
    let window = window_from_request(window_secs);
    let started = Instant::now();
    let outcome = deps
        .verifier
        .wait_for_running(target_service, n_restarts_pre, window, deps.systemd)
        .await?;
    let elapsed_ms = started.elapsed().as_millis() as u64;
    let post_state = deps.systemd.unit_state(target_service).await.ok();
    let n_restarts_post = match post_state.as_ref() {
        Some(UnitState::Known { n_restarts, .. }) => *n_restarts,
        _ => n_restarts_pre,
    };
    let summary = VerifySummary {
        window_used_ms: elapsed_ms,
        n_restarts_pre,
        n_restarts_post,
        final_unit_state: post_state.clone(),
    };
    match outcome {
        VerifyOutcome::Running => Ok(ActivateBundleResponse {
            outcome: ActivateOutcome::RolledBack,
            activation_id: failed_id.into(),
            previous_activation_id: prev_release_id,
            verify: summary,
        }),
        VerifyOutcome::Stuck { .. } | VerifyOutcome::NotFound => {
            // §10 step 15: previous also unstartable → stop service.
            let _ = control::run(Verb::Stop, target_service, deps.systemd).await;
            Ok(ActivateBundleResponse {
                outcome: ActivateOutcome::RollbackUnstartable,
                activation_id: failed_id.into(),
                previous_activation_id: prev_release_id,
                verify: summary,
            })
        }
    }
}

#[cfg(test)]
mod tests;
```

- [ ] **Step 3: Create `activate/tests.rs`**

Replace the `mod tests;` line above with an inline `#[cfg(test)] mod tests { ... }` (or create a separate file at `crates/boxpilotd/src/profile/activate/` and add `tests.rs`; either works — inline is simpler):

Append at end of `activate.rs`:

```rust
#[cfg(test)]
mod tests_impl {
    use super::*;
    use crate::profile::checker::testing::FakeChecker;
    use crate::profile::checker::CheckOutcome;
    use crate::profile::verifier::testing::ScriptedVerifier;
    use crate::systemd::testing::{RecordingSystemd, FixedSystemd};
    use boxpilot_ipc::{ActivationManifest, ACTIVATION_MANIFEST_SCHEMA_VERSION, SourceKind};
    use std::sync::Arc;
    use tempfile::tempdir;

    fn write_default_toml(paths: &Paths, with_active: bool) {
        std::fs::create_dir_all(paths.etc_dir()).unwrap();
        let mut text = String::from(
            "schema_version = 1\ncontroller_uid = 1000\ncore_path = \"/usr/bin/sing-box\"\n",
        );
        if with_active {
            text.push_str("active_release_id = \"old-id\"\nactive_profile_id = \"old-p\"\nactive_profile_sha256 = \"old-sha\"\nactivated_at = \"2026-04-29T00:00:00-07:00\"\n");
        }
        std::fs::write(paths.boxpilot_toml(), text).unwrap();
    }

    fn make_bundle_memfd(activation_id: &str) -> OwnedFd {
        use std::ffi::CString;
        use std::io::Write;
        let raw = nix::sys::memfd::memfd_create(
            CString::new("test").unwrap().as_c_str(),
            nix::sys::memfd::MemFdCreateFlag::MFD_CLOEXEC,
        )
        .expect("memfd_create");
        let fd: OwnedFd = raw;
        let manifest = ActivationManifest {
            schema_version: ACTIVATION_MANIFEST_SCHEMA_VERSION,
            activation_id: activation_id.into(),
            profile_id: "p".into(),
            profile_sha256: "sha".into(),
            config_sha256: "cfgsha".into(),
            source_kind: SourceKind::Local,
            source_url_redacted: None,
            core_path_at_activation: "/usr/bin/sing-box".into(),
            core_version_at_activation: "1.10.0".into(),
            created_at: "2026-04-30T00:00:00-07:00".into(),
            assets: Vec::new(),
        };
        let mbytes = serde_json::to_vec_pretty(&manifest).unwrap();
        let mut f = std::fs::File::from(fd.try_clone().unwrap());
        let mut b = tar::Builder::new(&mut f);
        let mut h = tar::Header::new_ustar();
        h.set_size(2);
        h.set_mode(0o600);
        h.set_entry_type(tar::EntryType::Regular);
        h.set_cksum();
        b.append_data(&mut h, "config.json", &b"{}"[..]).unwrap();
        let mut h2 = tar::Header::new_ustar();
        h2.set_size(mbytes.len() as u64);
        h2.set_mode(0o600);
        h2.set_entry_type(tar::EntryType::Regular);
        h2.set_cksum();
        b.append_data(&mut h2, "manifest.json", mbytes.as_slice()).unwrap();
        b.finish().unwrap();
        fd
    }

    #[tokio::test]
    async fn happy_path_returns_active_and_writes_active_toml() {
        let tmp = tempdir().unwrap();
        let paths = Paths::with_root(tmp.path());
        write_default_toml(&paths, false);
        std::fs::create_dir_all(paths.run_dir()).unwrap();

        let systemd = Arc::new(FixedSystemd { answer: UnitState::Known {
            active_state: "active".into(), sub_state: "running".into(),
            load_state: "loaded".into(), n_restarts: 0, exec_main_status: 0,
        }});
        let verifier = ScriptedVerifier::new(vec![VerifyOutcome::Running]);
        let checker = FakeChecker::ok();
        let deps = ActivateDeps {
            paths: paths.clone(),
            systemd: &*systemd,
            verifier: &verifier,
            checker: &checker,
        };
        let fd = make_bundle_memfd("act-1");
        let req = ActivateBundleRequest::default();
        let resp = activate_bundle(req, fd, &deps).await.unwrap();
        assert_eq!(resp.outcome, ActivateOutcome::Active);
        assert_eq!(resp.activation_id, "act-1");
        assert!(paths.release_dir("act-1").exists());
        assert!(!paths.staging_subdir("act-1").exists());
        let cfg = boxpilot_ipc::BoxpilotConfig::parse(
            &std::fs::read_to_string(paths.boxpilot_toml()).unwrap(),
        )
        .unwrap();
        assert_eq!(cfg.active_release_id.as_deref(), Some("act-1"));
    }

    #[tokio::test]
    async fn singbox_check_failure_aborts_and_cleans_staging() {
        let tmp = tempdir().unwrap();
        let paths = Paths::with_root(tmp.path());
        write_default_toml(&paths, false);
        std::fs::create_dir_all(paths.run_dir()).unwrap();
        let systemd = Arc::new(FixedSystemd { answer: UnitState::NotFound });
        let verifier = ScriptedVerifier::new(vec![]);
        let checker = FakeChecker::fail();
        let deps = ActivateDeps { paths: paths.clone(), systemd: &*systemd, verifier: &verifier, checker: &checker };
        let fd = make_bundle_memfd("act-2");
        let err = activate_bundle(ActivateBundleRequest::default(), fd, &deps).await.unwrap_err();
        assert!(matches!(err, HelperError::SingboxCheckFailed { .. }));
        assert!(!paths.staging_subdir("act-2").exists());
        assert!(!paths.release_dir("act-2").exists());
    }

    #[tokio::test]
    async fn verify_stuck_with_no_previous_returns_rollback_target_missing() {
        let tmp = tempdir().unwrap();
        let paths = Paths::with_root(tmp.path());
        write_default_toml(&paths, false);
        std::fs::create_dir_all(paths.run_dir()).unwrap();
        let systemd = Arc::new(FixedSystemd { answer: UnitState::Known {
            active_state: "activating".into(), sub_state: "auto-restart".into(),
            load_state: "loaded".into(), n_restarts: 5, exec_main_status: 1,
        }});
        let verifier = ScriptedVerifier::new(vec![
            VerifyOutcome::Stuck { final_state: UnitState::Known {
                active_state: "activating".into(), sub_state: "auto-restart".into(),
                load_state: "loaded".into(), n_restarts: 5, exec_main_status: 1,
            }},
        ]);
        let checker = FakeChecker::ok();
        let deps = ActivateDeps { paths: paths.clone(), systemd: &*systemd, verifier: &verifier, checker: &checker };
        let fd = make_bundle_memfd("act-3");
        let resp = activate_bundle(ActivateBundleRequest::default(), fd, &deps).await.unwrap();
        assert_eq!(resp.outcome, ActivateOutcome::RollbackTargetMissing);
        assert!(paths.release_dir("act-3").exists());
        assert_eq!(
            std::fs::read_link(paths.active_symlink()).unwrap(),
            paths.release_dir("act-3"),
        );
    }

    #[tokio::test]
    async fn verify_stuck_with_previous_rolls_back_and_returns_rolled_back() {
        let tmp = tempdir().unwrap();
        let paths = Paths::with_root(tmp.path());
        // Pre-create previous release on disk.
        let prev = paths.release_dir("prev-id");
        std::fs::create_dir_all(&prev).unwrap();
        std::fs::create_dir_all(paths.active_symlink().parent().unwrap()).unwrap();
        std::os::unix::fs::symlink(&prev, paths.active_symlink()).unwrap();
        write_default_toml(&paths, true);
        std::fs::create_dir_all(paths.run_dir()).unwrap();

        let systemd = Arc::new(FixedSystemd { answer: UnitState::Known {
            active_state: "active".into(), sub_state: "running".into(),
            load_state: "loaded".into(), n_restarts: 0, exec_main_status: 0,
        }});
        let verifier = ScriptedVerifier::new(vec![
            VerifyOutcome::Stuck { final_state: UnitState::NotFound },
            VerifyOutcome::Running,
        ]);
        let checker = FakeChecker::ok();
        let deps = ActivateDeps { paths: paths.clone(), systemd: &*systemd, verifier: &verifier, checker: &checker };
        let fd = make_bundle_memfd("new-id");
        let resp = activate_bundle(ActivateBundleRequest::default(), fd, &deps).await.unwrap();
        assert_eq!(resp.outcome, ActivateOutcome::RolledBack);
        assert_eq!(
            std::fs::read_link(paths.active_symlink()).unwrap(),
            prev,
            "active should be restored to previous after rollback",
        );
    }

    #[tokio::test]
    async fn rollback_unstartable_when_previous_also_fails() {
        let tmp = tempdir().unwrap();
        let paths = Paths::with_root(tmp.path());
        let prev = paths.release_dir("prev-id");
        std::fs::create_dir_all(&prev).unwrap();
        std::fs::create_dir_all(paths.active_symlink().parent().unwrap()).unwrap();
        std::os::unix::fs::symlink(&prev, paths.active_symlink()).unwrap();
        write_default_toml(&paths, true);
        std::fs::create_dir_all(paths.run_dir()).unwrap();

        let systemd = Arc::new(FixedSystemd { answer: UnitState::Known {
            active_state: "active".into(), sub_state: "running".into(),
            load_state: "loaded".into(), n_restarts: 0, exec_main_status: 0,
        }});
        let verifier = ScriptedVerifier::new(vec![
            VerifyOutcome::Stuck { final_state: UnitState::NotFound },
            VerifyOutcome::Stuck { final_state: UnitState::NotFound },
        ]);
        let checker = FakeChecker::ok();
        let deps = ActivateDeps { paths: paths.clone(), systemd: &*systemd, verifier: &verifier, checker: &checker };
        let fd = make_bundle_memfd("new-id-2");
        let resp = activate_bundle(ActivateBundleRequest::default(), fd, &deps).await.unwrap();
        assert_eq!(resp.outcome, ActivateOutcome::RollbackUnstartable);
    }

    #[tokio::test]
    async fn missing_core_path_returns_ipc_error() {
        let tmp = tempdir().unwrap();
        let paths = Paths::with_root(tmp.path());
        std::fs::create_dir_all(paths.etc_dir()).unwrap();
        std::fs::write(paths.boxpilot_toml(), "schema_version = 1\n").unwrap();
        std::fs::create_dir_all(paths.run_dir()).unwrap();
        let systemd = Arc::new(FixedSystemd { answer: UnitState::NotFound });
        let verifier = ScriptedVerifier::new(vec![]);
        let checker = FakeChecker::ok();
        let deps = ActivateDeps { paths: paths.clone(), systemd: &*systemd, verifier: &verifier, checker: &checker };
        let fd = make_bundle_memfd("x");
        let err = activate_bundle(ActivateBundleRequest::default(), fd, &deps).await.unwrap_err();
        assert!(matches!(err, HelperError::Ipc { .. }));
    }

    #[tokio::test]
    async fn active_corrupt_blocks_activation() {
        let tmp = tempdir().unwrap();
        let paths = Paths::with_root(tmp.path());
        write_default_toml(&paths, false);
        std::fs::create_dir_all(paths.run_dir()).unwrap();
        // Plant a dangling active symlink.
        std::fs::create_dir_all(paths.active_symlink().parent().unwrap()).unwrap();
        std::os::unix::fs::symlink(tmp.path().join("ghost"), paths.active_symlink()).unwrap();
        let systemd = Arc::new(FixedSystemd { answer: UnitState::NotFound });
        let verifier = ScriptedVerifier::new(vec![]);
        let checker = FakeChecker::ok();
        let deps = ActivateDeps { paths: paths.clone(), systemd: &*systemd, verifier: &verifier, checker: &checker };
        let fd = make_bundle_memfd("x");
        let err = activate_bundle(ActivateBundleRequest::default(), fd, &deps).await.unwrap_err();
        assert!(matches!(err, HelperError::ActiveCorrupt));
    }

    #[tokio::test]
    async fn window_clamped_to_max() {
        // 100s requested, MAX_WINDOW caps at 30s. The verifier doesn't
        // observe window duration directly; this test asserts the
        // clamp through the helper.
        assert_eq!(window_from_request(Some(100)), MAX_WINDOW);
        assert_eq!(window_from_request(Some(0)), DEFAULT_WINDOW);
        assert_eq!(window_from_request(None), DEFAULT_WINDOW);
        assert_eq!(window_from_request(Some(7)), Duration::from_secs(7));
    }
}
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p boxpilotd profile::activate`
Expected: 7 tests pass.

If `service::control` requires more verbs than `Restart`/`Stop`, check `crates/boxpilotd/src/service/control.rs` and adjust imports.

- [ ] **Step 5: Commit**

```bash
git add crates/boxpilotd/src/profile/
git commit -m "feat(plan-5): activation orchestrator + 7 outcome tests

T10 of plan #5. profile::activate::activate_bundle implements §10
steps 5-15. Lock → unpack → check → promote → swap → restart → verify
→ commit-or-rollback. Four ActivateOutcome variants exercised
end-to-end with FakeChecker + ScriptedVerifier. Best-effort GC after
Active.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

## Task 11: `profile/rollback.rs` — manual rollback verb

**Files:**
- Create: `crates/boxpilotd/src/profile/rollback.rs`
- Modify: `crates/boxpilotd/src/profile/mod.rs`

- [ ] **Step 1: Declare module**

`profile/mod.rs`:

```rust
pub mod rollback;
```

- [ ] **Step 2: Tests + impl**

Create `crates/boxpilotd/src/profile/rollback.rs`:

```rust
//! Spec §10 manual rollback. Differs from auto-rollback in two ways:
//! (1) caller picks a specific historical activation_id, (2) on
//! verify success the toml swap is symmetric — what was active becomes
//! previous. GC does not run inside this verb.

use crate::core::commit::{ActiveFields, PreviousFields, StateCommit, TomlUpdates};
use crate::lock;
use crate::paths::Paths;
use crate::profile::recovery;
use crate::profile::release::{read_active_target, swap_active_symlink};
use crate::profile::verifier::ServiceVerifier;
use crate::service::control::{self, Verb};
use crate::service::verify::VerifyOutcome;
use crate::systemd::Systemd;
use boxpilot_ipc::{
    ActivateBundleResponse, ActivateOutcome, ActivationManifest, BoxpilotConfig, HelperError,
    HelperResult, RollbackRequest, UnitState, VerifySummary,
};
use chrono::Utc;
use std::time::{Duration, Instant};

pub struct RollbackDeps<'a> {
    pub paths: Paths,
    pub systemd: &'a dyn Systemd,
    pub verifier: &'a dyn ServiceVerifier,
}

pub async fn rollback_release(
    req: RollbackRequest,
    deps: &RollbackDeps<'_>,
) -> HelperResult<ActivateBundleResponse> {
    let _guard = lock::try_acquire(&deps.paths.run_lock())?;

    if recovery::reconcile(&deps.paths).await.active_corrupt {
        return Err(HelperError::ActiveCorrupt);
    }

    let cfg_text = tokio::fs::read_to_string(deps.paths.boxpilot_toml())
        .await
        .map_err(|e| HelperError::Ipc { message: format!("read toml: {e}") })?;
    let cfg = BoxpilotConfig::parse(&cfg_text)?;
    let target_service = cfg.target_service.clone();

    let target_id = &req.target_activation_id;
    if cfg.active_release_id.as_deref() == Some(target_id.as_str()) {
        return Err(HelperError::ReleaseAlreadyActive);
    }
    let target_dir = deps.paths.release_dir(target_id);
    if !target_dir.exists() {
        return Err(HelperError::ReleaseNotFound { activation_id: target_id.clone() });
    }
    // Read the target's manifest to populate active_* fields.
    let manifest_path = target_dir.join("manifest.json");
    let manifest_bytes = tokio::fs::read(&manifest_path)
        .await
        .map_err(|e| HelperError::Ipc { message: format!("read target manifest: {e}") })?;
    let target_manifest: ActivationManifest = serde_json::from_slice(&manifest_bytes)
        .map_err(|e| HelperError::Ipc { message: format!("parse target manifest: {e}") })?;

    let prev_active_target = read_active_target(&deps.paths.active_symlink());

    let pre_state = deps.systemd.unit_state(&target_service).await?;
    let n_restarts_pre = match &pre_state {
        UnitState::Known { n_restarts, .. } => *n_restarts,
        UnitState::NotFound => 0,
    };

    swap_active_symlink(&deps.paths.active_symlink(), &target_dir)?;
    let _ = control::run(Verb::Restart, &target_service, deps.systemd).await;

    let window = window_from_request(req.verify_window_secs);
    let started = Instant::now();
    let outcome = deps
        .verifier
        .wait_for_running(&target_service, n_restarts_pre, window, deps.systemd)
        .await?;
    let elapsed_ms = started.elapsed().as_millis() as u64;
    let post_state = deps.systemd.unit_state(&target_service).await.ok();
    let n_restarts_post = match post_state.as_ref() {
        Some(UnitState::Known { n_restarts, .. }) => *n_restarts,
        _ => n_restarts_pre,
    };
    let summary = VerifySummary {
        window_used_ms: elapsed_ms,
        n_restarts_pre,
        n_restarts_post,
        final_unit_state: post_state.clone(),
    };

    match outcome {
        VerifyOutcome::Running => {
            // Symmetric swap: what was active becomes previous.
            let active = ActiveFields {
                release_id: target_id.clone(),
                profile_id: target_manifest.profile_id.clone(),
                profile_name: None,
                profile_sha256: target_manifest.profile_sha256.clone(),
                activated_at: Utc::now().to_rfc3339(),
            };
            let previous = if let (Some(rid), Some(pid), Some(psha), Some(at)) = (
                cfg.active_release_id.clone(),
                cfg.active_profile_id.clone(),
                cfg.active_profile_sha256.clone(),
                cfg.activated_at.clone(),
            ) {
                Some(PreviousFields {
                    release_id: rid,
                    profile_id: pid,
                    profile_sha256: psha,
                    activated_at: at,
                })
            } else {
                None
            };
            let commit = StateCommit {
                paths: deps.paths.clone(),
                toml_updates: TomlUpdates {
                    core_path: None,
                    core_state: None,
                    active: Some(active),
                    previous,
                },
                controller: None,
                install_state: boxpilot_ipc::InstallState::empty(),
                current_symlink_target: None,
            };
            commit.apply().await?;
            Ok(ActivateBundleResponse {
                outcome: ActivateOutcome::Active,
                activation_id: target_id.clone(),
                previous_activation_id: cfg.active_release_id.clone(),
                verify: summary,
            })
        }
        VerifyOutcome::Stuck { .. } | VerifyOutcome::NotFound => {
            // Re-swap back. If the original active was missing on disk we
            // cannot restore — surface RollbackTargetMissing.
            let restore_target = match prev_active_target {
                Some(p) => p,
                None => {
                    let _ = control::run(Verb::Stop, &target_service, deps.systemd).await;
                    return Ok(ActivateBundleResponse {
                        outcome: ActivateOutcome::RollbackTargetMissing,
                        activation_id: target_id.clone(),
                        previous_activation_id: cfg.active_release_id.clone(),
                        verify: summary,
                    });
                }
            };
            swap_active_symlink(&deps.paths.active_symlink(), &restore_target)?;
            let _ = control::run(Verb::Restart, &target_service, deps.systemd).await;
            let started2 = Instant::now();
            let restore_outcome = deps
                .verifier
                .wait_for_running(&target_service, n_restarts_post, window, deps.systemd)
                .await?;
            let elapsed2 = started2.elapsed().as_millis() as u64;
            let post2 = deps.systemd.unit_state(&target_service).await.ok();
            let final_summary = VerifySummary {
                window_used_ms: elapsed2,
                n_restarts_pre: n_restarts_post,
                n_restarts_post: match post2.as_ref() {
                    Some(UnitState::Known { n_restarts, .. }) => *n_restarts,
                    _ => n_restarts_post,
                },
                final_unit_state: post2,
            };
            match restore_outcome {
                VerifyOutcome::Running => Ok(ActivateBundleResponse {
                    // Manual rollback FAILED to switch but the original
                    // restored cleanly; surface as RollbackUnstartable so the
                    // GUI knows the chosen target is broken. ("RolledBack"
                    // is the auto-rollback semantic and is not used here.)
                    outcome: ActivateOutcome::RollbackUnstartable,
                    activation_id: target_id.clone(),
                    previous_activation_id: cfg.active_release_id.clone(),
                    verify: final_summary,
                }),
                _ => {
                    let _ = control::run(Verb::Stop, &target_service, deps.systemd).await;
                    Ok(ActivateBundleResponse {
                        outcome: ActivateOutcome::RollbackUnstartable,
                        activation_id: target_id.clone(),
                        previous_activation_id: cfg.active_release_id.clone(),
                        verify: final_summary,
                    })
                }
            }
        }
    }
}

fn window_from_request(secs: Option<u32>) -> Duration {
    use crate::service::verify::{DEFAULT_WINDOW, MAX_WINDOW};
    match secs {
        None | Some(0) => DEFAULT_WINDOW,
        Some(s) => Duration::from_secs(s as u64).min(MAX_WINDOW),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profile::checker::testing::FakeChecker;
    use crate::profile::verifier::testing::ScriptedVerifier;
    use crate::systemd::testing::FixedSystemd;
    use boxpilot_ipc::{ActivationManifest, ACTIVATION_MANIFEST_SCHEMA_VERSION, SourceKind};
    use std::sync::Arc;
    use tempfile::tempdir;

    fn write_release(paths: &Paths, id: &str, profile_id: &str) {
        let dir = paths.release_dir(id);
        std::fs::create_dir_all(&dir).unwrap();
        let m = ActivationManifest {
            schema_version: ACTIVATION_MANIFEST_SCHEMA_VERSION,
            activation_id: id.into(),
            profile_id: profile_id.into(),
            profile_sha256: "psha".into(),
            config_sha256: "csha".into(),
            source_kind: SourceKind::Local,
            source_url_redacted: None,
            core_path_at_activation: "/usr/bin/sing-box".into(),
            core_version_at_activation: "1.10.0".into(),
            created_at: "2026-04-30T00:00:00-07:00".into(),
            assets: Vec::new(),
        };
        std::fs::write(dir.join("manifest.json"), serde_json::to_vec_pretty(&m).unwrap()).unwrap();
    }

    #[tokio::test]
    async fn rollback_target_missing_returns_release_not_found() {
        let tmp = tempdir().unwrap();
        let paths = Paths::with_root(tmp.path());
        std::fs::create_dir_all(paths.etc_dir()).unwrap();
        std::fs::write(paths.boxpilot_toml(), "schema_version = 1\nactive_release_id = \"cur\"\n").unwrap();
        std::fs::create_dir_all(paths.run_dir()).unwrap();
        let systemd = Arc::new(FixedSystemd { answer: UnitState::NotFound });
        let verifier = ScriptedVerifier::new(vec![]);
        let deps = RollbackDeps { paths: paths.clone(), systemd: &*systemd, verifier: &verifier };
        let err = rollback_release(
            RollbackRequest { target_activation_id: "ghost".into(), verify_window_secs: None },
            &deps,
        )
        .await
        .unwrap_err();
        assert!(matches!(err, HelperError::ReleaseNotFound { .. }));
    }

    #[tokio::test]
    async fn rollback_to_already_active_release_is_refused() {
        let tmp = tempdir().unwrap();
        let paths = Paths::with_root(tmp.path());
        std::fs::create_dir_all(paths.etc_dir()).unwrap();
        std::fs::write(
            paths.boxpilot_toml(),
            "schema_version = 1\nactive_release_id = \"cur\"\n",
        )
        .unwrap();
        std::fs::create_dir_all(paths.run_dir()).unwrap();
        write_release(&paths, "cur", "pcur");
        let systemd = Arc::new(FixedSystemd { answer: UnitState::NotFound });
        let verifier = ScriptedVerifier::new(vec![]);
        let deps = RollbackDeps { paths: paths.clone(), systemd: &*systemd, verifier: &verifier };
        let err = rollback_release(
            RollbackRequest { target_activation_id: "cur".into(), verify_window_secs: None },
            &deps,
        )
        .await
        .unwrap_err();
        assert!(matches!(err, HelperError::ReleaseAlreadyActive));
    }

    #[tokio::test]
    async fn rollback_happy_path_swaps_active_and_writes_toml() {
        let tmp = tempdir().unwrap();
        let paths = Paths::with_root(tmp.path());
        std::fs::create_dir_all(paths.etc_dir()).unwrap();
        std::fs::write(
            paths.boxpilot_toml(),
            "schema_version = 1\nactive_release_id = \"cur\"\nactive_profile_id = \"pcur\"\nactive_profile_sha256 = \"sha-cur\"\nactivated_at = \"2026-04-29T00:00:00-07:00\"\n",
        )
        .unwrap();
        std::fs::create_dir_all(paths.run_dir()).unwrap();
        std::fs::create_dir_all(paths.active_symlink().parent().unwrap()).unwrap();
        write_release(&paths, "cur", "pcur");
        write_release(&paths, "tgt", "ptgt");
        std::os::unix::fs::symlink(paths.release_dir("cur"), paths.active_symlink()).unwrap();

        let systemd = Arc::new(FixedSystemd { answer: UnitState::Known {
            active_state: "active".into(), sub_state: "running".into(),
            load_state: "loaded".into(), n_restarts: 0, exec_main_status: 0,
        }});
        let verifier = ScriptedVerifier::new(vec![VerifyOutcome::Running]);
        let deps = RollbackDeps { paths: paths.clone(), systemd: &*systemd, verifier: &verifier };
        let resp = rollback_release(
            RollbackRequest { target_activation_id: "tgt".into(), verify_window_secs: Some(2) },
            &deps,
        )
        .await
        .unwrap();
        assert_eq!(resp.outcome, ActivateOutcome::Active);
        assert_eq!(resp.activation_id, "tgt");
        let cfg = boxpilot_ipc::BoxpilotConfig::parse(
            &std::fs::read_to_string(paths.boxpilot_toml()).unwrap(),
        )
        .unwrap();
        assert_eq!(cfg.active_release_id.as_deref(), Some("tgt"));
        assert_eq!(cfg.previous_release_id.as_deref(), Some("cur"));
    }
}
```

- [ ] **Step 3: Run tests**

Run: `cargo test -p boxpilotd profile::rollback`
Expected: 3 tests pass.

- [ ] **Step 4: Commit**

```bash
git add crates/boxpilotd/src/profile/
git commit -m "feat(plan-5): manual rollback verb

T11 of plan #5. profile::rollback::rollback_release picks a specific
historical release_id, performs the symmetric symlink swap + restart
+ verify, and on success rewrites active/previous in boxpilot.toml.
On failure: re-swap back and stop service. No GC.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

## Task 12: Wire iface stubs to real handlers

**Files:**
- Modify: `crates/boxpilotd/src/iface.rs`
- Modify: `crates/boxpilotd/src/context.rs`

- [ ] **Step 1: Add `checker` + `verifier` to `HelperContext`**

In `crates/boxpilotd/src/context.rs`:

```rust
use crate::profile::checker::SingboxChecker;
use crate::profile::verifier::ServiceVerifier;
```

Inside `HelperContext` add:

```rust
pub checker: Arc<dyn SingboxChecker>,
pub verifier: Arc<dyn ServiceVerifier>,
```

Update `HelperContext::new` signature to accept these (after `version_checker`). Update all three test helpers in `mod testing` to pass `Arc::new(crate::profile::checker::testing::FakeChecker::ok())` and `Arc::new(crate::profile::verifier::testing::ScriptedVerifier::new(vec![]))` (the existing tests do not exercise activate/rollback, so canned values are fine).

Update `crates/boxpilotd/src/main.rs`:

```rust
use crate::profile::checker::ProcessChecker;
use crate::profile::verifier::DefaultVerifier;
```

Pass `Arc::new(ProcessChecker)` and `Arc::new(DefaultVerifier)` into `HelperContext::new`.

- [ ] **Step 2: Replace `profile_activate_bundle` stub**

In `crates/boxpilotd/src/iface.rs`, replace the existing stub:

```rust
    async fn profile_activate_bundle(
        &self,
        #[zbus(header)] header: zbus::message::Header<'_>,
        request_json: String,
        bundle_fd: zbus::zvariant::OwnedFd,
    ) -> zbus::fdo::Result<String> {
        let sender = extract_sender(&header)?;
        let req: boxpilot_ipc::ActivateBundleRequest =
            serde_json::from_str(&request_json).map_err(|e| {
                zbus::fdo::Error::Failed(format!("app.boxpilot.Helper1.Ipc: parse: {e}"))
            })?;
        let resp = self
            .do_profile_activate_bundle(&sender, req, bundle_fd.into())
            .await
            .map_err(to_zbus_err)?;
        serde_json::to_string(&resp).map_err(|e| {
            zbus::fdo::Error::Failed(format!("app.boxpilot.Helper1.Ipc: serialize: {e}"))
        })
    }
```

`OwnedFd` from `zbus::zvariant::OwnedFd` converts to `std::os::fd::OwnedFd` via `.into()`; if the conversion is missing in your zbus version, use `OwnedFd::from(bundle_fd.into_inner())`. Confirm the path with `cargo check`.

- [ ] **Step 3: Replace `profile_rollback_release` stub**

```rust
    async fn profile_rollback_release(
        &self,
        #[zbus(header)] header: zbus::message::Header<'_>,
        request_json: String,
    ) -> zbus::fdo::Result<String> {
        let sender = extract_sender(&header)?;
        let req: boxpilot_ipc::RollbackRequest =
            serde_json::from_str(&request_json).map_err(|e| {
                zbus::fdo::Error::Failed(format!("app.boxpilot.Helper1.Ipc: parse: {e}"))
            })?;
        let resp = self
            .do_profile_rollback_release(&sender, req)
            .await
            .map_err(to_zbus_err)?;
        serde_json::to_string(&resp).map_err(|e| {
            zbus::fdo::Error::Failed(format!("app.boxpilot.Helper1.Ipc: serialize: {e}"))
        })
    }
```

- [ ] **Step 4: Add the `do_*` helpers in the bottom `impl Helper` block**

```rust
    async fn do_profile_activate_bundle(
        &self,
        sender: &str,
        req: boxpilot_ipc::ActivateBundleRequest,
        fd: std::os::fd::OwnedFd,
    ) -> Result<boxpilot_ipc::ActivateBundleResponse, HelperError> {
        let _call =
            dispatch::authorize(&self.ctx, sender, HelperMethod::ProfileActivateBundle).await?;
        let deps = crate::profile::activate::ActivateDeps {
            paths: self.ctx.paths.clone(),
            systemd: &*self.ctx.systemd,
            verifier: &*self.ctx.verifier,
            checker: &*self.ctx.checker,
        };
        crate::profile::activate::activate_bundle(req, fd, &deps).await
    }

    async fn do_profile_rollback_release(
        &self,
        sender: &str,
        req: boxpilot_ipc::RollbackRequest,
    ) -> Result<boxpilot_ipc::ActivateBundleResponse, HelperError> {
        let _call =
            dispatch::authorize(&self.ctx, sender, HelperMethod::ProfileRollbackRelease).await?;
        let deps = crate::profile::rollback::RollbackDeps {
            paths: self.ctx.paths.clone(),
            systemd: &*self.ctx.systemd,
            verifier: &*self.ctx.verifier,
        };
        crate::profile::rollback::rollback_release(req, &deps).await
    }
```

- [ ] **Step 5: Map new HelperError variants in `to_zbus_err`**

Add the new error names (mirroring existing pattern):

```rust
        HelperError::BundleTooLarge { .. } => "app.boxpilot.Helper1.BundleTooLarge",
        HelperError::BundleEntryRejected { .. } => "app.boxpilot.Helper1.BundleEntryRejected",
        HelperError::BundleAssetMismatch { .. } => "app.boxpilot.Helper1.BundleAssetMismatch",
        HelperError::SingboxCheckFailed { .. } => "app.boxpilot.Helper1.SingboxCheckFailed",
        HelperError::ActivationVerifyStuck { .. } => "app.boxpilot.Helper1.ActivationVerifyStuck",
        HelperError::RollbackTargetMissing => "app.boxpilot.Helper1.RollbackTargetMissing",
        HelperError::RollbackUnstartable { .. } => "app.boxpilot.Helper1.RollbackUnstartable",
        HelperError::ActiveCorrupt => "app.boxpilot.Helper1.ActiveCorrupt",
        HelperError::ReleaseAlreadyActive => "app.boxpilot.Helper1.ReleaseAlreadyActive",
        HelperError::ReleaseNotFound { .. } => "app.boxpilot.Helper1.ReleaseNotFound",
```

- [ ] **Step 6: Run workspace tests**

Run: `cargo test -p boxpilotd`
Expected: all green; no `dead_code` warnings on profile modules.

Run: `cargo clippy -p boxpilotd --all-targets -- -D warnings`
Expected: clean.

- [ ] **Step 7: Commit**

```bash
git add crates/boxpilotd/src/iface.rs crates/boxpilotd/src/context.rs crates/boxpilotd/src/main.rs
git commit -m "feat(plan-5): wire activate/rollback verbs into D-Bus iface

T12 of plan #5. profile_activate_bundle now takes (request_json, fd)
and dispatches to profile::activate; profile_rollback_release takes
request_json and dispatches to profile::rollback. HelperContext gains
checker + verifier dependencies; main wires ProcessChecker +
DefaultVerifier; tests use Fake/Scripted variants. New error variants
get reverse-DNS names in to_zbus_err.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

## Task 13: User-side `prepare_bundle` adds sealed memfd

**Files:**
- Modify: `crates/boxpilot-profile/src/bundle.rs`
- Modify: `crates/boxpilot-profile/Cargo.toml` (add `tar` dep)

- [ ] **Step 1: Add `tar` to boxpilot-profile**

Edit `crates/boxpilot-profile/Cargo.toml`, add under `[dependencies]`:

```toml
tar.workspace = true
```

- [ ] **Step 2: Failing test for memfd**

Append to `mod tests` in `crates/boxpilot-profile/src/bundle.rs`:

```rust
#[test]
fn prepare_bundle_returns_sealed_memfd_with_tar_layout() {
    use std::os::fd::AsRawFd;
    let (tmp, s) = fixture();
    let src = tmp.path().join("in.json");
    std::fs::write(&src, br#"{"log":{"level":"info"}}"#).unwrap();
    let m = crate::import::import_local_file(&s, &src, "P").unwrap();
    let b = prepare_bundle(&s, &m.id, "/p/sing-box", "1.10.0").unwrap();

    // Seal flags must include WRITE+GROW+SHRINK+SEAL.
    let seals = nix::fcntl::fcntl(b.memfd.as_raw_fd(), nix::fcntl::FcntlArg::F_GET_SEALS).unwrap();
    let mask = libc::F_SEAL_WRITE | libc::F_SEAL_GROW | libc::F_SEAL_SHRINK | libc::F_SEAL_SEAL;
    assert_eq!(seals & mask, mask, "all four seals must be set");

    // Tar must contain at minimum config.json and manifest.json.
    let mut file = std::fs::File::from(b.memfd.try_clone().unwrap());
    use std::io::Seek;
    file.seek(std::io::SeekFrom::Start(0)).unwrap();
    let mut archive = tar::Archive::new(&mut file);
    let names: Vec<String> = archive
        .entries()
        .unwrap()
        .filter_map(|e| e.ok().and_then(|e| e.path().ok().map(|p| p.to_string_lossy().into_owned())))
        .collect();
    assert!(names.iter().any(|n| n == "config.json"), "tar must contain config.json");
    assert!(names.iter().any(|n| n == "manifest.json"), "tar must contain manifest.json");
    assert!(b.tar_size > 0);
}
```

- [ ] **Step 3: Run, expect failure**

Run: `cargo test -p boxpilot-profile prepare_bundle_returns_sealed_memfd_with_tar_layout`
Expected: compile error (missing `memfd`, `tar_size`).

- [ ] **Step 4: Implement memfd seal in `prepare_bundle`**

In `crates/boxpilot-profile/src/bundle.rs`, after `let manifest_bytes = serde_json::to_vec_pretty(&manifest)…` and the existing `std::fs::write(staging_path.join("manifest.json"), &manifest_bytes)?`, add (still inside `prepare_bundle`):

```rust
    // Build the sealed memfd by tarring the staging directory we just
    // populated. Plain tar; no compression. This is the single artifact
    // boxpilotd unpacks (spec §9.2).
    let memfd = create_sealed_bundle_memfd(&staging_path)
        .map_err(|e| BundleError::Io(std::io::Error::other(format!("memfd build: {e}"))))?;
    let tar_size = nix::sys::stat::fstat(memfd.as_raw_fd())
        .map_err(|e| BundleError::Io(std::io::Error::other(format!("fstat memfd: {e}"))))?
        .st_size as u64;
```

Update the `PreparedBundle` struct:

```rust
pub struct PreparedBundle {
    pub staging: tempfile::TempDir,
    pub manifest: ActivationManifest,
    pub memfd: std::os::fd::OwnedFd,
    pub tar_size: u64,
}
```

Update the constructor:

```rust
    Ok(PreparedBundle { staging, manifest, memfd, tar_size })
```

Add the helper at the end of the file (above `mod tests`):

```rust
fn create_sealed_bundle_memfd(staging_root: &Path) -> std::io::Result<std::os::fd::OwnedFd> {
    use std::ffi::CString;
    use std::os::fd::AsRawFd;
    let raw = nix::sys::memfd::memfd_create(
        CString::new("boxpilot-bundle").unwrap().as_c_str(),
        nix::sys::memfd::MemFdCreateFlag::MFD_CLOEXEC
            | nix::sys::memfd::MemFdCreateFlag::MFD_ALLOW_SEALING,
    )
    .map_err(std::io::Error::from)?;
    let fd: std::os::fd::OwnedFd = raw;

    {
        let mut file = std::fs::File::from(fd.try_clone()?);
        let mut builder = tar::Builder::new(&mut file);
        builder.mode(tar::HeaderMode::Deterministic);
        // Walk in deterministic alphabetical order so manifest.json sha matches.
        append_dir_sorted(&mut builder, staging_root, Path::new(""))?;
        builder.finish()?;
    }

    let seals = libc::F_SEAL_WRITE | libc::F_SEAL_GROW | libc::F_SEAL_SHRINK | libc::F_SEAL_SEAL;
    nix::fcntl::fcntl(fd.as_raw_fd(), nix::fcntl::FcntlArg::F_ADD_SEALS(seals))
        .map_err(std::io::Error::from)?;
    Ok(fd)
}

fn append_dir_sorted(
    b: &mut tar::Builder<&mut std::fs::File>,
    abs_root: &Path,
    rel: &Path,
) -> std::io::Result<()> {
    let abs = abs_root.join(rel);
    let mut entries: Vec<_> = std::fs::read_dir(&abs)?
        .collect::<std::io::Result<_>>()?;
    entries.sort_by(|a, b| a.file_name().cmp(&b.file_name()));
    for e in entries {
        let path = e.path();
        let name = e.file_name();
        let rel_child = if rel.as_os_str().is_empty() {
            PathBuf::from(&name)
        } else {
            rel.join(&name)
        };
        let ft = std::fs::metadata(&path)?.file_type();
        if ft.is_dir() {
            let mut h = tar::Header::new_ustar();
            h.set_size(0);
            h.set_mode(0o700);
            h.set_entry_type(tar::EntryType::Directory);
            h.set_cksum();
            b.append_data(&mut h, &rel_child, std::io::empty())?;
            append_dir_sorted(b, abs_root, &rel_child)?;
        } else if ft.is_file() {
            let bytes = std::fs::read(&path)?;
            let mut h = tar::Header::new_ustar();
            h.set_size(bytes.len() as u64);
            h.set_mode(0o600);
            h.set_entry_type(tar::EntryType::Regular);
            h.set_cksum();
            b.append_data(&mut h, &rel_child, bytes.as_slice())?;
        }
    }
    Ok(())
}
```

Add `use std::os::fd::AsRawFd;` near the top of the file.

- [ ] **Step 5: Run tests**

Run: `cargo test -p boxpilot-profile prepare_bundle`
Expected: existing 4 tests + 1 new = pass.

Run: `cargo build -p boxpilot-profile`
Expected: clean.

- [ ] **Step 6: Commit**

```bash
git add crates/boxpilot-profile/src/bundle.rs crates/boxpilot-profile/Cargo.toml
git commit -m "feat(plan-5): prepare_bundle outputs sealed memfd of tar

T13 of plan #5. PreparedBundle gains memfd + tar_size. memfd_create
with MFD_ALLOW_SEALING, plain tar (deterministic order), then F_SEAL_
WRITE/GROW/SHRINK/SEAL. The TempDir staging stays for tests/debug.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

## Task 14: Tauri `profile_activate` command

**Files:**
- Modify: `crates/boxpilot-tauri/src/lib.rs`
- Modify: `crates/boxpilot-tauri/Cargo.toml`

- [ ] **Step 1: Add deps**

In `crates/boxpilot-tauri/Cargo.toml`:

```toml
nix.workspace = true
chrono.workspace = true
```

- [ ] **Step 2: Locate existing profile_* command structure**

Read `crates/boxpilot-tauri/src/lib.rs` to find the existing `profile_check` / `profile_prepare_bundle` commands from plan #4. The new command lives next to them.

- [ ] **Step 3: Add the command**

Add to `crates/boxpilot-tauri/src/lib.rs` (next to other profile_* commands):

```rust
#[derive(serde::Serialize)]
pub struct ActivateResult {
    pub outcome: String,
    pub activation_id: String,
    pub previous_activation_id: Option<String>,
}

#[tauri::command]
async fn profile_activate(
    profile_id: String,
    core_path: String,
    core_version: String,
    verify_window_secs: Option<u32>,
    state: tauri::State<'_, AppState>,
) -> Result<ActivateResult, String> {
    let store = state.profile_store.clone();
    let prepared = tokio::task::spawn_blocking(move || {
        boxpilot_profile::bundle::prepare_bundle(&store, &profile_id, &core_path, &core_version)
    })
    .await
    .map_err(|e| format!("join: {e}"))?
    .map_err(|e| format!("prepare: {e}"))?;

    let req = boxpilot_ipc::ActivateBundleRequest {
        verify_window_secs,
        expected_total_bytes: Some(prepared.tar_size),
    };
    let req_json = serde_json::to_string(&req).map_err(|e| format!("encode: {e}"))?;

    let conn = zbus::Connection::system().await.map_err(|e| format!("dbus connect: {e}"))?;
    let proxy = zbus::Proxy::new(
        &conn,
        "app.boxpilot.Helper",
        "/app/boxpilot/Helper",
        "app.boxpilot.Helper1",
    )
    .await
    .map_err(|e| format!("dbus proxy: {e}"))?;

    let fd_z: zbus::zvariant::OwnedFd = prepared.memfd.try_into().map_err(|e: std::io::Error| format!("fd: {e}"))?;
    let resp_json: String = proxy
        .call("ProfileActivateBundle", &(req_json, fd_z))
        .await
        .map_err(|e| format!("dbus call: {e}"))?;
    let resp: boxpilot_ipc::ActivateBundleResponse =
        serde_json::from_str(&resp_json).map_err(|e| format!("decode: {e}"))?;
    let outcome = match resp.outcome {
        boxpilot_ipc::ActivateOutcome::Active => "active",
        boxpilot_ipc::ActivateOutcome::RolledBack => "rolled_back",
        boxpilot_ipc::ActivateOutcome::RollbackTargetMissing => "rollback_target_missing",
        boxpilot_ipc::ActivateOutcome::RollbackUnstartable => "rollback_unstartable",
    }
    .to_string();
    Ok(ActivateResult {
        outcome,
        activation_id: resp.activation_id,
        previous_activation_id: resp.previous_activation_id,
    })
}
```

Then register the command in the `tauri::Builder::default().invoke_handler(...)` call alongside the existing profile_* invocations.

If `OwnedFd::try_into::<zbus::zvariant::OwnedFd>` does not exist in your zbus version, use `zbus::zvariant::OwnedFd::from(prepared.memfd)`.

- [ ] **Step 4: Build**

Run: `cargo build -p boxpilot`
Expected: clean.

Run: `cargo test -p boxpilot --no-run`
Expected: clean.

- [ ] **Step 5: Commit**

```bash
git add crates/boxpilot-tauri/
git commit -m "feat(plan-5): tauri profile_activate command

T14 of plan #5. Tauri command builds the bundle (prepare_bundle),
opens the system D-Bus, invokes ProfileActivateBundle with the JSON
request body and the sealed memfd, and returns the typed outcome to
the GUI. Plan #7 will wire UI affordances; this task ships the
backend bridge only.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

## Task 15: Smoke procedure doc

**Files:**
- Create: `docs/superpowers/plans/2026-04-30-activation-pipeline-smoke-procedure.md`

- [ ] **Step 1: Author doc**

Write the file:

```markdown
# Plan #5 — Activation Pipeline Smoke Procedure

Run on a Debian/Ubuntu desktop (or VM) with systemd. Requires
`sudo`, `gdbus`, `python3`, `tar`, and a built `boxpilotd`.

## 1. Pre-flight (one time)

- Build: `cargo build --workspace --release`
- Install daemon: `sudo install -m 0755 target/release/boxpilotd /usr/local/libexec/boxpilotd`
- Install the D-Bus service file + polkit policy + helper rules from `packaging/linux/`.
- Start daemon manually: `sudo /usr/local/libexec/boxpilotd` (in another terminal).

## 2. Plan #2/#3 prerequisites

- Adopt or install a sing-box core: `gdbus call ... CoreInstallManaged ...`.
- Install the managed unit: `gdbus call ... ServiceInstallManaged`.

## 3. Build a test bundle

Use the helper Python script (or write one inline) to:

1. Write `config.json`, `assets/`, and `manifest.json` into a temp dir.
2. Tar that directory into a memfd (Python `tarfile` to stdout, then dup into a memfd via `os.memfd_create`).
3. Pass the memfd to `gdbus call --system ... --object-path /app/boxpilot/Helper --method app.boxpilot.Helper1.ProfileActivateBundle '"{}"' h:<fd>`.

Concrete script: `scripts/smoke/plan-5-activate.py` (write inline — committed alongside this doc, kept simple, no third-party deps).

## 4. Verify happy path

After the call returns:

- `readlink /etc/boxpilot/active` resolves under `/etc/boxpilot/releases/`.
- `systemctl is-active boxpilot-sing-box.service` reports `active`.
- `cat /etc/boxpilot/boxpilot.toml` shows updated `active_release_id` and (if not first activation) `previous_release_id`.

## 5. Verify rollback paths

a. Build a bundle with an intentionally broken config (`outbounds: 0` instead of an array). Activate it. Expect `outcome=rolled_back`. Verify `active` still points at the previous release.

b. Pre-populate two activations, then `rm -rf` the previous on disk and trigger another bad-config activation. Expect `outcome=rollback_target_missing` and `systemctl is-active … = inactive`.

c. To exercise `rollback_unstartable`: corrupt the previous release's `config.json`, then activate a bad new bundle. Expect that outcome and service stopped.

## 6. Verify GC

Run 12 successful activations in a row (each tweaks `log.level`). After: `ls /etc/boxpilot/releases/ | wc -l` should be ≤ 11 (10 + active + previous, minus dupes = bounded). Confirm `boxpilot.toml` still tracks the right pair.

## 7. Crash recovery

Kill the daemon during step 6's middle activation (between unpack and rename). Restart `boxpilotd`. Confirm `.staging/` is empty and the next activation succeeds normally.
```

- [ ] **Step 2: Commit**

```bash
git add docs/superpowers/plans/2026-04-30-activation-pipeline-smoke-procedure.md
git commit -m "docs(plan-5): activation pipeline smoke procedure

T15 of plan #5. Operator-facing checklist for verifying happy path,
all three rollback outcomes, GC retention, and crash recovery on a
real systemd desktop. Helper Python is to be written ad-hoc for the
smoke run; not committed alongside.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

## Final verification

- [ ] **Step 1: Workspace tests + clippy + fmt**

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --no-fail-fast
```

Expected: clean across all three. Test count up by ≥60.

- [ ] **Step 2: Frontend build (regression check)**

```bash
cd frontend && npm install && npm run build && cd -
```

Expected: clean (Plan #5 added no UI surface; build only matters as a regression gate).

- [ ] **Step 3: Done.** Hand off to finishing-a-development-branch skill.

