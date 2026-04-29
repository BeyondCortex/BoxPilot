# BoxPilot Profile Store + Editor Implementation Plan (Plan #4)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement the user-side profile store described in §5.6, the profile editor data layer from §9.1, and the activation-bundle composer from §9.2 / §9.3 — i.e. everything that lives in `~/.local/share/boxpilot/` and runs as the desktop user. Plan #4 ends with a `prepare_bundle()` function that produces a constrained `(config.json + assets/ + manifest.json)` staging directory ready for plan #5 to seal onto a memfd and hand to `boxpilotd`.

**Architecture:** A new workspace crate `boxpilot-profile` owns all user-side filesystem logic so its tests do not depend on the Tauri runtime. The crate is layered: `store` (paths + permissioned IO) → `meta` / `remotes` / `ui_state` (per-file schemas) → `import` / `remote` (entry points that produce profiles) → `editor` (sparse JSON patch over `serde_json::Value`, preserving unknown fields) → `asset_check` (walks `config.json` to enumerate referenced asset paths and refuse non-system absolute paths per §9.3) → `bundle` (composes the §9.2 staging directory with size-limit enforcement and computes the §9.2 manifest). `ActivationManifest` itself lives in `boxpilot-ipc::profile` so plan #5's daemon-side unpacker decodes the same struct. URL redaction is sing-box-schema-aware *only* at the URL layer (full schema-aware redaction for diagnostics is plan #8); plan #4 ships `redact_url_for_display` to satisfy the §14 subscription-URL split. Tauri exposes 11 typed commands; the frontend gets a minimal `ProfilesPanel.vue` (list / import-file / import-dir / add-remote / edit JSON / prepare-bundle preview) — structured TUN editing and the rich overview are plan #7.

**Tech Stack:** Rust 2021, existing workspace deps from plans #1–#3 (`serde` / `serde_json` / `tokio` / `reqwest` / `sha2` / `chrono` / `nix` / `tempfile` / `tracing` / `thiserror` / `async-trait`). Adds one workspace dep — `url = "2"` — for safe query-string parsing in URL redaction. Frontend stays on Vue 3 + TS + Vite (no new deps).

**Worktree:** Branch from `main` once plan #3 is merged (`git worktree add .worktrees/profile-store -b profile-store main`). All commits below land on the `profile-store` branch.

**Out of scope (deferred):**
- The actual `profile.activate_bundle` / `profile.rollback_release` IPC calls and daemon-side bundle unpacking, lock acquisition, atomic release-rename, and verification (§10 steps 5–15) → plan #5. Plan #4's bundle ends as a `tempfile::TempDir` next to the `ActivationManifest`; plan #5 owns sealing it onto a memfd and the round-trip.
- Existing `sing-box.service` observation / migration (`legacy.*`) → plan #6. The `import_legacy_unit_config(...)` helper that plan #6 needs is intentionally absent.
- Structured overview of inbounds / outbounds / DNS / route / rule sets, structured TUN patch UI, "patch common TUN fields from structured controls" — these are §3.2 UI features → plan #7. Plan #4's `editor::apply_patch` is the data layer plan #7 will consume; the GUI in this plan is intentionally minimal (textarea + Save).
- Schema-aware redaction of profile JSON for diagnostics (`outbounds[*].password`, etc. per §14) → plan #8. Plan #4 only ships `redact::redact_url_for_display` because it is required *now* for the subscription-URL split (§14: full URL never written system-side).
- `experimental.clash_api` enable/patch flow (§3.3, §12) → plan #7.
- §6.3 whitelist stays at 19 methods. **Do NOT modify `boxpilot_ipc::method::HelperMethod::ALL` count.** Plan #4 adds zero helper methods — every new Tauri command runs in-process as the desktop user.

---

## File Structure

```
crates/boxpilot-ipc/src/
  profile.rs                       # NEW — ActivationManifest + bundle limits + AssetEntry + SourceKind
  lib.rs                           # MODIFY — pub mod profile; pub use profile::*

crates/boxpilot-profile/           # NEW WORKSPACE MEMBER
  Cargo.toml                       # NEW
  src/
    lib.rs                         # NEW — re-exports + integration test
    store.rs                       # NEW — paths + permissioned IO (0700 dirs, 0600 files)
    meta.rs                        # NEW — ProfileMetadata schema + atomic write
    remotes.rs                     # NEW — RemotesFile (full URL bearer; 0600)
    ui_state.rs                    # NEW — UiState (last selected tab/profile etc.)
    redact.rs                      # NEW — redact_url_for_display
    list.rs                        # NEW — ProfileStore::list / get
    import.rs                      # NEW — import_local_file / import_local_dir
    remote.rs                      # NEW — RemoteFetcher trait + ReqwestFetcher + FixedFetcher
    editor.rs                      # NEW — apply_patch + ensure_object_root
    asset_check.rs                 # NEW — extract_asset_refs + detect_absolute_paths
    bundle.rs                      # NEW — prepare_bundle + size-limit enforcement
    snapshot.rs                    # NEW — last-valid/ snapshot + revert_to_last_valid
    check.rs                       # NEW — best-effort sing-box check (spawn external core)

crates/boxpilot-tauri/src/
  commands.rs                      # MODIFY — 11 new #[tauri::command] wrappers
  lib.rs                           # MODIFY — register new commands in invoke_handler!
  Cargo.toml                       # MODIFY — depend on boxpilot-profile

frontend/src/
  api/types.ts                     # MODIFY — TS mirrors of new request/response types
  api/profile.ts                   # NEW — invoke wrappers for the 11 new commands
  components/ProfilesPanel.vue     # NEW — list / import / add remote / edit / prepare-bundle preview
  App.vue                          # MODIFY — add Profiles tab

Cargo.toml                         # MODIFY — add `url = "2"` to workspace.dependencies; add new member

docs/superpowers/plans/
  2026-04-30-profile-store-smoke-procedure.md   # NEW — manual smoke run on a real desktop
```

---

## Task 1: New `boxpilot-profile` crate skeleton + workspace wiring

**Files:**
- Create: `crates/boxpilot-profile/Cargo.toml`
- Create: `crates/boxpilot-profile/src/lib.rs`
- Modify: `Cargo.toml` (workspace members + workspace.dependencies)

The new crate compiles before any module is added so dependent crates can wire it up early. `url = "2"` is added to workspace deps now (used by `redact.rs` in Task 8).

- [ ] **Step 1: Add the workspace member and the `url` dep**

Modify `Cargo.toml`:

```toml
[workspace]
resolver = "2"
members = [
    "crates/boxpilot-ipc",
    "crates/boxpilotd",
    "crates/boxpilot-tauri",
    "crates/boxpilot-profile",
]
```

Add to `[workspace.dependencies]` (alphabetical after `tracing-subscriber`):

```toml
url = "2"
```

- [ ] **Step 2: Create the crate Cargo.toml**

Create `crates/boxpilot-profile/Cargo.toml`:

```toml
[package]
name = "boxpilot-profile"
version = "0.1.0"
edition.workspace = true
rust-version.workspace = true
license.workspace = true

[dependencies]
boxpilot-ipc = { path = "../boxpilot-ipc" }
serde.workspace = true
serde_json.workspace = true
thiserror.workspace = true
tracing.workspace = true
tokio = { workspace = true }
chrono = { workspace = true }
sha2.workspace = true
hex.workspace = true
nix = { workspace = true }
tempfile.workspace = true
reqwest = { workspace = true }
async-trait.workspace = true
url.workspace = true

[dev-dependencies]
pretty_assertions.workspace = true
```

- [ ] **Step 3: Create the lib root**

Create `crates/boxpilot-profile/src/lib.rs`:

```rust
//! User-side profile store, editor, and activation-bundle composer.
//!
//! Everything in this crate runs as the desktop user — never as root.
//! It owns `~/.local/share/boxpilot/` per spec §5.6 and produces the
//! constrained activation bundle described in §9.2 for plan #5 to
//! transfer to `boxpilotd`.

#[cfg(test)]
mod sanity {
    #[test]
    fn crate_compiles() {
        assert_eq!(2 + 2, 4);
    }
}
```

- [ ] **Step 4: Verify the workspace builds**

Run: `cargo build --workspace`
Expected: clean build; new crate compiles with one passing test.

Run: `cargo test -p boxpilot-profile`
Expected: PASS (`crate_compiles`).

- [ ] **Step 5: Commit**

```bash
git add Cargo.toml crates/boxpilot-profile/
git commit -m "feat(profile): boxpilot-profile crate skeleton"
```

---

## Task 2: ActivationManifest IPC types

**Files:**
- Create: `crates/boxpilot-ipc/src/profile.rs`
- Modify: `crates/boxpilot-ipc/src/lib.rs`

Spec §9.2 defines manifest.json v1.0 plus four bundle limits. These types are shared between the user-side composer (this plan) and the daemon-side unpacker (plan #5), so they live in `boxpilot-ipc`. `SourceKind`'s wire form uses the lowercase strings the spec literally shows: `"local" | "local-dir" | "remote"`.

- [ ] **Step 1: Write the failing tests (round-trip + literal wire form)**

Create `crates/boxpilot-ipc/src/profile.rs`:

```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AssetEntry {
    pub path: String,
    pub sha256: String,
    pub size: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SourceKind {
    #[serde(rename = "local")]
    Local,
    #[serde(rename = "local-dir")]
    LocalDir,
    #[serde(rename = "remote")]
    Remote,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActivationManifest {
    pub schema_version: u32,
    pub activation_id: String,
    pub profile_id: String,
    pub profile_sha256: String,
    pub config_sha256: String,
    pub source_kind: SourceKind,
    /// Always present for `Remote`, always `None` for `Local` / `LocalDir`.
    pub source_url_redacted: Option<String>,
    pub core_path_at_activation: String,
    pub core_version_at_activation: String,
    /// RFC3339 with timezone (matches plan #2 install-state timestamps).
    pub created_at: String,
    pub assets: Vec<AssetEntry>,
}

pub const ACTIVATION_MANIFEST_SCHEMA_VERSION: u32 = 1;

/// Spec §9.2 default size limits.
pub const BUNDLE_MAX_FILE_BYTES: u64 = 16 * 1024 * 1024;
pub const BUNDLE_MAX_TOTAL_BYTES: u64 = 64 * 1024 * 1024;
pub const BUNDLE_MAX_FILE_COUNT: u32 = 1024;
pub const BUNDLE_MAX_NESTING_DEPTH: u32 = 8;

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn source_kind_wire_form_matches_spec() {
        assert_eq!(serde_json::to_string(&SourceKind::Local).unwrap(), "\"local\"");
        assert_eq!(serde_json::to_string(&SourceKind::LocalDir).unwrap(), "\"local-dir\"");
        assert_eq!(serde_json::to_string(&SourceKind::Remote).unwrap(), "\"remote\"");
    }

    #[test]
    fn activation_manifest_round_trips() {
        let m = ActivationManifest {
            schema_version: ACTIVATION_MANIFEST_SCHEMA_VERSION,
            activation_id: "2026-04-30T00-00-00Z-abc123".into(),
            profile_id: "profile-id".into(),
            profile_sha256: "deadbeef".into(),
            config_sha256: "cafebabe".into(),
            source_kind: SourceKind::Remote,
            source_url_redacted: Some("https://host/path?token=***".into()),
            core_path_at_activation: "/var/lib/boxpilot/cores/current/sing-box".into(),
            core_version_at_activation: "1.10.0".into(),
            created_at: "2026-04-30T00:00:00-07:00".into(),
            assets: vec![AssetEntry {
                path: "geosite.db".into(),
                sha256: "abc".into(),
                size: 12345,
            }],
        };
        let s = serde_json::to_string(&m).unwrap();
        let back: ActivationManifest = serde_json::from_str(&s).unwrap();
        assert_eq!(back, m);
    }

    #[test]
    fn limits_match_spec_defaults() {
        assert_eq!(BUNDLE_MAX_FILE_BYTES, 16 * 1024 * 1024);
        assert_eq!(BUNDLE_MAX_TOTAL_BYTES, 64 * 1024 * 1024);
        assert_eq!(BUNDLE_MAX_FILE_COUNT, 1024);
        assert_eq!(BUNDLE_MAX_NESTING_DEPTH, 8);
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail (compile error: module missing from lib.rs)**

Run: `cargo test -p boxpilot-ipc profile::tests`
Expected: FAIL — module `profile` not declared.

- [ ] **Step 3: Wire the module into `lib.rs`**

Modify `crates/boxpilot-ipc/src/lib.rs` — append after the `service` block:

```rust
pub mod profile;
pub use profile::{
    ActivationManifest, AssetEntry, SourceKind, ACTIVATION_MANIFEST_SCHEMA_VERSION,
    BUNDLE_MAX_FILE_BYTES, BUNDLE_MAX_FILE_COUNT, BUNDLE_MAX_NESTING_DEPTH,
    BUNDLE_MAX_TOTAL_BYTES,
};
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p boxpilot-ipc profile::tests`
Expected: PASS (3 tests).

- [ ] **Step 5: Commit**

```bash
git add crates/boxpilot-ipc/src/profile.rs crates/boxpilot-ipc/src/lib.rs
git commit -m "feat(ipc): activation-manifest types + §9.2 bundle limits"
```

---

## Task 3: Profile-store paths + permissioned IO

**Files:**
- Create: `crates/boxpilot-profile/src/store.rs`
- Modify: `crates/boxpilot-profile/src/lib.rs`

`ProfileStorePaths` is a pure value type — no fs access in its constructors so tests construct it from a `tempfile::TempDir`. Two helpers (`ensure_dir_0700` and `write_file_0600_atomic`) encapsulate the §5.6 `0700` / `0600` requirements and the standard `tmp + rename` atomic write.

- [ ] **Step 1: Write the failing tests**

Create `crates/boxpilot-profile/src/store.rs`:

```rust
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct ProfileStorePaths {
    root: PathBuf,
}

impl ProfileStorePaths {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    /// Compute the spec-mandated `~/.local/share/boxpilot/` path from the
    /// environment. Honours `XDG_DATA_HOME`, falls back to `$HOME/.local/share`.
    pub fn from_env() -> std::io::Result<Self> {
        let base = if let Some(v) = std::env::var_os("XDG_DATA_HOME") {
            PathBuf::from(v)
        } else if let Some(h) = std::env::var_os("HOME") {
            PathBuf::from(h).join(".local/share")
        } else {
            return Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "neither XDG_DATA_HOME nor HOME is set",
            ));
        };
        Ok(Self::new(base.join("boxpilot")))
    }

    pub fn root(&self) -> &Path { &self.root }
    pub fn profiles_dir(&self) -> PathBuf { self.root.join("profiles") }
    pub fn profile_dir(&self, id: &str) -> PathBuf { self.profiles_dir().join(id) }
    pub fn profile_source(&self, id: &str) -> PathBuf { self.profile_dir(id).join("source.json") }
    pub fn profile_assets_dir(&self, id: &str) -> PathBuf { self.profile_dir(id).join("assets") }
    pub fn profile_metadata(&self, id: &str) -> PathBuf { self.profile_dir(id).join("metadata.json") }
    pub fn profile_last_valid_dir(&self, id: &str) -> PathBuf { self.profile_dir(id).join("last-valid") }
    pub fn profile_last_valid_config(&self, id: &str) -> PathBuf { self.profile_last_valid_dir(id).join("config.json") }
    pub fn profile_last_valid_assets_dir(&self, id: &str) -> PathBuf { self.profile_last_valid_dir(id).join("assets") }
    pub fn remotes_json(&self) -> PathBuf { self.root.join("remotes.json") }
    pub fn ui_state_json(&self) -> PathBuf { self.root.join("ui-state.json") }
}

/// Idempotent: creates `path` (and parents) if missing, then forces `0700`.
pub fn ensure_dir_0700(path: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(path)?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))
}

/// Atomic write with `0600` mode. Writes to `path.tmp`, fsyncs, renames.
pub fn write_file_0600_atomic(path: &Path, contents: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)?;
        }
    }
    let tmp = path.with_extension("tmp");
    {
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&tmp)?;
        f.write_all(contents)?;
        f.sync_all()?;
    }
    std::fs::rename(&tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn paths_layout_matches_spec_5_6() {
        let p = ProfileStorePaths::new(PathBuf::from("/x"));
        assert_eq!(p.profiles_dir(), PathBuf::from("/x/profiles"));
        assert_eq!(p.profile_dir("abc"), PathBuf::from("/x/profiles/abc"));
        assert_eq!(p.profile_source("abc"), PathBuf::from("/x/profiles/abc/source.json"));
        assert_eq!(p.profile_assets_dir("abc"), PathBuf::from("/x/profiles/abc/assets"));
        assert_eq!(p.profile_metadata("abc"), PathBuf::from("/x/profiles/abc/metadata.json"));
        assert_eq!(p.profile_last_valid_config("abc"), PathBuf::from("/x/profiles/abc/last-valid/config.json"));
        assert_eq!(p.profile_last_valid_assets_dir("abc"), PathBuf::from("/x/profiles/abc/last-valid/assets"));
        assert_eq!(p.remotes_json(), PathBuf::from("/x/remotes.json"));
        assert_eq!(p.ui_state_json(), PathBuf::from("/x/ui-state.json"));
    }

    #[test]
    fn ensure_dir_0700_creates_and_chmods() {
        let tmp = tempfile::tempdir().unwrap();
        let target = tmp.path().join("a/b/c");
        ensure_dir_0700(&target).unwrap();
        let mode = std::fs::metadata(&target).unwrap().permissions().mode() & 0o7777;
        assert_eq!(mode, 0o700);
    }

    #[test]
    fn ensure_dir_0700_is_idempotent_and_repairs_perms() {
        let tmp = tempfile::tempdir().unwrap();
        let target = tmp.path().join("d");
        std::fs::create_dir_all(&target).unwrap();
        std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o755)).unwrap();
        ensure_dir_0700(&target).unwrap();
        let mode = std::fs::metadata(&target).unwrap().permissions().mode() & 0o7777;
        assert_eq!(mode, 0o700);
    }

    #[test]
    fn write_file_0600_atomic_creates_with_correct_mode() {
        let tmp = tempfile::tempdir().unwrap();
        let target = tmp.path().join("nested/secret.json");
        write_file_0600_atomic(&target, b"{}").unwrap();
        let mode = std::fs::metadata(&target).unwrap().permissions().mode() & 0o7777;
        assert_eq!(mode, 0o600);
        assert_eq!(std::fs::read(&target).unwrap(), b"{}");
        // The .tmp sidecar must have been renamed away.
        assert!(!target.with_extension("tmp").exists());
    }
}
```

- [ ] **Step 2: Wire into lib.rs**

Replace the contents of `crates/boxpilot-profile/src/lib.rs` with:

```rust
//! User-side profile store, editor, and activation-bundle composer.

pub mod store;
pub use store::{ensure_dir_0700, write_file_0600_atomic, ProfileStorePaths};

#[cfg(test)]
mod sanity {
    #[test]
    fn crate_compiles() {
        assert_eq!(2 + 2, 4);
    }
}
```

- [ ] **Step 3: Run the tests**

Run: `cargo test -p boxpilot-profile store::tests`
Expected: PASS (4 tests).

- [ ] **Step 4: Commit**

```bash
git add crates/boxpilot-profile/src/store.rs crates/boxpilot-profile/src/lib.rs
git commit -m "feat(profile): store paths + permissioned IO helpers"
```

---

## Task 4: ProfileMetadata schema + atomic write

**Files:**
- Create: `crates/boxpilot-profile/src/meta.rs`
- Modify: `crates/boxpilot-profile/src/lib.rs`

Each profile dir holds a single `metadata.json` describing its provenance plus the most recent good activation. `last_valid_activation_id` is `None` until the first successful activation (filled in by plan #5 — plan #4 only writes the `None` shape). `config_sha256` is recomputed from `source.json` on every save so the GUI can trust it without re-hashing.

- [ ] **Step 1: Write the failing tests**

Create `crates/boxpilot-profile/src/meta.rs`:

```rust
use boxpilot_ipc::SourceKind;
use serde::{Deserialize, Serialize};
use std::path::Path;

use crate::store::write_file_0600_atomic;

pub const METADATA_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProfileMetadata {
    pub schema_version: u32,
    pub id: String,
    pub name: String,
    pub source_kind: SourceKind,
    /// Set only when `source_kind == Remote`; key into `remotes.json`.
    pub remote_id: Option<String>,
    pub created_at: String, // RFC3339
    pub updated_at: String, // RFC3339
    pub last_valid_activation_id: Option<String>,
    /// SHA-256 hex of the bytes currently on disk in `source.json`.
    pub config_sha256: String,
}

impl ProfileMetadata {
    pub fn new_local(id: &str, name: &str, now_rfc3339: &str, config_sha256: &str) -> Self {
        Self {
            schema_version: METADATA_SCHEMA_VERSION,
            id: id.to_string(),
            name: name.to_string(),
            source_kind: SourceKind::Local,
            remote_id: None,
            created_at: now_rfc3339.to_string(),
            updated_at: now_rfc3339.to_string(),
            last_valid_activation_id: None,
            config_sha256: config_sha256.to_string(),
        }
    }
}

pub fn read_metadata(path: &Path) -> std::io::Result<ProfileMetadata> {
    let bytes = std::fs::read(path)?;
    serde_json::from_slice(&bytes)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
}

pub fn write_metadata(path: &Path, meta: &ProfileMetadata) -> std::io::Result<()> {
    let bytes = serde_json::to_vec_pretty(meta)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    write_file_0600_atomic(path, &bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn round_trip_local() {
        let m = ProfileMetadata::new_local("p1", "My Profile", "2026-04-30T00:00:00-07:00", "abc");
        let s = serde_json::to_string(&m).unwrap();
        let back: ProfileMetadata = serde_json::from_str(&s).unwrap();
        assert_eq!(back, m);
        assert!(matches!(back.source_kind, SourceKind::Local));
        assert!(back.remote_id.is_none());
    }

    #[test]
    fn write_then_read() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("metadata.json");
        let m = ProfileMetadata::new_local("p1", "n", "t", "h");
        write_metadata(&path, &m).unwrap();
        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o7777;
        assert_eq!(mode, 0o600);
        assert_eq!(read_metadata(&path).unwrap(), m);
    }
}
```

- [ ] **Step 2: Wire into lib.rs**

Edit `crates/boxpilot-profile/src/lib.rs`:

```rust
pub mod store;
pub use store::{ensure_dir_0700, write_file_0600_atomic, ProfileStorePaths};

pub mod meta;
pub use meta::{read_metadata, write_metadata, ProfileMetadata, METADATA_SCHEMA_VERSION};
```

- [ ] **Step 3: Run the tests**

Run: `cargo test -p boxpilot-profile meta::tests`
Expected: PASS (2 tests).

- [ ] **Step 4: Commit**

```bash
git add crates/boxpilot-profile/src/meta.rs crates/boxpilot-profile/src/lib.rs
git commit -m "feat(profile): metadata schema + atomic write"
```

---

## Task 5: ProfileStore listing + read

**Files:**
- Create: `crates/boxpilot-profile/src/list.rs`
- Modify: `crates/boxpilot-profile/src/lib.rs`

`ProfileStore` is the entry point the rest of the crate (and the Tauri commands) consume. It owns a `ProfileStorePaths` and exposes `list()` (returns `Vec<ProfileMetadata>` sorted by `updated_at` desc) and `get(id)`. Listing is best-effort — a directory under `profiles/` with a missing or corrupt `metadata.json` is logged and skipped, never fatal, so one bad profile cannot brick the GUI.

- [ ] **Step 1: Write the failing tests**

Create `crates/boxpilot-profile/src/list.rs`:

```rust
use std::path::PathBuf;

use crate::meta::{read_metadata, ProfileMetadata};
use crate::store::ProfileStorePaths;

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("profile {0} not found")]
    NotFound(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

#[derive(Debug, Clone)]
pub struct ProfileStore {
    paths: ProfileStorePaths,
}

impl ProfileStore {
    pub fn new(paths: ProfileStorePaths) -> Self {
        Self { paths }
    }

    pub fn paths(&self) -> &ProfileStorePaths { &self.paths }

    pub fn list(&self) -> Result<Vec<ProfileMetadata>, StoreError> {
        let dir = self.paths.profiles_dir();
        let read = match std::fs::read_dir(&dir) {
            Ok(r) => r,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(StoreError::Io(e)),
        };
        let mut out = Vec::new();
        for entry in read {
            let entry = match entry { Ok(e) => e, Err(_) => continue };
            if !entry.file_type().map(|t| t.is_dir()).unwrap_or(false) { continue; }
            let id = match entry.file_name().to_str() { Some(s) => s.to_string(), None => continue };
            let meta_path = self.paths.profile_metadata(&id);
            match read_metadata(&meta_path) {
                Ok(m) => out.push(m),
                Err(e) => tracing::warn!(profile_id = %id, error = %e, "skipping profile with unreadable metadata"),
            }
        }
        out.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        Ok(out)
    }

    pub fn get(&self, id: &str) -> Result<ProfileMetadata, StoreError> {
        let meta_path = self.paths.profile_metadata(id);
        if !meta_path.exists() {
            return Err(StoreError::NotFound(id.to_string()));
        }
        Ok(read_metadata(&meta_path)?)
    }

    pub fn read_source_bytes(&self, id: &str) -> Result<Vec<u8>, StoreError> {
        let p = self.paths.profile_source(id);
        if !p.exists() { return Err(StoreError::NotFound(id.to_string())); }
        Ok(std::fs::read(&p)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::meta::{write_metadata, ProfileMetadata};
    use pretty_assertions::assert_eq;

    fn fixture() -> (tempfile::TempDir, ProfileStore) {
        let tmp = tempfile::tempdir().unwrap();
        let store = ProfileStore::new(ProfileStorePaths::new(tmp.path().to_path_buf()));
        (tmp, store)
    }

    fn put(store: &ProfileStore, id: &str, name: &str, ts: &str) {
        let dir = store.paths().profile_dir(id);
        std::fs::create_dir_all(&dir).unwrap();
        let mut m = ProfileMetadata::new_local(id, name, ts, "h");
        m.updated_at = ts.into();
        write_metadata(&store.paths().profile_metadata(id), &m).unwrap();
    }

    #[test]
    fn list_empty_when_dir_missing() {
        let (_t, s) = fixture();
        assert!(s.list().unwrap().is_empty());
    }

    #[test]
    fn list_sorts_by_updated_at_desc() {
        let (_t, s) = fixture();
        put(&s, "a", "A", "2026-04-29T00:00:00-07:00");
        put(&s, "b", "B", "2026-04-30T00:00:00-07:00");
        put(&s, "c", "C", "2026-04-28T00:00:00-07:00");
        let ids: Vec<_> = s.list().unwrap().into_iter().map(|m| m.id).collect();
        assert_eq!(ids, vec!["b", "a", "c"]);
    }

    #[test]
    fn list_skips_bad_metadata_without_failing() {
        let (_t, s) = fixture();
        put(&s, "good", "g", "2026-04-30T00:00:00-07:00");
        let bad = s.paths().profile_dir("bad");
        std::fs::create_dir_all(&bad).unwrap();
        std::fs::write(s.paths().profile_metadata("bad"), b"{not json").unwrap();
        let ids: Vec<_> = s.list().unwrap().into_iter().map(|m| m.id).collect();
        assert_eq!(ids, vec!["good"]);
    }

    #[test]
    fn get_returns_not_found_for_unknown_id() {
        let (_t, s) = fixture();
        assert!(matches!(s.get("missing"), Err(StoreError::NotFound(_))));
    }
}
```

- [ ] **Step 2: Wire into lib.rs**

Edit `crates/boxpilot-profile/src/lib.rs`:

```rust
pub mod list;
pub use list::{ProfileStore, StoreError};
```

- [ ] **Step 3: Run the tests**

Run: `cargo test -p boxpilot-profile list::tests`
Expected: PASS (4 tests).

- [ ] **Step 4: Commit**

```bash
git add crates/boxpilot-profile/src/list.rs crates/boxpilot-profile/src/lib.rs
git commit -m "feat(profile): ProfileStore::list + get"
```

---

## Task 6: `remotes.json` with subscription-URL split (§14)

**Files:**
- Create: `crates/boxpilot-profile/src/remotes.rs`
- Modify: `crates/boxpilot-profile/src/lib.rs`

§14 mandates: full URL with tokens lives **only** in `~/.local/share/boxpilot/remotes.json` (`0600`); system-side never sees it. This module owns that file. `remote_id` is a content-addressed slug derived from the URL — stable across remote-edit operations as long as the URL doesn't change. Reads tolerate a missing file (returns empty); writes go through `write_file_0600_atomic`.

- [ ] **Step 1: Write the failing tests**

Create `crates/boxpilot-profile/src/remotes.rs`:

```rust
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::Path;

use crate::store::write_file_0600_atomic;

pub const REMOTES_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoteEntry {
    /// Full URL with tokens. NEVER replicated to /etc/boxpilot.
    pub url: String,
    pub last_fetched_at: Option<String>,
    pub last_etag: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemotesFile {
    pub schema_version: u32,
    #[serde(default)]
    pub remotes: BTreeMap<String, RemoteEntry>,
}

impl Default for RemotesFile {
    fn default() -> Self {
        Self { schema_version: REMOTES_SCHEMA_VERSION, remotes: BTreeMap::new() }
    }
}

pub fn read_remotes(path: &Path) -> std::io::Result<RemotesFile> {
    match std::fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(RemotesFile::default()),
        Err(e) => Err(e),
    }
}

pub fn write_remotes(path: &Path, file: &RemotesFile) -> std::io::Result<()> {
    let bytes = serde_json::to_vec_pretty(file)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    write_file_0600_atomic(path, &bytes)
}

/// Stable content-addressed remote id. Identical URL → identical id, so
/// re-adding the same URL is idempotent.
pub fn remote_id_for_url(url: &str) -> String {
    let mut h = Sha256::new();
    h.update(url.as_bytes());
    let digest = hex::encode(h.finalize());
    format!("r-{}", &digest[..16])
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn round_trip_default_and_populated() {
        let mut f = RemotesFile::default();
        f.remotes.insert(
            "r-abc".into(),
            RemoteEntry { url: "https://x?token=t".into(), last_fetched_at: None, last_etag: None },
        );
        let s = serde_json::to_string(&f).unwrap();
        let back: RemotesFile = serde_json::from_str(&s).unwrap();
        assert_eq!(back, f);
    }

    #[test]
    fn read_missing_returns_empty() {
        let tmp = tempfile::tempdir().unwrap();
        let f = read_remotes(&tmp.path().join("remotes.json")).unwrap();
        assert!(f.remotes.is_empty());
        assert_eq!(f.schema_version, REMOTES_SCHEMA_VERSION);
    }

    #[test]
    fn write_uses_0600() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("remotes.json");
        write_remotes(&path, &RemotesFile::default()).unwrap();
        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o7777;
        assert_eq!(mode, 0o600);
    }

    #[test]
    fn remote_id_is_stable_and_url_dependent() {
        let a = remote_id_for_url("https://host/p?token=AAA");
        let b = remote_id_for_url("https://host/p?token=AAA");
        let c = remote_id_for_url("https://host/p?token=BBB");
        assert_eq!(a, b);
        assert_ne!(a, c);
        assert!(a.starts_with("r-"));
    }
}
```

- [ ] **Step 2: Wire into lib.rs**

Edit `crates/boxpilot-profile/src/lib.rs`:

```rust
pub mod remotes;
pub use remotes::{
    read_remotes, remote_id_for_url, write_remotes, RemoteEntry, RemotesFile,
    REMOTES_SCHEMA_VERSION,
};
```

- [ ] **Step 3: Run the tests**

Run: `cargo test -p boxpilot-profile remotes::tests`
Expected: PASS (4 tests).

- [ ] **Step 4: Commit**

```bash
git add crates/boxpilot-profile/src/remotes.rs crates/boxpilot-profile/src/lib.rs
git commit -m "feat(profile): user-only remotes.json (subscription URL split)"
```

---

## Task 7: `ui-state.json`

**Files:**
- Create: `crates/boxpilot-profile/src/ui_state.rs`
- Modify: `crates/boxpilot-profile/src/lib.rs`

Spec §5.6 lists `ui-state.json` next to `remotes.json`. Its purpose is unspecified beyond "UI state" — plan #4 ships a minimal forward-compatible schema (`schema_version` plus `selected_profile_id`) with `#[serde(default)]` on every other future field so adding more keys later cannot break old installs.

- [ ] **Step 1: Write the failing tests**

Create `crates/boxpilot-profile/src/ui_state.rs`:

```rust
use serde::{Deserialize, Serialize};
use std::path::Path;

use crate::store::write_file_0600_atomic;

pub const UI_STATE_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UiState {
    pub schema_version: u32,
    #[serde(default)]
    pub selected_profile_id: Option<String>,
}

impl Default for UiState {
    fn default() -> Self {
        Self { schema_version: UI_STATE_SCHEMA_VERSION, selected_profile_id: None }
    }
}

pub fn read_ui_state(path: &Path) -> std::io::Result<UiState> {
    match std::fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(UiState::default()),
        Err(e) => Err(e),
    }
}

pub fn write_ui_state(path: &Path, state: &UiState) -> std::io::Result<()> {
    let bytes = serde_json::to_vec_pretty(state)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    write_file_0600_atomic(path, &bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn missing_yields_default() {
        let tmp = tempfile::tempdir().unwrap();
        let s = read_ui_state(&tmp.path().join("ui-state.json")).unwrap();
        assert_eq!(s, UiState::default());
    }

    #[test]
    fn round_trip() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("ui-state.json");
        let mut s = UiState::default();
        s.selected_profile_id = Some("p1".into());
        write_ui_state(&path, &s).unwrap();
        assert_eq!(read_ui_state(&path).unwrap(), s);
    }

    #[test]
    fn unknown_fields_in_input_are_ignored() {
        let json = r#"{"schema_version":1,"selected_profile_id":"x","future_field":42}"#;
        let s: UiState = serde_json::from_str(json).unwrap();
        assert_eq!(s.selected_profile_id.as_deref(), Some("x"));
    }
}
```

- [ ] **Step 2: Wire into lib.rs**

Edit `crates/boxpilot-profile/src/lib.rs`:

```rust
pub mod ui_state;
pub use ui_state::{read_ui_state, write_ui_state, UiState, UI_STATE_SCHEMA_VERSION};
```

- [ ] **Step 3: Run the tests**

Run: `cargo test -p boxpilot-profile ui_state::tests`
Expected: PASS (3 tests).

- [ ] **Step 4: Commit**

```bash
git add crates/boxpilot-profile/src/ui_state.rs crates/boxpilot-profile/src/lib.rs
git commit -m "feat(profile): ui-state.json (forward-compatible)"
```

---

## Task 8: URL redaction for display + manifest

**Files:**
- Create: `crates/boxpilot-profile/src/redact.rs`
- Modify: `crates/boxpilot-profile/src/lib.rs`

§14: "GUI must avoid showing full remote URLs unless the user explicitly reveals them" and "diagnostics export must redact … remote URL query strings". Manifest's `source_url_redacted` (§9.2) consumes the same function. Auth-bearing query keys (case-insensitive): `token`, `key`, `secret`, `password`, `auth`, `t`, `sub`, `subscription`, `apikey`, `api_key`. Userinfo (`https://user:pass@host`) is dropped entirely.

- [ ] **Step 1: Write the failing tests**

Create `crates/boxpilot-profile/src/redact.rs`:

```rust
use url::Url;

const SENSITIVE_KEYS: &[&str] = &[
    "token", "key", "secret", "password", "auth",
    "t", "sub", "subscription", "apikey", "api_key",
];

/// Returns a string suitable for display, logs, and the system-side
/// `manifest.json`'s `source_url_redacted` field.
///
/// - Drops `userinfo` (`user:pass@`).
/// - Replaces sensitive query values with `***`.
/// - Returns the original string unchanged on parse failure (with a
///   `tracing::warn` so we know we couldn't parse it). Better to display
///   a possibly-tokenful URL than to silently drop the whole field — but
///   we should never store an un-redacted URL in a system-side manifest,
///   so the bundle composer (Task 15) treats parse failure as a fatal
///   error rather than calling this function blindly.
pub fn redact_url_for_display(url: &str) -> String {
    let mut parsed = match Url::parse(url) {
        Ok(u) => u,
        Err(_) => {
            tracing::warn!(target: "redact", "could not parse URL for redaction; returning input");
            return url.to_string();
        }
    };
    let _ = parsed.set_username("");
    let _ = parsed.set_password(None);

    let pairs: Vec<(String, String)> = parsed
        .query_pairs()
        .map(|(k, v)| {
            let key = k.to_string();
            let lower = key.to_ascii_lowercase();
            let redacted = if SENSITIVE_KEYS.iter().any(|s| *s == lower) {
                "***".to_string()
            } else {
                v.to_string()
            };
            (key, redacted)
        })
        .collect();

    if !pairs.is_empty() {
        let mut q = parsed.query_pairs_mut();
        q.clear();
        for (k, v) in &pairs {
            q.append_pair(k, v);
        }
    }

    parsed.to_string()
}

/// Strict variant for system-side manifest writing. Returns `None` on
/// parse failure so callers can refuse to compose a manifest.
pub fn redact_url_strict(url: &str) -> Option<String> {
    Url::parse(url).ok()?;
    Some(redact_url_for_display(url))
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn redacts_token_query_param() {
        let r = redact_url_for_display("https://host/path?token=ABC&keep=1");
        assert!(r.contains("token=***"));
        assert!(r.contains("keep=1"));
    }

    #[test]
    fn drops_userinfo() {
        let r = redact_url_for_display("https://user:pass@host/p");
        assert!(!r.contains("user"));
        assert!(!r.contains("pass"));
        assert!(r.contains("host"));
    }

    #[test]
    fn case_insensitive_key_matching() {
        let r = redact_url_for_display("https://h/p?Token=X&KEY=Y&Subscription=Z");
        assert!(r.contains("Token=***"));
        assert!(r.contains("KEY=***"));
        assert!(r.contains("Subscription=***"));
    }

    #[test]
    fn passes_through_url_with_no_secrets() {
        let r = redact_url_for_display("https://host/p?lang=en");
        assert_eq!(r, "https://host/p?lang=en");
    }

    #[test]
    fn strict_rejects_garbage() {
        assert!(redact_url_strict("not a url").is_none());
        assert!(redact_url_strict("https://h/p?token=x").is_some());
    }
}
```

- [ ] **Step 2: Wire into lib.rs**

```rust
pub mod redact;
pub use redact::{redact_url_for_display, redact_url_strict};
```

- [ ] **Step 3: Run the tests**

Run: `cargo test -p boxpilot-profile redact::tests`
Expected: PASS (5 tests).

- [ ] **Step 4: Commit**

```bash
git add crates/boxpilot-profile/src/redact.rs crates/boxpilot-profile/src/lib.rs
git commit -m "feat(profile): URL redaction (§14 subscription URL split)"
```

---

## Task 9: Profile id generation + local file import

**Files:**
- Create: `crates/boxpilot-profile/src/import.rs`
- Modify: `crates/boxpilot-profile/src/lib.rs`

`new_profile_id(name)` produces `"<slug>-<rfc3339-no-colons>-<8-hex>"` — readable in `ls`, sortable, and unique across this user's machine without pulling in `uuid`. `import_local_file` validates the input is JSON, computes its sha256, writes the directory layout (`source.json` + empty `assets/` + `metadata.json`), and returns the new metadata.

- [ ] **Step 1: Write the failing tests**

Create `crates/boxpilot-profile/src/import.rs`:

```rust
use chrono::Utc;
use sha2::{Digest, Sha256};
use std::path::Path;

use boxpilot_ipc::SourceKind;

use crate::list::ProfileStore;
use crate::meta::{write_metadata, ProfileMetadata, METADATA_SCHEMA_VERSION};
use crate::store::{ensure_dir_0700, write_file_0600_atomic};

#[derive(Debug, thiserror::Error)]
pub enum ImportError {
    #[error("source is not valid JSON: {0}")]
    InvalidJson(serde_json::Error),
    #[error("source file is too large ({size} bytes; limit {limit})")]
    TooLarge { size: u64, limit: u64 },
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

/// Cap an in-memory single-JSON import at the per-file limit so a huge
/// pasted blob can't OOM the GUI; matches §9.2's per-file cap.
pub const SINGLE_JSON_MAX_BYTES: u64 = boxpilot_ipc::BUNDLE_MAX_FILE_BYTES;

pub fn slugify(name: &str) -> String {
    let mut s: String = name
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '-' })
        .collect();
    while s.contains("--") { s = s.replace("--", "-"); }
    let trimmed = s.trim_matches('-');
    if trimmed.is_empty() { "profile".to_string() } else { trimmed.to_string() }
}

/// Stable-but-unique-on-this-machine. `name` only contributes the slug;
/// the timestamp + 8-hex random suffix guarantee no collisions across
/// repeated imports of profiles with the same name.
pub fn new_profile_id(name: &str, now: chrono::DateTime<Utc>) -> String {
    let ts = now.format("%Y%m%dT%H%M%SZ").to_string();
    let nanos = now.timestamp_subsec_nanos();
    let pid = std::process::id();
    let mut h = Sha256::new();
    h.update(ts.as_bytes());
    h.update(nanos.to_le_bytes());
    h.update(pid.to_le_bytes());
    h.update(name.as_bytes());
    let suffix = &hex::encode(h.finalize())[..8];
    format!("{}-{}-{}", slugify(name), ts, suffix)
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(bytes);
    hex::encode(h.finalize())
}

pub fn import_local_file(
    store: &ProfileStore,
    src_path: &Path,
    name: &str,
) -> Result<ProfileMetadata, ImportError> {
    let meta = std::fs::metadata(src_path)?;
    if meta.len() > SINGLE_JSON_MAX_BYTES {
        return Err(ImportError::TooLarge { size: meta.len(), limit: SINGLE_JSON_MAX_BYTES });
    }
    let bytes = std::fs::read(src_path)?;
    serde_json::from_slice::<serde_json::Value>(&bytes).map_err(ImportError::InvalidJson)?;

    let now = Utc::now();
    let id = new_profile_id(name, now);
    let dir = store.paths().profile_dir(&id);
    ensure_dir_0700(&dir)?;
    ensure_dir_0700(&store.paths().profile_assets_dir(&id))?;

    write_file_0600_atomic(&store.paths().profile_source(&id), &bytes)?;

    let now_str = now.to_rfc3339();
    let m = ProfileMetadata {
        schema_version: METADATA_SCHEMA_VERSION,
        id: id.clone(),
        name: name.to_string(),
        source_kind: SourceKind::Local,
        remote_id: None,
        created_at: now_str.clone(),
        updated_at: now_str,
        last_valid_activation_id: None,
        config_sha256: sha256_hex(&bytes),
    };
    write_metadata(&store.paths().profile_metadata(&id), &m)?;
    Ok(m)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::ProfileStorePaths;
    use pretty_assertions::assert_eq;
    use std::os::unix::fs::PermissionsExt;

    fn fixture() -> (tempfile::TempDir, ProfileStore) {
        let tmp = tempfile::tempdir().unwrap();
        let s = ProfileStore::new(ProfileStorePaths::new(tmp.path().to_path_buf()));
        (tmp, s)
    }

    #[test]
    fn slugify_handles_punctuation_and_unicode() {
        assert_eq!(slugify("My Profile!"), "my-profile");
        assert_eq!(slugify("一二三 abc"), "abc");
        assert_eq!(slugify("---"), "profile");
    }

    #[test]
    fn id_is_collision_resistant_for_same_name_different_times() {
        let t1 = chrono::Utc::now();
        let t2 = t1 + chrono::Duration::seconds(1);
        assert_ne!(new_profile_id("same", t1), new_profile_id("same", t2));
    }

    #[test]
    fn import_local_file_writes_layout_and_perms() {
        let (tmp, s) = fixture();
        let src = tmp.path().join("input.json");
        std::fs::write(&src, r#"{"hello":"world"}"#).unwrap();

        let m = import_local_file(&s, &src, "Hello").unwrap();
        assert!(matches!(m.source_kind, SourceKind::Local));
        assert!(m.id.starts_with("hello-"));

        // source.json mode 0600
        let src_mode = std::fs::metadata(s.paths().profile_source(&m.id)).unwrap()
            .permissions().mode() & 0o7777;
        assert_eq!(src_mode, 0o600);

        // assets/ mode 0700
        let assets_mode = std::fs::metadata(s.paths().profile_assets_dir(&m.id)).unwrap()
            .permissions().mode() & 0o7777;
        assert_eq!(assets_mode, 0o700);

        // metadata.json mode 0600
        let mm = std::fs::metadata(s.paths().profile_metadata(&m.id)).unwrap()
            .permissions().mode() & 0o7777;
        assert_eq!(mm, 0o600);
    }

    #[test]
    fn import_local_file_rejects_invalid_json() {
        let (tmp, s) = fixture();
        let src = tmp.path().join("bad.json");
        std::fs::write(&src, b"{not json").unwrap();
        assert!(matches!(import_local_file(&s, &src, "n"), Err(ImportError::InvalidJson(_))));
    }
}
```

- [ ] **Step 2: Wire into lib.rs**

```rust
pub mod import;
pub use import::{
    import_local_file, new_profile_id, sha256_hex, slugify, ImportError, SINGLE_JSON_MAX_BYTES,
};
```

- [ ] **Step 3: Run the tests**

Run: `cargo test -p boxpilot-profile import::tests`
Expected: PASS (4 tests).

- [ ] **Step 4: Commit**

```bash
git add crates/boxpilot-profile/src/import.rs crates/boxpilot-profile/src/lib.rs
git commit -m "feat(profile): id generation + import_local_file"
```

---

## Task 10: Local directory import (config + assets)

**Files:**
- Modify: `crates/boxpilot-profile/src/import.rs`

§3.2: "Import local profile directory containing `config.json` and assets". Walk the source directory once, refuse symlinks / non-regular files (defense-in-depth — daemon-side bundle unpack already refuses them per §9.2, but failing fast here gives a better UX), enforce per-file and total-size limits, and stream each asset directly into `~/.local/share/boxpilot/profiles/<id>/assets/` with `0600`. The config can be either `config.json` or `source.json` (some real-world bundles ship as the latter); `source.json` wins when both exist.

- [ ] **Step 1: Write the failing tests**

Append to `crates/boxpilot-profile/src/import.rs` (above the `#[cfg(test)] mod tests` block, alongside `import_local_file`):

```rust
use std::collections::VecDeque;

#[derive(Debug, thiserror::Error)]
pub enum DirImportError {
    #[error("directory has no config.json or source.json")]
    MissingConfig,
    #[error("source is not valid JSON: {0}")]
    InvalidJson(serde_json::Error),
    #[error("symlink not allowed at {0}")]
    SymlinkRejected(std::path::PathBuf),
    #[error("non-regular file rejected at {0}")]
    NotRegular(std::path::PathBuf),
    #[error("file {path} too large ({size} bytes; per-file limit {limit})")]
    FileTooLarge { path: std::path::PathBuf, size: u64, limit: u64 },
    #[error("bundle exceeds total size {total} > {limit}")]
    TotalTooLarge { total: u64, limit: u64 },
    #[error("bundle exceeds file count {count} > {limit}")]
    TooManyFiles { count: u32, limit: u32 },
    #[error("bundle exceeds nesting depth {depth} > {limit}")]
    TooDeep { depth: u32, limit: u32 },
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

pub fn import_local_dir(
    store: &ProfileStore,
    src_dir: &std::path::Path,
    name: &str,
) -> Result<ProfileMetadata, DirImportError> {
    use boxpilot_ipc::{
        BUNDLE_MAX_FILE_BYTES, BUNDLE_MAX_FILE_COUNT, BUNDLE_MAX_NESTING_DEPTH,
        BUNDLE_MAX_TOTAL_BYTES,
    };

    // Pick the source config file. Prefer source.json (sing-box-native),
    // fall back to config.json.
    let src_meta = std::fs::symlink_metadata(src_dir)?;
    if src_meta.file_type().is_symlink() {
        return Err(DirImportError::SymlinkRejected(src_dir.to_path_buf()));
    }
    let mut config_path = src_dir.join("source.json");
    if !config_path.exists() {
        config_path = src_dir.join("config.json");
    }
    if !config_path.exists() {
        return Err(DirImportError::MissingConfig);
    }
    let config_bytes = std::fs::read(&config_path)?;
    serde_json::from_slice::<serde_json::Value>(&config_bytes)
        .map_err(DirImportError::InvalidJson)?;

    // Walk to enumerate assets (every regular file in src_dir except the chosen config).
    struct WalkEntry { rel: std::path::PathBuf, abs: std::path::PathBuf, size: u64 }
    let mut entries: Vec<WalkEntry> = Vec::new();
    let mut total_bytes: u64 = config_bytes.len() as u64;
    let mut file_count: u32 = 1;
    let mut max_depth: u32 = 0;
    let mut queue: VecDeque<(std::path::PathBuf, u32)> = VecDeque::new();
    queue.push_back((src_dir.to_path_buf(), 0));

    while let Some((dir, depth)) = queue.pop_front() {
        max_depth = max_depth.max(depth);
        if depth > BUNDLE_MAX_NESTING_DEPTH {
            return Err(DirImportError::TooDeep { depth, limit: BUNDLE_MAX_NESTING_DEPTH });
        }
        for entry in std::fs::read_dir(&dir)? {
            let entry = entry?;
            let abs = entry.path();
            let ft = std::fs::symlink_metadata(&abs)?.file_type();
            if ft.is_symlink() {
                return Err(DirImportError::SymlinkRejected(abs));
            }
            if ft.is_dir() {
                queue.push_back((abs, depth + 1));
                continue;
            }
            if !ft.is_file() {
                return Err(DirImportError::NotRegular(abs));
            }
            // Skip the chosen config file (we already loaded it).
            if abs == config_path { continue; }
            // Skip a stray sibling of the chosen config to avoid double-importing.
            if abs == src_dir.join("source.json") || abs == src_dir.join("config.json") {
                continue;
            }
            let size = entry.metadata()?.len();
            if size > BUNDLE_MAX_FILE_BYTES {
                return Err(DirImportError::FileTooLarge {
                    path: abs, size, limit: BUNDLE_MAX_FILE_BYTES,
                });
            }
            total_bytes = total_bytes.saturating_add(size);
            file_count = file_count.saturating_add(1);
            if total_bytes > BUNDLE_MAX_TOTAL_BYTES {
                return Err(DirImportError::TotalTooLarge {
                    total: total_bytes, limit: BUNDLE_MAX_TOTAL_BYTES,
                });
            }
            if file_count > BUNDLE_MAX_FILE_COUNT {
                return Err(DirImportError::TooManyFiles {
                    count: file_count, limit: BUNDLE_MAX_FILE_COUNT,
                });
            }
            let rel = abs.strip_prefix(src_dir).unwrap().to_path_buf();
            entries.push(WalkEntry { rel, abs, size });
        }
    }

    // Compose the new profile dir.
    let now = chrono::Utc::now();
    let id = new_profile_id(name, now);
    let dir = store.paths().profile_dir(&id);
    ensure_dir_0700(&dir)?;
    let assets_root = store.paths().profile_assets_dir(&id);
    ensure_dir_0700(&assets_root)?;

    write_file_0600_atomic(&store.paths().profile_source(&id), &config_bytes)?;

    for e in &entries {
        let dst = assets_root.join(&e.rel);
        if let Some(p) = dst.parent() { ensure_dir_0700(p)?; }
        let bytes = std::fs::read(&e.abs)?;
        write_file_0600_atomic(&dst, &bytes)?;
    }

    let now_str = now.to_rfc3339();
    let m = ProfileMetadata {
        schema_version: METADATA_SCHEMA_VERSION,
        id: id.clone(),
        name: name.to_string(),
        source_kind: SourceKind::LocalDir,
        remote_id: None,
        created_at: now_str.clone(),
        updated_at: now_str,
        last_valid_activation_id: None,
        config_sha256: sha256_hex(&config_bytes),
    };
    write_metadata(&store.paths().profile_metadata(&id), &m)?;
    Ok(m)
}
```

Append the test block to the existing `tests` module:

```rust
    #[test]
    fn import_local_dir_walks_and_copies() {
        let (tmp, s) = fixture();
        let src = tmp.path().join("bundle");
        std::fs::create_dir_all(src.join("rules")).unwrap();
        std::fs::write(src.join("config.json"), r#"{"v":1}"#).unwrap();
        std::fs::write(src.join("geosite.db"), b"GEO").unwrap();
        std::fs::write(src.join("rules/r1.srs"), b"SRS").unwrap();

        let m = import_local_dir(&s, &src, "B").unwrap();
        assert!(matches!(m.source_kind, SourceKind::LocalDir));
        let assets = s.paths().profile_assets_dir(&m.id);
        assert_eq!(std::fs::read(assets.join("geosite.db")).unwrap(), b"GEO");
        assert_eq!(std::fs::read(assets.join("rules/r1.srs")).unwrap(), b"SRS");
    }

    #[test]
    fn import_local_dir_rejects_symlink_inside() {
        let (tmp, s) = fixture();
        let src = tmp.path().join("bundle");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(src.join("config.json"), r#"{}"#).unwrap();
        std::os::unix::fs::symlink("/etc/passwd", src.join("evil")).unwrap();
        assert!(matches!(import_local_dir(&s, &src, "B"), Err(DirImportError::SymlinkRejected(_))));
    }

    #[test]
    fn import_local_dir_rejects_missing_config() {
        let (tmp, s) = fixture();
        let src = tmp.path().join("bundle");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(src.join("geosite.db"), b"x").unwrap();
        assert!(matches!(import_local_dir(&s, &src, "B"), Err(DirImportError::MissingConfig)));
    }

    #[test]
    fn import_local_dir_prefers_source_json_over_config_json() {
        let (tmp, s) = fixture();
        let src = tmp.path().join("bundle");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(src.join("source.json"), r#"{"chosen":true}"#).unwrap();
        std::fs::write(src.join("config.json"), r#"{"chosen":false}"#).unwrap();
        let m = import_local_dir(&s, &src, "B").unwrap();
        let saved = std::fs::read(s.paths().profile_source(&m.id)).unwrap();
        assert!(String::from_utf8_lossy(&saved).contains("\"chosen\":true"));
    }
```

- [ ] **Step 2: Update lib.rs re-exports**

```rust
pub use import::{
    import_local_dir, import_local_file, new_profile_id, sha256_hex, slugify,
    DirImportError, ImportError, SINGLE_JSON_MAX_BYTES,
};
```

- [ ] **Step 3: Run the tests**

Run: `cargo test -p boxpilot-profile import::tests`
Expected: PASS (8 tests = 4 from Task 9 + 4 new).

- [ ] **Step 4: Commit**

```bash
git add crates/boxpilot-profile/src/import.rs crates/boxpilot-profile/src/lib.rs
git commit -m "feat(profile): import_local_dir with §9.2 size/count limits"
```

---

## Task 11: Remote URL fetcher trait + cache

**Files:**
- Create: `crates/boxpilot-profile/src/remote.rs`
- Modify: `crates/boxpilot-profile/src/lib.rs`

`RemoteFetcher` is a trait so unit tests don't hit the network — `FixedFetcher` returns canned bytes, `ReqwestFetcher` is the production impl. `import_remote` adds (or refreshes) the entry in `remotes.json` (keyed by `remote_id_for_url`), validates the body is JSON, and writes a profile dir whose `metadata.json` carries the same `remote_id`. The `source_url_redacted` field on the future activation manifest will come from this same URL via `redact_url_strict` — never from `remotes.json`'s full URL.

- [ ] **Step 1: Write the failing tests**

Create `crates/boxpilot-profile/src/remote.rs`:

```rust
use async_trait::async_trait;
use chrono::Utc;
use std::path::Path;

use boxpilot_ipc::SourceKind;

use crate::import::{new_profile_id, sha256_hex, SINGLE_JSON_MAX_BYTES};
use crate::list::ProfileStore;
use crate::meta::{write_metadata, ProfileMetadata, METADATA_SCHEMA_VERSION};
use crate::remotes::{read_remotes, remote_id_for_url, write_remotes, RemoteEntry};
use crate::store::{ensure_dir_0700, write_file_0600_atomic};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FetchedRemote {
    pub bytes: Vec<u8>,
    pub etag: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum FetchError {
    #[error("transport: {0}")]
    Transport(String),
    #[error("body too large: {size} > {limit}")]
    TooLarge { size: u64, limit: u64 },
    #[error("body is not JSON: {0}")]
    InvalidJson(serde_json::Error),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

#[async_trait]
pub trait RemoteFetcher: Send + Sync {
    async fn fetch(&self, url: &str) -> Result<FetchedRemote, FetchError>;
}

pub struct ReqwestFetcher {
    client: reqwest::Client,
}

impl Default for ReqwestFetcher {
    fn default() -> Self {
        Self {
            client: reqwest::Client::builder()
                .user_agent(concat!("boxpilot/", env!("CARGO_PKG_VERSION")))
                .build()
                .expect("reqwest client builder"),
        }
    }
}

#[async_trait]
impl RemoteFetcher for ReqwestFetcher {
    async fn fetch(&self, url: &str) -> Result<FetchedRemote, FetchError> {
        let resp = self.client.get(url).send().await
            .map_err(|e| FetchError::Transport(e.to_string()))?;
        let etag = resp.headers().get(reqwest::header::ETAG)
            .and_then(|v| v.to_str().ok()).map(str::to_string);
        if let Some(len) = resp.content_length() {
            if len > SINGLE_JSON_MAX_BYTES {
                return Err(FetchError::TooLarge { size: len, limit: SINGLE_JSON_MAX_BYTES });
            }
        }
        let bytes = resp.bytes().await
            .map_err(|e| FetchError::Transport(e.to_string()))?;
        if (bytes.len() as u64) > SINGLE_JSON_MAX_BYTES {
            return Err(FetchError::TooLarge {
                size: bytes.len() as u64, limit: SINGLE_JSON_MAX_BYTES,
            });
        }
        Ok(FetchedRemote { bytes: bytes.to_vec(), etag })
    }
}

pub async fn import_remote(
    store: &ProfileStore,
    fetcher: &dyn RemoteFetcher,
    name: &str,
    url: &str,
) -> Result<ProfileMetadata, FetchError> {
    let fetched = fetcher.fetch(url).await?;
    serde_json::from_slice::<serde_json::Value>(&fetched.bytes)
        .map_err(FetchError::InvalidJson)?;

    // Update remotes.json with the full URL (0600).
    let remotes_path = store.paths().remotes_json();
    let mut rfile = read_remotes(&remotes_path).unwrap_or_default();
    let rid = remote_id_for_url(url);
    let now = Utc::now();
    let entry = rfile.remotes.entry(rid.clone()).or_insert(RemoteEntry {
        url: url.to_string(),
        last_fetched_at: None,
        last_etag: None,
    });
    entry.url = url.to_string();
    entry.last_fetched_at = Some(now.to_rfc3339());
    entry.last_etag = fetched.etag.clone();
    ensure_dir_0700(store.paths().root())?;
    write_remotes(&remotes_path, &rfile)?;

    let id = new_profile_id(name, now);
    ensure_dir_0700(&store.paths().profile_dir(&id))?;
    ensure_dir_0700(&store.paths().profile_assets_dir(&id))?;
    write_file_0600_atomic(&store.paths().profile_source(&id), &fetched.bytes)?;

    let now_str = now.to_rfc3339();
    let m = ProfileMetadata {
        schema_version: METADATA_SCHEMA_VERSION,
        id: id.clone(),
        name: name.to_string(),
        source_kind: SourceKind::Remote,
        remote_id: Some(rid),
        created_at: now_str.clone(),
        updated_at: now_str,
        last_valid_activation_id: None,
        config_sha256: sha256_hex(&fetched.bytes),
    };
    write_metadata(&store.paths().profile_metadata(&id), &m)?;
    Ok(m)
}

/// Re-fetch an existing remote profile and overwrite `source.json` in place.
pub async fn refresh_remote(
    store: &ProfileStore,
    fetcher: &dyn RemoteFetcher,
    profile_id: &str,
) -> Result<ProfileMetadata, FetchError> {
    let mut meta = store.get(profile_id)
        .map_err(|e| FetchError::Io(std::io::Error::new(std::io::ErrorKind::NotFound, e.to_string())))?;
    let remote_id = meta.remote_id.clone()
        .ok_or_else(|| FetchError::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidInput, "profile is not a remote profile",
        )))?;

    let remotes_path = store.paths().remotes_json();
    let mut rfile = read_remotes(&remotes_path).unwrap_or_default();
    let url = rfile.remotes.get(&remote_id)
        .ok_or_else(|| FetchError::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound, "remote entry missing from remotes.json",
        )))?
        .url.clone();
    let fetched = fetcher.fetch(&url).await?;
    serde_json::from_slice::<serde_json::Value>(&fetched.bytes)
        .map_err(FetchError::InvalidJson)?;
    let now = Utc::now();
    if let Some(e) = rfile.remotes.get_mut(&remote_id) {
        e.last_fetched_at = Some(now.to_rfc3339());
        e.last_etag = fetched.etag.clone();
    }
    write_remotes(&remotes_path, &rfile)?;

    write_file_0600_atomic(&store.paths().profile_source(profile_id), &fetched.bytes)?;
    meta.updated_at = now.to_rfc3339();
    meta.config_sha256 = sha256_hex(&fetched.bytes);
    write_metadata(&store.paths().profile_metadata(profile_id), &meta)?;
    Ok(meta)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::ProfileStorePaths;
    use pretty_assertions::assert_eq;

    struct FixedFetcher { reply: FetchedRemote }
    #[async_trait]
    impl RemoteFetcher for FixedFetcher {
        async fn fetch(&self, _url: &str) -> Result<FetchedRemote, FetchError> {
            Ok(self.reply.clone())
        }
    }

    fn store_in(tmp: &tempfile::TempDir) -> ProfileStore {
        ProfileStore::new(ProfileStorePaths::new(tmp.path().to_path_buf()))
    }

    #[tokio::test]
    async fn import_remote_writes_metadata_and_remotes_json() {
        let tmp = tempfile::tempdir().unwrap();
        let s = store_in(&tmp);
        let f = FixedFetcher {
            reply: FetchedRemote { bytes: br#"{"v":1}"#.to_vec(), etag: Some("\"abc\"".into()) },
        };
        let m = import_remote(&s, &f, "Sub", "https://h/p?token=AAA").await.unwrap();
        assert!(matches!(m.source_kind, SourceKind::Remote));
        assert!(m.remote_id.is_some());

        let rfile = read_remotes(&s.paths().remotes_json()).unwrap();
        assert_eq!(rfile.remotes.len(), 1);
        let entry = rfile.remotes.values().next().unwrap();
        assert_eq!(entry.url, "https://h/p?token=AAA");
        assert!(entry.last_etag.is_some());
    }

    #[tokio::test]
    async fn import_remote_rejects_non_json() {
        let tmp = tempfile::tempdir().unwrap();
        let s = store_in(&tmp);
        let f = FixedFetcher {
            reply: FetchedRemote { bytes: b"<html>".to_vec(), etag: None },
        };
        let err = import_remote(&s, &f, "Bad", "https://h/p").await.unwrap_err();
        assert!(matches!(err, FetchError::InvalidJson(_)));
    }

    #[tokio::test]
    async fn refresh_remote_updates_in_place() {
        let tmp = tempfile::tempdir().unwrap();
        let s = store_in(&tmp);
        let f1 = FixedFetcher { reply: FetchedRemote { bytes: br#"{"v":1}"#.to_vec(), etag: None } };
        let m = import_remote(&s, &f1, "Sub", "https://h/p").await.unwrap();
        let f2 = FixedFetcher { reply: FetchedRemote { bytes: br#"{"v":2}"#.to_vec(), etag: None } };
        let m2 = refresh_remote(&s, &f2, &m.id).await.unwrap();
        assert_eq!(m2.id, m.id);
        let on_disk = std::fs::read(s.paths().profile_source(&m.id)).unwrap();
        assert!(String::from_utf8_lossy(&on_disk).contains("\"v\":2"));
    }
}
```

The `tokio` test attribute requires `tokio = { workspace = true, features = [..., "rt-multi-thread", "macros"] }` — already satisfied (workspace `tokio` features include `macros` and `rt-multi-thread`). Add `tokio = { workspace = true, features = ["test-util"] }` to `[dev-dependencies]` of the crate **only if a test fails because of a missing feature** — first run without it.

- [ ] **Step 2: Wire into lib.rs**

```rust
pub mod remote;
pub use remote::{
    import_remote, refresh_remote, FetchError, FetchedRemote, RemoteFetcher, ReqwestFetcher,
};
```

- [ ] **Step 3: Run the tests**

Run: `cargo test -p boxpilot-profile remote::tests`
Expected: PASS (3 tests).

- [ ] **Step 4: Commit**

```bash
git add crates/boxpilot-profile/src/remote.rs crates/boxpilot-profile/src/lib.rs
git commit -m "feat(profile): RemoteFetcher trait + import_remote/refresh_remote"
```

---

## Task 12: JSON editor (sparse patch + unknown-field preservation)

**Files:**
- Create: `crates/boxpilot-profile/src/editor.rs`
- Modify: `crates/boxpilot-profile/src/lib.rs`

§9.1: "The editor treats JSON as `serde_json::Value`. Structured editing is implemented as patch operations against the JSON value. Unknown fields must be preserved." Plan #4 ships two operations:

1. `apply_patch(target, patch)` — recursively merges `patch` (a `serde_json::Value`) into `target`. Object keys present in `patch` overwrite the same key in `target`; arrays in `patch` replace arrays in `target` wholesale (sing-box configs use arrays for `inbounds` / `outbounds` / etc., where partial array merging would be unsound). Object keys present only in `target` are left intact — that's where unknown-field preservation comes from.
2. `save_edits(store, id, new_source_bytes)` — validate JSON, atomic-write `source.json`, refresh `metadata.json` (`updated_at`, `config_sha256`).

- [ ] **Step 1: Write the failing tests**

Create `crates/boxpilot-profile/src/editor.rs`:

```rust
use serde_json::Value;
use std::path::Path;

use crate::import::sha256_hex;
use crate::list::{ProfileStore, StoreError};
use crate::meta::{read_metadata, write_metadata};
use crate::store::write_file_0600_atomic;

/// Recursively merge `patch` into `target`. Object keys are merged;
/// non-object values (arrays, strings, numbers, null) replace.
pub fn apply_patch(target: &mut Value, patch: Value) {
    match (target, patch) {
        (Value::Object(t), Value::Object(p)) => {
            for (k, v) in p {
                if v.is_null() {
                    t.remove(&k);
                } else {
                    apply_patch(t.entry(k).or_insert(Value::Null), v);
                }
            }
        }
        (slot, other) => *slot = other,
    }
}

#[derive(Debug, thiserror::Error)]
pub enum EditError {
    #[error("source is not valid JSON: {0}")]
    InvalidJson(serde_json::Error),
    #[error(transparent)]
    Store(#[from] StoreError),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

pub fn save_edits(
    store: &ProfileStore,
    profile_id: &str,
    new_source_bytes: &[u8],
) -> Result<(), EditError> {
    serde_json::from_slice::<Value>(new_source_bytes).map_err(EditError::InvalidJson)?;
    write_file_0600_atomic(&store.paths().profile_source(profile_id), new_source_bytes)?;
    let mut meta = read_metadata(&store.paths().profile_metadata(profile_id))?;
    meta.updated_at = chrono::Utc::now().to_rfc3339();
    meta.config_sha256 = sha256_hex(new_source_bytes);
    write_metadata(&store.paths().profile_metadata(profile_id), &meta)?;
    Ok(())
}

/// Convenience: load source, apply a patch, and save.
pub fn patch_in_place(
    store: &ProfileStore,
    profile_id: &str,
    patch: Value,
) -> Result<(), EditError> {
    let bytes = std::fs::read(store.paths().profile_source(profile_id))?;
    let mut value: Value = serde_json::from_slice(&bytes).map_err(EditError::InvalidJson)?;
    apply_patch(&mut value, patch);
    let new_bytes = serde_json::to_vec_pretty(&value)
        .map_err(EditError::InvalidJson)?;
    save_edits(store, profile_id, &new_bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::import::import_local_file;
    use crate::store::ProfileStorePaths;
    use pretty_assertions::assert_eq;
    use serde_json::json;

    #[test]
    fn apply_patch_preserves_unknown_fields() {
        let mut t = json!({
            "log": {"level": "info", "_unknown_x": 42},
            "inbounds": [{"type": "tun", "_secret": true}],
            "future_top_level": "stays",
        });
        let p = json!({"log": {"level": "debug"}});
        apply_patch(&mut t, p);
        assert_eq!(t["log"]["level"], json!("debug"));
        assert_eq!(t["log"]["_unknown_x"], json!(42));
        assert_eq!(t["future_top_level"], json!("stays"));
        assert_eq!(t["inbounds"][0]["_secret"], json!(true));
    }

    #[test]
    fn apply_patch_array_replaces_wholesale() {
        let mut t = json!({"inbounds": [{"type":"tun"}]});
        apply_patch(&mut t, json!({"inbounds": [{"type":"mixed"}]}));
        assert_eq!(t["inbounds"], json!([{"type":"mixed"}]));
    }

    #[test]
    fn apply_patch_null_removes_key() {
        let mut t = json!({"keep":1, "drop": "x"});
        apply_patch(&mut t, json!({"drop": null}));
        assert_eq!(t, json!({"keep": 1}));
    }

    #[test]
    fn save_edits_updates_metadata_hash_and_timestamp() {
        let tmp = tempfile::tempdir().unwrap();
        let store = ProfileStore::new(ProfileStorePaths::new(tmp.path().to_path_buf()));
        let src = tmp.path().join("in.json");
        std::fs::write(&src, br#"{"v":1}"#).unwrap();
        let m = import_local_file(&store, &src, "P").unwrap();
        let original_hash = m.config_sha256.clone();

        save_edits(&store, &m.id, br#"{"v":2}"#).unwrap();
        let m2 = store.get(&m.id).unwrap();
        assert_ne!(m2.config_sha256, original_hash);
        assert_eq!(m2.config_sha256, sha256_hex(br#"{"v":2}"#));
    }

    #[test]
    fn save_edits_rejects_invalid_json() {
        let tmp = tempfile::tempdir().unwrap();
        let store = ProfileStore::new(ProfileStorePaths::new(tmp.path().to_path_buf()));
        let src = tmp.path().join("in.json");
        std::fs::write(&src, br#"{"v":1}"#).unwrap();
        let m = import_local_file(&store, &src, "P").unwrap();
        let err = save_edits(&store, &m.id, b"{not json").unwrap_err();
        assert!(matches!(err, EditError::InvalidJson(_)));
    }
}
```

- [ ] **Step 2: Wire into lib.rs**

```rust
pub mod editor;
pub use editor::{apply_patch, patch_in_place, save_edits, EditError};
```

- [ ] **Step 3: Run the tests**

Run: `cargo test -p boxpilot-profile editor::tests`
Expected: PASS (5 tests).

- [ ] **Step 4: Commit**

```bash
git add crates/boxpilot-profile/src/editor.rs crates/boxpilot-profile/src/lib.rs
git commit -m "feat(profile): JSON patch editor + save_edits"
```

---

## Task 13: Asset reference enumeration + absolute-path detection

**Files:**
- Create: `crates/boxpilot-profile/src/asset_check.rs`
- Modify: `crates/boxpilot-profile/src/lib.rs`

§9.2: "the user-side backend must … verify that every asset referenced by `config.json` … is present in the bundle's `assets/` directory before submission". §9.3: "If config analysis detects absolute paths … BoxPilot marks them as external dependency risk … **Default behavior at activation time: refuse**." Both reduce to a single AST-style walk over `serde_json::Value` looking for fields whose name is in a known set of asset-path-bearing keys (sing-box uses `path` for `rule_set` / `route_directly`, plus `format` neighbors). Plan #4 covers the high-confidence keys: `path`, `geosite_path`, `geoip_path`. Unknown future asset keys land in plan #8's diagnostics.

- [ ] **Step 1: Write the failing tests**

Create `crates/boxpilot-profile/src/asset_check.rs`:

```rust
use serde_json::Value;
use std::collections::BTreeSet;

const ASSET_PATH_KEYS: &[&str] = &["path", "geosite_path", "geoip_path"];

/// Recursively walk `config` and return the list of relative asset
/// paths it references. Absolute paths are *not* returned here —
/// callers should run [`detect_absolute_paths`] separately so the two
/// concerns can be reported with distinct error codes.
pub fn extract_asset_refs(config: &Value) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    walk_refs(config, &mut out);
    out
}

fn walk_refs(v: &Value, out: &mut BTreeSet<String>) {
    match v {
        Value::Object(map) => {
            for (k, child) in map {
                if ASSET_PATH_KEYS.iter().any(|target| *target == k.as_str()) {
                    if let Value::String(s) = child {
                        if !s.is_empty() && !is_absolute_or_url(s) {
                            out.insert(s.clone());
                        }
                    }
                } else {
                    walk_refs(child, out);
                }
            }
        }
        Value::Array(arr) => {
            for child in arr { walk_refs(child, out); }
        }
        _ => {}
    }
}

fn is_absolute_or_url(s: &str) -> bool {
    s.starts_with('/') || s.contains("://")
}

/// Returns the list of absolute filesystem paths the config references
/// (anything starting with `/`). URL-like values (`http://`, `https://`)
/// are NOT considered "absolute paths" for this check — they are remote
/// fetch targets handled by sing-box itself.
pub fn detect_absolute_paths(config: &Value) -> Vec<String> {
    let mut out = Vec::new();
    walk_abs(config, &mut out);
    out
}

fn walk_abs(v: &Value, out: &mut Vec<String>) {
    match v {
        Value::Object(map) => {
            for (k, child) in map {
                if ASSET_PATH_KEYS.iter().any(|t| *t == k.as_str()) {
                    if let Value::String(s) = child {
                        if s.starts_with('/') {
                            out.push(s.clone());
                        }
                    }
                } else {
                    walk_abs(child, out);
                }
            }
        }
        Value::Array(arr) => for c in arr { walk_abs(c, out); }
        _ => {}
    }
}

#[derive(Debug, thiserror::Error)]
pub enum AssetCheckError {
    #[error("config references {missing} asset(s) not present in assets/: {paths}", paths = .paths.join(", "))]
    MissingFromBundle { missing: usize, paths: Vec<String> },
    #[error("config references absolute path(s) refused per §9.3: {0}", .0.join(", "))]
    AbsolutePathRefused(Vec<String>),
}

/// Walks `assets_dir` to collect every regular file's relative path,
/// then verifies every reference returned by `extract_asset_refs` is
/// present.
pub fn verify_asset_refs(
    config: &Value,
    assets_dir: &std::path::Path,
) -> Result<(), AssetCheckError> {
    let abs = detect_absolute_paths(config);
    if !abs.is_empty() {
        return Err(AssetCheckError::AbsolutePathRefused(abs));
    }
    let needed = extract_asset_refs(config);
    let present = walk_present_assets(assets_dir);
    let missing: Vec<String> = needed.difference(&present).cloned().collect();
    if !missing.is_empty() {
        return Err(AssetCheckError::MissingFromBundle {
            missing: missing.len(), paths: missing,
        });
    }
    Ok(())
}

fn walk_present_assets(root: &std::path::Path) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let read = match std::fs::read_dir(&dir) { Ok(r) => r, Err(_) => continue };
        for entry in read.flatten() {
            let p = entry.path();
            if let Ok(ft) = std::fs::symlink_metadata(&p).map(|m| m.file_type()) {
                if ft.is_dir() { stack.push(p); continue; }
                if ft.is_file() {
                    if let Ok(rel) = p.strip_prefix(root) {
                        if let Some(s) = rel.to_str() {
                            out.insert(s.replace('\\', "/"));
                        }
                    }
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use serde_json::json;

    #[test]
    fn extract_finds_known_keys_in_nested_arrays() {
        let v = json!({
            "route": {
                "rule_set": [
                    {"tag": "geosite", "type": "local", "format": "binary", "path": "geosite.db"},
                    {"tag": "rules",   "type": "local", "format": "source", "path": "rules/r1.json"},
                ]
            },
            "outbounds": [{"path": "ignored-because-array-walk-still-finds-it"}],
        });
        let refs = extract_asset_refs(&v);
        assert!(refs.contains("geosite.db"));
        assert!(refs.contains("rules/r1.json"));
        assert!(refs.contains("ignored-because-array-walk-still-finds-it"));
    }

    #[test]
    fn extract_skips_absolute_and_urls() {
        let v = json!({"x": {"path": "/etc/passwd"}, "y": {"path": "https://h/x"}});
        assert!(extract_asset_refs(&v).is_empty());
    }

    #[test]
    fn detect_absolute_paths_returns_offenders() {
        let v = json!({
            "x": {"path": "/etc/passwd"},
            "y": {"path": "ok.db"},
            "z": {"path": "/home/user/secret"},
        });
        let abs = detect_absolute_paths(&v);
        assert!(abs.contains(&"/etc/passwd".into()));
        assert!(abs.contains(&"/home/user/secret".into()));
        assert!(!abs.iter().any(|p| p == "ok.db"));
    }

    #[test]
    fn verify_passes_when_all_refs_present() {
        let v = json!({"route": {"rule_set": [{"path": "geosite.db"}]}});
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("geosite.db"), b"x").unwrap();
        verify_asset_refs(&v, tmp.path()).unwrap();
    }

    #[test]
    fn verify_fails_when_ref_missing() {
        let v = json!({"route": {"rule_set": [{"path": "geosite.db"}]}});
        let tmp = tempfile::tempdir().unwrap();
        let err = verify_asset_refs(&v, tmp.path()).unwrap_err();
        assert!(matches!(err, AssetCheckError::MissingFromBundle { .. }));
    }

    #[test]
    fn verify_fails_when_absolute_path() {
        let v = json!({"route": {"rule_set": [{"path": "/etc/passwd"}]}});
        let tmp = tempfile::tempdir().unwrap();
        let err = verify_asset_refs(&v, tmp.path()).unwrap_err();
        assert!(matches!(err, AssetCheckError::AbsolutePathRefused(_)));
    }
}
```

- [ ] **Step 2: Wire into lib.rs**

```rust
pub mod asset_check;
pub use asset_check::{
    detect_absolute_paths, extract_asset_refs, verify_asset_refs, AssetCheckError,
};
```

- [ ] **Step 3: Run the tests**

Run: `cargo test -p boxpilot-profile asset_check::tests`
Expected: PASS (6 tests).

- [ ] **Step 4: Commit**

```bash
git add crates/boxpilot-profile/src/asset_check.rs crates/boxpilot-profile/src/lib.rs
git commit -m "feat(profile): asset-ref verifier + §9.3 absolute path refusal"
```

---

## Task 14: Best-effort `sing-box check`

**Files:**
- Create: `crates/boxpilot-profile/src/check.rs`
- Modify: `crates/boxpilot-profile/src/lib.rs`

§10 step 3: "User-side backend runs selected sing-box check when a usable core is reachable as the current uid; this is best-effort and never authoritative." A 5-second hard timeout caps misbehaving cores — `boxpilotd`'s authoritative check (§10 step 7) runs again post-staging anyway. Plan #4 spawns `<core_path> check -c config.json` from a working directory the caller chose (typically the bundle staging dir from Task 15, but unit tests can pass any dir).

- [ ] **Step 1: Write the failing tests**

Create `crates/boxpilot-profile/src/check.rs`:

```rust
use std::path::Path;
use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckOutput {
    pub success: bool,
    pub stdout: String,
    pub stderr: String,
}

#[derive(Debug, thiserror::Error)]
pub enum CheckError {
    #[error("could not spawn core at {0}: {1}")]
    Spawn(std::path::PathBuf, std::io::Error),
    #[error("check timed out after {0:?}")]
    Timeout(Duration),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

pub const CHECK_TIMEOUT: Duration = Duration::from_secs(5);

/// Runs `<core_path> check -c config.json` from `working_dir`.
///
/// `config.json` (and any referenced relative assets) must exist under
/// `working_dir`. Caller is responsible for permissions: `core_path` is
/// expected to be world-executable (managed cores live under
/// `/var/lib/boxpilot/cores/` with `0755`).
pub fn run_singbox_check(core_path: &Path, working_dir: &Path) -> Result<CheckOutput, CheckError> {
    use std::process::{Command, Stdio};
    let mut child = Command::new(core_path)
        .args(["check", "-c", "config.json"])
        .current_dir(working_dir)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| CheckError::Spawn(core_path.to_path_buf(), e))?;

    let start = std::time::Instant::now();
    loop {
        match child.try_wait()? {
            Some(status) => {
                let mut stdout = String::new();
                let mut stderr = String::new();
                if let Some(mut s) = child.stdout.take() {
                    use std::io::Read;
                    let _ = s.read_to_string(&mut stdout);
                }
                if let Some(mut s) = child.stderr.take() {
                    use std::io::Read;
                    let _ = s.read_to_string(&mut stderr);
                }
                return Ok(CheckOutput { success: status.success(), stdout, stderr });
            }
            None => {
                if start.elapsed() >= CHECK_TIMEOUT {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(CheckError::Timeout(CHECK_TIMEOUT));
                }
                std::thread::sleep(Duration::from_millis(50));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    fn write_executable(path: &Path, body: &str) {
        use std::os::unix::fs::PermissionsExt;
        std::fs::write(path, body).unwrap();
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    #[test]
    fn success_case_returns_success_true() {
        let tmp = tempfile::tempdir().unwrap();
        let fake_core = tmp.path().join("fake-core");
        write_executable(&fake_core, "#!/bin/sh\necho ok\nexit 0\n");
        let out = run_singbox_check(&fake_core, tmp.path()).unwrap();
        assert!(out.success);
        assert!(out.stdout.contains("ok"));
    }

    #[test]
    fn failure_case_returns_success_false_and_stderr() {
        let tmp = tempfile::tempdir().unwrap();
        let fake_core = tmp.path().join("fake-core");
        write_executable(&fake_core, "#!/bin/sh\necho boom 1>&2\nexit 1\n");
        let out = run_singbox_check(&fake_core, tmp.path()).unwrap();
        assert!(!out.success);
        assert!(out.stderr.contains("boom"));
    }

    #[test]
    fn missing_core_returns_spawn_error() {
        let tmp = tempfile::tempdir().unwrap();
        let err = run_singbox_check(&tmp.path().join("nope"), tmp.path()).unwrap_err();
        assert!(matches!(err, CheckError::Spawn(..)));
    }
}
```

- [ ] **Step 2: Wire into lib.rs**

```rust
pub mod check;
pub use check::{run_singbox_check, CheckError, CheckOutput, CHECK_TIMEOUT};
```

- [ ] **Step 3: Run the tests**

Run: `cargo test -p boxpilot-profile check::tests`
Expected: PASS (3 tests).

- [ ] **Step 4: Commit**

```bash
git add crates/boxpilot-profile/src/check.rs crates/boxpilot-profile/src/lib.rs
git commit -m "feat(profile): best-effort sing-box check (5s timeout)"
```

---

## Task 15: Bundle composition (§9.2)

**Files:**
- Create: `crates/boxpilot-profile/src/bundle.rs`
- Modify: `crates/boxpilot-profile/src/lib.rs`

`prepare_bundle()` is the contract handoff to plan #5. It composes a `tempfile::TempDir` containing the literal §9.2 layout (`config.json` + `assets/` + `manifest.json`), enforces every §9.2 limit one more time (defense in depth — daemon-side enforces on unpack, but failing here gives the GUI a structured error with a path), runs `verify_asset_refs` (Task 13), generates an `activation_id` of the spec form `<RFC3339-no-colons>-<6-hex>`, and computes both `profile_sha256` and `config_sha256`. The returned `PreparedBundle` owns the `TempDir` so the Tauri layer keeps it alive until plan #5 transmits the fd; dropping it cleans up.

- [ ] **Step 1: Write the failing tests**

Create `crates/boxpilot-profile/src/bundle.rs`:

```rust
use chrono::Utc;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

use boxpilot_ipc::{
    ActivationManifest, AssetEntry, SourceKind, ACTIVATION_MANIFEST_SCHEMA_VERSION,
    BUNDLE_MAX_FILE_BYTES, BUNDLE_MAX_FILE_COUNT, BUNDLE_MAX_NESTING_DEPTH,
    BUNDLE_MAX_TOTAL_BYTES,
};

use crate::asset_check::{verify_asset_refs, AssetCheckError};
use crate::list::ProfileStore;
use crate::redact::redact_url_strict;
use crate::remotes::read_remotes;
use crate::store::ensure_dir_0700;

#[derive(Debug)]
pub struct PreparedBundle {
    pub staging: tempfile::TempDir,
    pub manifest: ActivationManifest,
}

impl PreparedBundle {
    pub fn config_path(&self) -> PathBuf { self.staging.path().join("config.json") }
    pub fn assets_dir(&self) -> PathBuf { self.staging.path().join("assets") }
    pub fn manifest_path(&self) -> PathBuf { self.staging.path().join("manifest.json") }
}

#[derive(Debug, thiserror::Error)]
pub enum BundleError {
    #[error("profile {0} has no source.json on disk")]
    MissingSource(String),
    #[error("profile source is not valid JSON: {0}")]
    InvalidJson(serde_json::Error),
    #[error(transparent)]
    AssetCheck(#[from] AssetCheckError),
    #[error("file {path} too large ({size} bytes; per-file limit {limit})")]
    FileTooLarge { path: PathBuf, size: u64, limit: u64 },
    #[error("bundle exceeds total size {total} > {limit}")]
    TotalTooLarge { total: u64, limit: u64 },
    #[error("bundle exceeds file count {count} > {limit}")]
    TooManyFiles { count: u32, limit: u32 },
    #[error("bundle exceeds nesting depth {depth} > {limit}")]
    TooDeep { depth: u32, limit: u32 },
    #[error("remote profile {0} has no entry in remotes.json")]
    RemoteMissing(String),
    #[error("remote URL is not parseable; refusing to write a manifest")]
    UnparseableRemoteUrl,
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

/// Compose a §9.2 staging directory ready for plan #5 to transfer.
///
/// `core_path_at_activation` and `core_version_at_activation` are
/// passed in by the caller (plan #4 does not call into `boxpilotd`'s
/// `core.discover`; the GUI fetches that separately and forwards the
/// chosen core to this function).
pub fn prepare_bundle(
    store: &ProfileStore,
    profile_id: &str,
    core_path_at_activation: &str,
    core_version_at_activation: &str,
) -> Result<PreparedBundle, BundleError> {
    let meta = store.get(profile_id)
        .map_err(|_| BundleError::MissingSource(profile_id.to_string()))?;

    let source_path = store.paths().profile_source(profile_id);
    if !source_path.exists() {
        return Err(BundleError::MissingSource(profile_id.to_string()));
    }
    let config_bytes = std::fs::read(&source_path)?;
    let config_value: serde_json::Value =
        serde_json::from_slice(&config_bytes).map_err(BundleError::InvalidJson)?;

    let staging = tempfile::tempdir()?;
    let staging_path = staging.path().to_path_buf();
    let assets_dst = staging_path.join("assets");
    ensure_dir_0700(&assets_dst)?;

    // Copy assets out of the user's profile dir so verify_asset_refs runs
    // against the same view boxpilotd will see post-staging-rename.
    let assets_src = store.paths().profile_assets_dir(profile_id);
    let mut total: u64 = config_bytes.len() as u64;
    let mut file_count: u32 = 1;
    let mut max_depth: u32 = 0;
    let mut entries: Vec<AssetEntry> = Vec::new();
    if assets_src.exists() {
        copy_assets_into(&assets_src, &assets_dst, 0, &mut total, &mut file_count, &mut max_depth, &mut entries)?;
    }
    if max_depth > BUNDLE_MAX_NESTING_DEPTH {
        return Err(BundleError::TooDeep { depth: max_depth, limit: BUNDLE_MAX_NESTING_DEPTH });
    }

    // Write config.json
    let config_dst = staging_path.join("config.json");
    std::fs::write(&config_dst, &config_bytes)?;

    // §9.2 reference verification (after assets are in place).
    verify_asset_refs(&config_value, &assets_dst)?;

    // Sort manifest assets for determinism.
    entries.sort_by(|a, b| a.path.cmp(&b.path));

    let now = Utc::now();
    let activation_id = format!(
        "{}-{}",
        now.format("%Y-%m-%dT%H-%M-%SZ"),
        &hex::encode({
            let mut h = Sha256::new();
            h.update(now.timestamp_subsec_nanos().to_le_bytes());
            h.update(profile_id.as_bytes());
            h.update(std::process::id().to_le_bytes());
            h.finalize()
        })[..6]
    );

    let source_url_redacted = match meta.source_kind {
        SourceKind::Local | SourceKind::LocalDir => None,
        SourceKind::Remote => {
            let remote_id = meta.remote_id.clone()
                .ok_or_else(|| BundleError::RemoteMissing(profile_id.to_string()))?;
            let rfile = read_remotes(&store.paths().remotes_json()).unwrap_or_default();
            let entry = rfile.remotes.get(&remote_id)
                .ok_or_else(|| BundleError::RemoteMissing(profile_id.to_string()))?;
            Some(redact_url_strict(&entry.url).ok_or(BundleError::UnparseableRemoteUrl)?)
        }
    };

    let mut profile_hasher = Sha256::new();
    profile_hasher.update(&config_bytes);
    for e in &entries {
        profile_hasher.update(e.path.as_bytes());
        profile_hasher.update(e.sha256.as_bytes());
    }
    let profile_sha256 = hex::encode(profile_hasher.finalize());

    let mut config_hasher = Sha256::new();
    config_hasher.update(&config_bytes);
    let config_sha256 = hex::encode(config_hasher.finalize());

    let manifest = ActivationManifest {
        schema_version: ACTIVATION_MANIFEST_SCHEMA_VERSION,
        activation_id,
        profile_id: profile_id.to_string(),
        profile_sha256,
        config_sha256,
        source_kind: meta.source_kind,
        source_url_redacted,
        core_path_at_activation: core_path_at_activation.to_string(),
        core_version_at_activation: core_version_at_activation.to_string(),
        created_at: now.to_rfc3339(),
        assets: entries,
    };

    let manifest_bytes = serde_json::to_vec_pretty(&manifest)
        .map_err(BundleError::InvalidJson)?;
    std::fs::write(staging_path.join("manifest.json"), &manifest_bytes)?;

    let _ = (total, file_count); // already enforced inside copy_assets_into
    Ok(PreparedBundle { staging, manifest })
}

fn copy_assets_into(
    src: &Path,
    dst: &Path,
    depth: u32,
    total: &mut u64,
    file_count: &mut u32,
    max_depth: &mut u32,
    entries: &mut Vec<AssetEntry>,
) -> Result<(), BundleError> {
    *max_depth = (*max_depth).max(depth);
    if depth > BUNDLE_MAX_NESTING_DEPTH {
        return Err(BundleError::TooDeep { depth, limit: BUNDLE_MAX_NESTING_DEPTH });
    }
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let p = entry.path();
        let ft = std::fs::symlink_metadata(&p)?.file_type();
        if ft.is_symlink() {
            // Symlinks are refused by daemon-side; refuse here too for parity.
            return Err(BundleError::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("symlink in profile assets at {}", p.display()),
            )));
        }
        let rel = entry.file_name();
        let dst_child = dst.join(&rel);
        if ft.is_dir() {
            ensure_dir_0700(&dst_child)?;
            copy_assets_into(&p, &dst_child, depth + 1, total, file_count, max_depth, entries)?;
            continue;
        }
        if !ft.is_file() {
            return Err(BundleError::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("non-regular file in profile assets at {}", p.display()),
            )));
        }
        let bytes = std::fs::read(&p)?;
        let size = bytes.len() as u64;
        if size > BUNDLE_MAX_FILE_BYTES {
            return Err(BundleError::FileTooLarge { path: p.clone(), size, limit: BUNDLE_MAX_FILE_BYTES });
        }
        *total = (*total).saturating_add(size);
        if *total > BUNDLE_MAX_TOTAL_BYTES {
            return Err(BundleError::TotalTooLarge { total: *total, limit: BUNDLE_MAX_TOTAL_BYTES });
        }
        *file_count = (*file_count).saturating_add(1);
        if *file_count > BUNDLE_MAX_FILE_COUNT {
            return Err(BundleError::TooManyFiles { count: *file_count, limit: BUNDLE_MAX_FILE_COUNT });
        }
        std::fs::write(&dst_child, &bytes)?;
        let rel_str = dst_child.strip_prefix(dst.ancestors().last().unwrap_or(dst))
            .unwrap_or(&dst_child)
            .to_string_lossy()
            .to_string();
        let mut h = Sha256::new();
        h.update(&bytes);
        let sha = hex::encode(h.finalize());
        // The path inside the manifest must be relative to the bundle's
        // `assets/` directory. Compute that explicitly.
        // (Walk uses dst as the real assets root passed in by caller.)
        // We re-derive rel from `dst` here:
        let rel_from_assets_root = strip_to_assets_root(&dst_child, dst);
        entries.push(AssetEntry {
            path: rel_from_assets_root.unwrap_or(rel_str),
            sha256: sha,
            size,
        });
    }
    Ok(())
}

fn strip_to_assets_root(dst_child: &Path, _initial_dst: &Path) -> Option<String> {
    // Walk parents until we find a component named "assets" and return
    // everything below it, slash-joined.
    let mut found = false;
    let mut parts: Vec<String> = Vec::new();
    for c in dst_child.components() {
        let s = c.as_os_str().to_string_lossy().to_string();
        if found { parts.push(s); continue; }
        if s == "assets" { found = true; }
    }
    if found && !parts.is_empty() { Some(parts.join("/")) } else { None }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::import::{import_local_dir, import_local_file};
    use crate::store::ProfileStorePaths;
    use pretty_assertions::assert_eq;

    fn fixture() -> (tempfile::TempDir, ProfileStore) {
        let tmp = tempfile::tempdir().unwrap();
        let s = ProfileStore::new(ProfileStorePaths::new(tmp.path().to_path_buf()));
        (tmp, s)
    }

    #[test]
    fn prepare_bundle_local_no_assets_writes_layout() {
        let (tmp, s) = fixture();
        let src = tmp.path().join("in.json");
        std::fs::write(&src, br#"{"log":{"level":"info"}}"#).unwrap();
        let m = import_local_file(&s, &src, "P").unwrap();

        let b = prepare_bundle(&s, &m.id, "/var/lib/boxpilot/cores/current/sing-box", "1.10.0").unwrap();
        assert!(b.config_path().exists());
        assert!(b.assets_dir().exists());
        assert!(b.manifest_path().exists());
        assert!(b.manifest.activation_id.contains('Z'));
        assert!(matches!(b.manifest.source_kind, SourceKind::Local));
        assert!(b.manifest.source_url_redacted.is_none());
    }

    #[test]
    fn prepare_bundle_dir_carries_assets_and_manifest_entries() {
        let (tmp, s) = fixture();
        let src = tmp.path().join("bundle");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(src.join("config.json"),
            br#"{"route":{"rule_set":[{"path":"geosite.db"}]}}"#).unwrap();
        std::fs::write(src.join("geosite.db"), b"GEO").unwrap();
        let m = import_local_dir(&s, &src, "P").unwrap();

        let b = prepare_bundle(&s, &m.id, "/path/sing-box", "1.10.0").unwrap();
        assert_eq!(b.manifest.assets.len(), 1);
        assert_eq!(b.manifest.assets[0].path, "geosite.db");
        assert_eq!(b.manifest.assets[0].size, 3);
        assert!(b.assets_dir().join("geosite.db").exists());
    }

    #[test]
    fn prepare_bundle_refuses_when_asset_missing() {
        let (tmp, s) = fixture();
        let src = tmp.path().join("in.json");
        std::fs::write(&src, br#"{"route":{"rule_set":[{"path":"missing.db"}]}}"#).unwrap();
        let m = import_local_file(&s, &src, "P").unwrap();
        let err = prepare_bundle(&s, &m.id, "/p/sb", "1.10.0").unwrap_err();
        assert!(matches!(err, BundleError::AssetCheck(AssetCheckError::MissingFromBundle { .. })));
    }

    #[test]
    fn prepare_bundle_refuses_absolute_path_in_config() {
        let (tmp, s) = fixture();
        let src = tmp.path().join("in.json");
        std::fs::write(&src,
            br#"{"route":{"rule_set":[{"path":"/etc/passwd"}]}}"#).unwrap();
        let m = import_local_file(&s, &src, "P").unwrap();
        let err = prepare_bundle(&s, &m.id, "/p/sb", "1.10.0").unwrap_err();
        assert!(matches!(err, BundleError::AssetCheck(AssetCheckError::AbsolutePathRefused(_))));
    }
}
```

- [ ] **Step 2: Wire into lib.rs**

```rust
pub mod bundle;
pub use bundle::{prepare_bundle, BundleError, PreparedBundle};
```

- [ ] **Step 3: Run the tests**

Run: `cargo test -p boxpilot-profile bundle::tests`
Expected: PASS (4 tests).

- [ ] **Step 4: Commit**

```bash
git add crates/boxpilot-profile/src/bundle.rs crates/boxpilot-profile/src/lib.rs
git commit -m "feat(profile): activation bundle composer (§9.2)"
```

---

## Task 16: `last-valid/` snapshot + revert

**Files:**
- Create: `crates/boxpilot-profile/src/snapshot.rs`
- Modify: `crates/boxpilot-profile/src/lib.rs`

§5.6: "`last-valid/` is updated only after a successful activation completes step 12 of §10". Plan #5 owns the *update* (it's part of the post-verify commit step). Plan #4 owns the *read* (`revert_to_last_valid` for the editor's revert button) and exposes the *update* function (`record_last_valid`) that plan #5 will call. Both run from this user-side crate so plan #5 doesn't need to touch `~/.local/share`.

- [ ] **Step 1: Write the failing tests**

Create `crates/boxpilot-profile/src/snapshot.rs`:

```rust
use std::path::Path;

use crate::list::{ProfileStore, StoreError};
use crate::meta::{read_metadata, write_metadata};
use crate::store::{ensure_dir_0700, write_file_0600_atomic};

#[derive(Debug, thiserror::Error)]
pub enum SnapshotError {
    #[error("profile has no last-valid snapshot")]
    NoSnapshot,
    #[error(transparent)]
    Store(#[from] StoreError),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

/// Mirror the staged config + assets into `last-valid/`. Replaces any
/// existing snapshot. Idempotent — safe to call from plan #5 after each
/// successful activation. `staged_config` and `staged_assets_dir` are the
/// post-rename release contents (plan #5 reads them back from
/// `/etc/boxpilot/active/...` via `boxpilotd` and forwards the bytes).
///
/// In plan #4 unit tests we copy directly from the staging tempdir.
pub fn record_last_valid(
    store: &ProfileStore,
    profile_id: &str,
    activation_id: &str,
    staged_config: &[u8],
    staged_assets_dir: &Path,
) -> Result<(), SnapshotError> {
    let dst_root = store.paths().profile_last_valid_dir(profile_id);
    if dst_root.exists() {
        std::fs::remove_dir_all(&dst_root)?;
    }
    ensure_dir_0700(&dst_root)?;
    let dst_assets = store.paths().profile_last_valid_assets_dir(profile_id);
    ensure_dir_0700(&dst_assets)?;
    write_file_0600_atomic(&store.paths().profile_last_valid_config(profile_id), staged_config)?;
    if staged_assets_dir.exists() {
        copy_tree(staged_assets_dir, &dst_assets)?;
    }
    let mut meta = read_metadata(&store.paths().profile_metadata(profile_id))
        .map_err(|e| SnapshotError::Io(e))?;
    meta.last_valid_activation_id = Some(activation_id.to_string());
    write_metadata(&store.paths().profile_metadata(profile_id), &meta)?;
    Ok(())
}

fn copy_tree(src: &Path, dst: &Path) -> std::io::Result<()> {
    let mut stack = vec![(src.to_path_buf(), dst.to_path_buf())];
    while let Some((s, d)) = stack.pop() {
        ensure_dir_0700(&d)?;
        for entry in std::fs::read_dir(&s)? {
            let entry = entry?;
            let p = entry.path();
            let ft = std::fs::symlink_metadata(&p)?.file_type();
            let dst_child = d.join(entry.file_name());
            if ft.is_dir() {
                stack.push((p, dst_child));
            } else if ft.is_file() {
                let bytes = std::fs::read(&p)?;
                write_file_0600_atomic(&dst_child, &bytes)?;
            }
            // ignore other file types; daemon-side rejected them on import
        }
    }
    Ok(())
}

/// Restore the editor's `source.json` (and asset tree) from `last-valid/`.
pub fn revert_to_last_valid(
    store: &ProfileStore,
    profile_id: &str,
) -> Result<(), SnapshotError> {
    let lv_config = store.paths().profile_last_valid_config(profile_id);
    if !lv_config.exists() { return Err(SnapshotError::NoSnapshot); }
    let bytes = std::fs::read(&lv_config)?;
    write_file_0600_atomic(&store.paths().profile_source(profile_id), &bytes)?;

    let lv_assets = store.paths().profile_last_valid_assets_dir(profile_id);
    let dst_assets = store.paths().profile_assets_dir(profile_id);
    if dst_assets.exists() { std::fs::remove_dir_all(&dst_assets)?; }
    ensure_dir_0700(&dst_assets)?;
    if lv_assets.exists() { copy_tree(&lv_assets, &dst_assets)?; }

    let mut meta = read_metadata(&store.paths().profile_metadata(profile_id))?;
    meta.updated_at = chrono::Utc::now().to_rfc3339();
    meta.config_sha256 = crate::import::sha256_hex(&bytes);
    write_metadata(&store.paths().profile_metadata(profile_id), &meta)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::import::import_local_dir;
    use crate::store::ProfileStorePaths;
    use pretty_assertions::assert_eq;

    fn fixture() -> (tempfile::TempDir, ProfileStore) {
        let tmp = tempfile::tempdir().unwrap();
        let s = ProfileStore::new(ProfileStorePaths::new(tmp.path().to_path_buf()));
        (tmp, s)
    }

    #[test]
    fn record_then_revert_round_trips_config_and_assets() {
        let (tmp, s) = fixture();
        let src = tmp.path().join("bundle");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(src.join("config.json"), br#"{"v":1}"#).unwrap();
        std::fs::write(src.join("a.db"), b"A").unwrap();
        let m = import_local_dir(&s, &src, "P").unwrap();

        record_last_valid(&s, &m.id, "act-1", br#"{"v":1}"#, &s.paths().profile_assets_dir(&m.id)).unwrap();
        // mutate the working copy
        crate::editor::save_edits(&s, &m.id, br#"{"v":99}"#).unwrap();
        std::fs::write(s.paths().profile_assets_dir(&m.id).join("a.db"), b"DIRTY").unwrap();

        revert_to_last_valid(&s, &m.id).unwrap();
        assert_eq!(std::fs::read(s.paths().profile_source(&m.id)).unwrap(), br#"{"v":1}"#);
        assert_eq!(std::fs::read(s.paths().profile_assets_dir(&m.id).join("a.db")).unwrap(), b"A");

        let m2 = s.get(&m.id).unwrap();
        assert_eq!(m2.last_valid_activation_id.as_deref(), Some("act-1"));
    }

    #[test]
    fn revert_without_snapshot_errors() {
        let (tmp, s) = fixture();
        let src = tmp.path().join("in.json");
        std::fs::write(&src, br#"{"v":1}"#).unwrap();
        let m = crate::import::import_local_file(&s, &src, "P").unwrap();
        let err = revert_to_last_valid(&s, &m.id).unwrap_err();
        assert!(matches!(err, SnapshotError::NoSnapshot));
    }
}
```

- [ ] **Step 2: Wire into lib.rs**

```rust
pub mod snapshot;
pub use snapshot::{record_last_valid, revert_to_last_valid, SnapshotError};
```

- [ ] **Step 3: Run the tests**

Run: `cargo test -p boxpilot-profile snapshot::tests`
Expected: PASS (2 tests).

- [ ] **Step 4: Commit**

```bash
git add crates/boxpilot-profile/src/snapshot.rs crates/boxpilot-profile/src/lib.rs
git commit -m "feat(profile): last-valid snapshot + revert"
```

---

## Task 17: Tauri commands

**Files:**
- Modify: `crates/boxpilot-tauri/Cargo.toml`
- Create: `crates/boxpilot-tauri/src/profile_cmds.rs`
- Modify: `crates/boxpilot-tauri/src/lib.rs`

11 commands (none touch `boxpilotd` — they all run as the desktop user via the new `boxpilot-profile` crate). Errors map onto the existing `CommandError` shape from `commands.rs`. Long-running ones (`profile_import_remote`, `profile_refresh_remote`, `profile_check`) are `async`; pure-fs ones are sync wrapped in `tauri::async_runtime::spawn_blocking` to avoid blocking the UI thread.

State management: a single `Arc<ProfileStore>` is constructed from `ProfileStorePaths::from_env()` once at app start and stored in Tauri `State`. Tests for these wrappers belong in plan #4's overall integration smoke (Task 22), not here — Tauri's `#[command]` proc-macro doesn't unit-test cleanly.

- [ ] **Step 1: Add the dep**

Edit `crates/boxpilot-tauri/Cargo.toml` — add to `[dependencies]`:

```toml
boxpilot-profile = { path = "../boxpilot-profile" }
```

- [ ] **Step 2: Create the command module**

Create `crates/boxpilot-tauri/src/profile_cmds.rs`:

```rust
//! Tauri commands for the user-side profile store. None of these talk
//! to `boxpilotd`; they run in-process as the desktop user.

use std::sync::Arc;

use boxpilot_ipc::ActivationManifest;
use boxpilot_profile::{
    apply_patch, import_local_dir, import_local_file, prepare_bundle, read_remotes,
    redact_url_for_display, refresh_remote, revert_to_last_valid, run_singbox_check, save_edits,
    BundleError, CheckError, CheckOutput, DirImportError, EditError, FetchError, ImportError,
    PreparedBundle, ProfileMetadata, ProfileStore, ReqwestFetcher, RemoteFetcher, SnapshotError,
    StoreError,
};
use serde::{Deserialize, Serialize};
use tauri::State;

use crate::commands::CommandError;

pub struct ProfileState {
    pub store: Arc<ProfileStore>,
    pub fetcher: Arc<ReqwestFetcher>,
    /// Hold the most recent prepared bundle alive so plan #5 can re-use
    /// it; for plan #4 it is just for the GUI preview round-trip.
    pub last_bundle: tokio::sync::Mutex<Option<PreparedBundle>>,
}

trait ToCommandError { fn to_cmd(self) -> CommandError; }

impl ToCommandError for std::io::Error {
    fn to_cmd(self) -> CommandError { CommandError { code: "io".into(), message: self.to_string() } }
}
impl ToCommandError for ImportError {
    fn to_cmd(self) -> CommandError { CommandError { code: "profile.import".into(), message: self.to_string() } }
}
impl ToCommandError for DirImportError {
    fn to_cmd(self) -> CommandError { CommandError { code: "profile.import_dir".into(), message: self.to_string() } }
}
impl ToCommandError for FetchError {
    fn to_cmd(self) -> CommandError { CommandError { code: "profile.fetch".into(), message: self.to_string() } }
}
impl ToCommandError for EditError {
    fn to_cmd(self) -> CommandError { CommandError { code: "profile.edit".into(), message: self.to_string() } }
}
impl ToCommandError for StoreError {
    fn to_cmd(self) -> CommandError { CommandError { code: "profile.store".into(), message: self.to_string() } }
}
impl ToCommandError for BundleError {
    fn to_cmd(self) -> CommandError { CommandError { code: "profile.bundle".into(), message: self.to_string() } }
}
impl ToCommandError for SnapshotError {
    fn to_cmd(self) -> CommandError { CommandError { code: "profile.snapshot".into(), message: self.to_string() } }
}
impl ToCommandError for CheckError {
    fn to_cmd(self) -> CommandError { CommandError { code: "profile.check".into(), message: self.to_string() } }
}

#[derive(Debug, Serialize)]
pub struct ProfileSummary {
    pub id: String,
    pub name: String,
    pub source_kind: boxpilot_ipc::SourceKind,
    pub remote_id: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub last_valid_activation_id: Option<String>,
    pub config_sha256: String,
    pub remote_url_redacted: Option<String>,
}

fn summarize(store: &ProfileStore, m: ProfileMetadata) -> ProfileSummary {
    let remote_url_redacted = m.remote_id.as_ref().and_then(|rid| {
        let rfile = read_remotes(&store.paths().remotes_json()).ok()?;
        rfile.remotes.get(rid).map(|e| redact_url_for_display(&e.url))
    });
    ProfileSummary {
        id: m.id, name: m.name,
        source_kind: m.source_kind, remote_id: m.remote_id,
        created_at: m.created_at, updated_at: m.updated_at,
        last_valid_activation_id: m.last_valid_activation_id,
        config_sha256: m.config_sha256, remote_url_redacted,
    }
}

#[tauri::command]
pub async fn profile_list(state: State<'_, ProfileState>) -> Result<Vec<ProfileSummary>, CommandError> {
    let store = state.store.clone();
    let res = tauri::async_runtime::spawn_blocking(move || store.list())
        .await.map_err(|e| CommandError { code: "join".into(), message: e.to_string() })?
        .map_err(|e| e.to_cmd())?;
    Ok(res.into_iter().map(|m| summarize(&state.store, m)).collect())
}

#[tauri::command]
pub async fn profile_get_source(state: State<'_, ProfileState>, id: String)
    -> Result<String, CommandError>
{
    let store = state.store.clone();
    let bytes = tauri::async_runtime::spawn_blocking(move || store.read_source_bytes(&id))
        .await.map_err(|e| CommandError { code: "join".into(), message: e.to_string() })?
        .map_err(|e| e.to_cmd())?;
    String::from_utf8(bytes).map_err(|e| CommandError { code: "utf8".into(), message: e.to_string() })
}

#[tauri::command]
pub async fn profile_import_file(state: State<'_, ProfileState>, name: String, path: String)
    -> Result<ProfileSummary, CommandError>
{
    let store = state.store.clone();
    let m = tauri::async_runtime::spawn_blocking(move || {
        import_local_file(&store, std::path::Path::new(&path), &name)
    }).await.map_err(|e| CommandError { code: "join".into(), message: e.to_string() })?
        .map_err(|e| e.to_cmd())?;
    Ok(summarize(&state.store, m))
}

#[tauri::command]
pub async fn profile_import_dir(state: State<'_, ProfileState>, name: String, dir: String)
    -> Result<ProfileSummary, CommandError>
{
    let store = state.store.clone();
    let m = tauri::async_runtime::spawn_blocking(move || {
        import_local_dir(&store, std::path::Path::new(&dir), &name)
    }).await.map_err(|e| CommandError { code: "join".into(), message: e.to_string() })?
        .map_err(|e| e.to_cmd())?;
    Ok(summarize(&state.store, m))
}

#[tauri::command]
pub async fn profile_import_remote(state: State<'_, ProfileState>, name: String, url: String)
    -> Result<ProfileSummary, CommandError>
{
    let m = boxpilot_profile::import_remote(&state.store, state.fetcher.as_ref(), &name, &url)
        .await.map_err(|e| e.to_cmd())?;
    Ok(summarize(&state.store, m))
}

#[tauri::command]
pub async fn profile_refresh_remote(state: State<'_, ProfileState>, id: String)
    -> Result<ProfileSummary, CommandError>
{
    let m = refresh_remote(&state.store, state.fetcher.as_ref(), &id)
        .await.map_err(|e| e.to_cmd())?;
    Ok(summarize(&state.store, m))
}

#[tauri::command]
pub async fn profile_save_source(
    state: State<'_, ProfileState>, id: String, source: String,
) -> Result<(), CommandError> {
    let store = state.store.clone();
    let bytes = source.into_bytes();
    tauri::async_runtime::spawn_blocking(move || save_edits(&store, &id, &bytes))
        .await.map_err(|e| CommandError { code: "join".into(), message: e.to_string() })?
        .map_err(|e| e.to_cmd())
}

#[tauri::command]
pub async fn profile_apply_patch_json(
    state: State<'_, ProfileState>, id: String, patch_json: String,
) -> Result<(), CommandError> {
    let patch: serde_json::Value = serde_json::from_str(&patch_json)
        .map_err(|e| CommandError { code: "json".into(), message: e.to_string() })?;
    let store = state.store.clone();
    tauri::async_runtime::spawn_blocking(move || boxpilot_profile::patch_in_place(&store, &id, patch))
        .await.map_err(|e| CommandError { code: "join".into(), message: e.to_string() })?
        .map_err(|e| e.to_cmd())
}

#[tauri::command]
pub async fn profile_revert(state: State<'_, ProfileState>, id: String) -> Result<(), CommandError> {
    let store = state.store.clone();
    tauri::async_runtime::spawn_blocking(move || revert_to_last_valid(&store, &id))
        .await.map_err(|e| CommandError { code: "join".into(), message: e.to_string() })?
        .map_err(|e| e.to_cmd())
}

#[derive(Debug, Deserialize)]
pub struct PrepareBundleRequest {
    pub profile_id: String,
    pub core_path: String,
    pub core_version: String,
}

#[derive(Debug, Serialize)]
pub struct PrepareBundleResponse {
    pub staging_path: String,
    pub manifest: ActivationManifest,
}

#[tauri::command]
pub async fn profile_prepare_bundle(
    state: State<'_, ProfileState>, request: PrepareBundleRequest,
) -> Result<PrepareBundleResponse, CommandError> {
    let store = state.store.clone();
    let prepared = tauri::async_runtime::spawn_blocking(move || {
        prepare_bundle(&store, &request.profile_id, &request.core_path, &request.core_version)
    }).await.map_err(|e| CommandError { code: "join".into(), message: e.to_string() })?
        .map_err(|e| e.to_cmd())?;
    let resp = PrepareBundleResponse {
        staging_path: prepared.staging.path().to_string_lossy().into_owned(),
        manifest: prepared.manifest.clone(),
    };
    *state.last_bundle.lock().await = Some(prepared);
    Ok(resp)
}

#[derive(Debug, Deserialize)]
pub struct CheckRequest { pub profile_id: String, pub core_path: String }

#[derive(Debug, Serialize)]
pub struct CheckResponse { pub success: bool, pub stdout: String, pub stderr: String }

#[tauri::command]
pub async fn profile_check(
    state: State<'_, ProfileState>, request: CheckRequest,
) -> Result<CheckResponse, CommandError> {
    let CheckRequest { profile_id, core_path } = request;
    // `core_path` is moved into the first spawn_blocking closure, so
    // clone it now for the second call.
    let core_path_for_check = core_path.clone();
    let store = state.store.clone();
    let prepared = tauri::async_runtime::spawn_blocking(move || {
        prepare_bundle(&store, &profile_id, &core_path, "best-effort")
    }).await.map_err(|e| CommandError { code: "join".into(), message: e.to_string() })?
        .map_err(|e| e.to_cmd())?;

    let core_path_buf = std::path::PathBuf::from(core_path_for_check);
    let staging = prepared.staging.path().to_path_buf();
    let out: CheckOutput = tauri::async_runtime::spawn_blocking(move || {
        run_singbox_check(&core_path_buf, &staging)
    }).await.map_err(|e| CommandError { code: "join".into(), message: e.to_string() })?
        .map_err(|e| e.to_cmd())?;
    Ok(CheckResponse { success: out.success, stdout: out.stdout, stderr: out.stderr })
}
```

- [ ] **Step 3: Wire into lib.rs**

Replace the entire contents of `crates/boxpilot-tauri/src/lib.rs` with:

```rust
pub mod commands;
pub mod helper_client;
pub mod profile_cmds;

use std::sync::Arc;

pub fn run() {
    init_tracing();

    let store = boxpilot_profile::ProfileStore::new(
        boxpilot_profile::ProfileStorePaths::from_env()
            .expect("could not resolve profile store path"),
    );
    let profile_state = profile_cmds::ProfileState {
        store: Arc::new(store),
        fetcher: Arc::new(boxpilot_profile::ReqwestFetcher::default()),
        last_bundle: tokio::sync::Mutex::new(None),
    };

    tauri::Builder::default()
        .manage(profile_state)
        .invoke_handler(tauri::generate_handler![
            commands::helper_service_status,
            commands::helper_ping,
            commands::helper_core_discover,
            commands::helper_core_install_managed,
            commands::helper_core_upgrade_managed,
            commands::helper_core_rollback_managed,
            commands::helper_core_adopt,
            commands::helper_service_start,
            commands::helper_service_stop,
            commands::helper_service_restart,
            commands::helper_service_enable,
            commands::helper_service_disable,
            commands::helper_service_install_managed,
            commands::helper_service_logs,
            profile_cmds::profile_list,
            profile_cmds::profile_get_source,
            profile_cmds::profile_import_file,
            profile_cmds::profile_import_dir,
            profile_cmds::profile_import_remote,
            profile_cmds::profile_refresh_remote,
            profile_cmds::profile_save_source,
            profile_cmds::profile_apply_patch_json,
            profile_cmds::profile_revert,
            profile_cmds::profile_prepare_bundle,
            profile_cmds::profile_check,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

fn init_tracing() {
    use tracing_subscriber::{fmt, EnvFilter};
    let filter =
        EnvFilter::try_from_env("BOXPILOT_LOG").unwrap_or_else(|_| EnvFilter::new("boxpilot=info"));
    let _ = fmt().with_env_filter(filter).with_target(false).try_init();
}
```

- [ ] **Step 4: Verify the workspace builds**

Run: `cargo build --workspace`
Expected: clean build.

Run: `cargo clippy -p boxpilot-tauri -- -D warnings`
Expected: clean.

- [ ] **Step 5: Commit**

```bash
git add crates/boxpilot-tauri/Cargo.toml crates/boxpilot-tauri/src/profile_cmds.rs crates/boxpilot-tauri/src/lib.rs
git commit -m "feat(tauri): 11 profile_* commands"
```

---

## Task 18: Frontend TS types

**Files:**
- Modify: `frontend/src/api/types.ts`

Mirror the new Rust request/response types so the Vue panel gets typed `invoke` calls.

- [ ] **Step 1: Append to `frontend/src/api/types.ts`**

```typescript
export type SourceKind = "local" | "local-dir" | "remote";

export interface ProfileSummary {
  id: string;
  name: string;
  source_kind: SourceKind;
  remote_id: string | null;
  created_at: string;
  updated_at: string;
  last_valid_activation_id: string | null;
  config_sha256: string;
  remote_url_redacted: string | null;
}

export interface AssetEntry {
  path: string;
  sha256: string;
  size: number;
}

export interface ActivationManifest {
  schema_version: number;
  activation_id: string;
  profile_id: string;
  profile_sha256: string;
  config_sha256: string;
  source_kind: SourceKind;
  source_url_redacted: string | null;
  core_path_at_activation: string;
  core_version_at_activation: string;
  created_at: string;
  assets: AssetEntry[];
}

export interface PrepareBundleRequest {
  profile_id: string;
  core_path: string;
  core_version: string;
}

export interface PrepareBundleResponse {
  staging_path: string;
  manifest: ActivationManifest;
}

export interface CheckRequest { profile_id: string; core_path: string; }
export interface CheckResponse { success: boolean; stdout: string; stderr: string; }
```

- [ ] **Step 2: Verify the frontend type-checks**

Run: `cd frontend && npx vue-tsc -b`
Expected: clean (no errors; new types unused yet, that's fine).

- [ ] **Step 3: Commit**

```bash
git add frontend/src/api/types.ts
git commit -m "feat(frontend): TS types for profile commands"
```

---

## Task 19: Frontend invoke wrappers (`api/profile.ts`)

**Files:**
- Create: `frontend/src/api/profile.ts`

One thin wrapper per command, mirroring the existing `api/helper.ts` style.

- [ ] **Step 1: Create the file**

```typescript
import { invoke } from "@tauri-apps/api/core";
import type {
  CheckRequest, CheckResponse,
  PrepareBundleRequest, PrepareBundleResponse,
  ProfileSummary,
} from "./types";

export async function profileList(): Promise<ProfileSummary[]> {
  return await invoke<ProfileSummary[]>("profile_list");
}
export async function profileGetSource(id: string): Promise<string> {
  return await invoke<string>("profile_get_source", { id });
}
export async function profileImportFile(name: string, path: string): Promise<ProfileSummary> {
  return await invoke<ProfileSummary>("profile_import_file", { name, path });
}
export async function profileImportDir(name: string, dir: string): Promise<ProfileSummary> {
  return await invoke<ProfileSummary>("profile_import_dir", { name, dir });
}
export async function profileImportRemote(name: string, url: string): Promise<ProfileSummary> {
  return await invoke<ProfileSummary>("profile_import_remote", { name, url });
}
export async function profileRefreshRemote(id: string): Promise<ProfileSummary> {
  return await invoke<ProfileSummary>("profile_refresh_remote", { id });
}
export async function profileSaveSource(id: string, source: string): Promise<void> {
  await invoke<void>("profile_save_source", { id, source });
}
export async function profileApplyPatchJson(id: string, patchJson: string): Promise<void> {
  await invoke<void>("profile_apply_patch_json", { id, patchJson });
}
export async function profileRevert(id: string): Promise<void> {
  await invoke<void>("profile_revert", { id });
}
export async function profilePrepareBundle(req: PrepareBundleRequest): Promise<PrepareBundleResponse> {
  return await invoke<PrepareBundleResponse>("profile_prepare_bundle", { request: req });
}
export async function profileCheck(req: CheckRequest): Promise<CheckResponse> {
  return await invoke<CheckResponse>("profile_check", { request: req });
}
```

- [ ] **Step 2: Verify**

Run: `cd frontend && npx vue-tsc -b`
Expected: clean.

- [ ] **Step 3: Commit**

```bash
git add frontend/src/api/profile.ts
git commit -m "feat(frontend): profile invoke wrappers"
```

---

## Task 20: `ProfilesPanel.vue` (minimal)

**Files:**
- Create: `frontend/src/components/ProfilesPanel.vue`

Minimum surface to drive the smoke procedure: list view, three "Add" buttons (paste JSON / import dir / add remote URL), an editor textarea per selected profile, and a "Prepare bundle" button that surfaces the manifest. Structured TUN editing and the §3.2 overview are explicitly plan #7.

- [ ] **Step 1: Create the file**

```vue
<script setup lang="ts">
import { onMounted, ref } from "vue";
import {
  profileApplyPatchJson, profileCheck, profileGetSource, profileImportDir,
  profileImportFile, profileImportRemote, profileList, profilePrepareBundle,
  profileRefreshRemote, profileRevert, profileSaveSource,
} from "../api/profile";
import type { CheckResponse, PrepareBundleResponse, ProfileSummary } from "../api/types";

const profiles = ref<ProfileSummary[]>([]);
const selected = ref<string | null>(null);
const editorText = ref("");
const status = ref<string>("");
const lastBundle = ref<PrepareBundleResponse | null>(null);
const lastCheck = ref<CheckResponse | null>(null);

const newName = ref("");
const newJsonPath = ref("");
const newDirPath = ref("");
const newRemoteUrl = ref("");

const corePath = ref("/var/lib/boxpilot/cores/current/sing-box");
const coreVersion = ref("unknown");

async function refresh() {
  try { profiles.value = await profileList(); }
  catch (e) { status.value = `list failed: ${JSON.stringify(e)}`; }
}

async function selectProfile(id: string) {
  selected.value = id;
  try { editorText.value = await profileGetSource(id); }
  catch (e) { status.value = `read failed: ${JSON.stringify(e)}`; }
}

async function importFile() {
  if (!newName.value || !newJsonPath.value) return;
  try { await profileImportFile(newName.value, newJsonPath.value); newName.value = ""; newJsonPath.value = ""; await refresh(); }
  catch (e) { status.value = `import file: ${JSON.stringify(e)}`; }
}

async function importDir() {
  if (!newName.value || !newDirPath.value) return;
  try { await profileImportDir(newName.value, newDirPath.value); newName.value = ""; newDirPath.value = ""; await refresh(); }
  catch (e) { status.value = `import dir: ${JSON.stringify(e)}`; }
}

async function importRemote() {
  if (!newName.value || !newRemoteUrl.value) return;
  try { await profileImportRemote(newName.value, newRemoteUrl.value); newName.value = ""; newRemoteUrl.value = ""; await refresh(); }
  catch (e) { status.value = `import remote: ${JSON.stringify(e)}`; }
}

async function refreshRemote(id: string) {
  try { await profileRefreshRemote(id); await refresh(); }
  catch (e) { status.value = `refresh: ${JSON.stringify(e)}`; }
}

async function save() {
  if (!selected.value) return;
  try { await profileSaveSource(selected.value, editorText.value); status.value = "saved"; await refresh(); }
  catch (e) { status.value = `save: ${JSON.stringify(e)}`; }
}

async function revert() {
  if (!selected.value) return;
  try { await profileRevert(selected.value); editorText.value = await profileGetSource(selected.value); status.value = "reverted"; }
  catch (e) { status.value = `revert: ${JSON.stringify(e)}`; }
}

async function prepareBundle() {
  if (!selected.value) return;
  try {
    lastBundle.value = await profilePrepareBundle({
      profile_id: selected.value, core_path: corePath.value, core_version: coreVersion.value,
    });
    status.value = `bundle ready @ ${lastBundle.value.staging_path}`;
  } catch (e) { status.value = `bundle: ${JSON.stringify(e)}`; }
}

async function runCheck() {
  if (!selected.value) return;
  try {
    lastCheck.value = await profileCheck({ profile_id: selected.value, core_path: corePath.value });
    status.value = lastCheck.value.success ? "check OK" : "check FAILED";
  } catch (e) { status.value = `check: ${JSON.stringify(e)}`; }
}

onMounted(refresh);
</script>

<template>
  <section class="profiles">
    <h2>Profiles</h2>
    <div v-if="status" class="status">{{ status }}</div>

    <div class="add">
      <h3>Add</h3>
      <label>Name <input v-model="newName" placeholder="My Profile" /></label>
      <div class="row">
        <input v-model="newJsonPath" placeholder="/path/to/file.json" />
        <button :disabled="!newName || !newJsonPath" @click="importFile">Import file</button>
      </div>
      <div class="row">
        <input v-model="newDirPath" placeholder="/path/to/profile-dir" />
        <button :disabled="!newName || !newDirPath" @click="importDir">Import directory</button>
      </div>
      <div class="row">
        <input v-model="newRemoteUrl" placeholder="https://host/path?token=…" />
        <button :disabled="!newName || !newRemoteUrl" @click="importRemote">Add remote</button>
      </div>
    </div>

    <div class="list">
      <h3>Profiles ({{ profiles.length }})</h3>
      <ul>
        <li v-for="p in profiles" :key="p.id" :class="{ active: p.id === selected }">
          <button @click="selectProfile(p.id)">{{ p.name }}</button>
          <span class="meta">{{ p.source_kind }} · {{ p.config_sha256.slice(0, 8) }}</span>
          <span class="url" v-if="p.remote_url_redacted">{{ p.remote_url_redacted }}</span>
          <button v-if="p.source_kind === 'remote'" @click="refreshRemote(p.id)">Refresh</button>
        </li>
      </ul>
    </div>

    <div v-if="selected" class="editor">
      <h3>Editor</h3>
      <textarea v-model="editorText" rows="20" cols="80"></textarea>
      <div class="row">
        <button @click="save">Save</button>
        <button @click="revert">Revert to last-valid</button>
      </div>

      <h3>Activation</h3>
      <label>Core path <input v-model="corePath" /></label>
      <label>Core version <input v-model="coreVersion" /></label>
      <div class="row">
        <button @click="runCheck">Best-effort check</button>
        <button @click="prepareBundle">Prepare bundle (preview)</button>
      </div>
      <pre v-if="lastCheck">{{ lastCheck.success ? 'OK' : 'FAIL' }}
{{ lastCheck.stderr || lastCheck.stdout }}</pre>
      <pre v-if="lastBundle">{{ JSON.stringify(lastBundle.manifest, null, 2) }}</pre>
    </div>
  </section>
</template>

<style scoped>
.profiles { display: flex; flex-direction: column; gap: 1rem; }
.row { display: flex; gap: 0.5rem; align-items: center; }
.list ul { list-style: none; padding: 0; }
.list li { display: flex; gap: 0.5rem; align-items: center; padding: 0.25rem 0; }
.list li.active button:first-child { font-weight: bold; }
.meta { color: #888; font-size: 0.85rem; }
.url { font-family: monospace; font-size: 0.85rem; color: #555; }
textarea { font-family: monospace; }
.status { background: #ffd; padding: 0.5rem; border-radius: 4px; }
</style>
```

- [ ] **Step 2: Verify**

Run: `cd frontend && npx vue-tsc -b`
Expected: clean.

- [ ] **Step 3: Commit**

```bash
git add frontend/src/components/ProfilesPanel.vue
git commit -m "feat(frontend): minimal ProfilesPanel"
```

---

## Task 21: `App.vue` Profiles tab wiring

**Files:**
- Modify: `frontend/src/App.vue`

- [ ] **Step 1: Update App.vue**

Replace the contents of `frontend/src/App.vue`:

```vue
<script setup lang="ts">
import { ref } from "vue";
import CoresPanel from "./components/CoresPanel.vue";
import ProfilesPanel from "./components/ProfilesPanel.vue";
import ServicePanel from "./components/ServicePanel.vue";

type Tab = "home" | "profiles" | "cores";
const tab = ref<Tab>("home");
</script>

<template>
  <main>
    <h1>BoxPilot</h1>
    <nav>
      <button :class="{ active: tab === 'home' }" @click="tab = 'home'">Home</button>
      <button :class="{ active: tab === 'profiles' }" @click="tab = 'profiles'">Profiles</button>
      <button :class="{ active: tab === 'cores' }" @click="tab = 'cores'">Settings → Cores</button>
    </nav>
    <ServicePanel v-if="tab === 'home'" />
    <ProfilesPanel v-else-if="tab === 'profiles'" />
    <CoresPanel v-else-if="tab === 'cores'" />
  </main>
</template>

<style>
body { font-family: system-ui, sans-serif; padding: 2rem; max-width: 60rem; margin: auto; }
nav { display: flex; gap: 0.5rem; margin: 1rem 0; }
nav button { padding: 0.5rem 1rem; }
nav button.active { background: #333; color: #fff; }
</style>
```

- [ ] **Step 2: Build the frontend**

Run: `cd frontend && npm run build`
Expected: clean build.

- [ ] **Step 3: Commit**

```bash
git add frontend/src/App.vue
git commit -m "feat(frontend): mount ProfilesPanel under Profiles tab"
```

---

## Task 22: Workspace test sweep + clippy + frontend build

**Files:** none (verification step).

- [ ] **Step 1: Run the full test suite**

Run: `cargo test --workspace`
Expected: all tests pass. Plan #3 had ~148 tests; this plan adds approximately:
- 3 (Task 2: profile IPC types)
- 4 (Task 3: store paths + perms)
- 2 (Task 4: metadata)
- 4 (Task 5: list)
- 4 (Task 6: remotes)
- 3 (Task 7: ui-state)
- 5 (Task 8: redact)
- 4 (Task 9: import file)
- 4 (Task 10: import dir)
- 3 (Task 11: remote import)
- 5 (Task 12: editor)
- 6 (Task 13: asset_check)
- 3 (Task 14: check)
- 4 (Task 15: bundle)
- 2 (Task 16: snapshot)

…for ~56 new tests, taking the total to ~204. Numbers may shift by ±5 depending on test refactors during implementation.

- [ ] **Step 2: Frontend type-check + build**

Run: `cd frontend && npm run build`
Expected: clean build with no warnings.

- [ ] **Step 3: Run `cargo clippy` workspace-wide**

Run: `cargo clippy --workspace --all-targets -- -D warnings`
Expected: clean.

- [ ] **Step 4: Commit nothing (verification only); proceed to smoke procedure**

---

## Task 23: Manual smoke procedure doc

**Files:**
- Create: `docs/superpowers/plans/2026-04-30-profile-store-smoke-procedure.md`

- [ ] **Step 1: Write the smoke procedure**

Create `docs/superpowers/plans/2026-04-30-profile-store-smoke-procedure.md`:

````markdown
# Plan #4 manual smoke procedure

Run on a Debian/Ubuntu desktop after Task 22 passes. The helper does
**not** need to be reinstalled — plan #4 changes nothing in `boxpilotd`.
A managed core under `/var/lib/boxpilot/cores/current/sing-box` is
required only for step 4 (best-effort check) and step 5 (prepare bundle
preview). If you do not have one, complete plan #2's smoke procedure
through step 2 first.

## 1. Launch GUI on the new branch

```bash
make run-gui
```

Expected: the top nav now has three tabs: **Home**, **Profiles**,
**Settings → Cores**. Click **Profiles** — the panel renders with an
empty list.

## 2. Import a local JSON file

Save a minimal sing-box config to `/tmp/p1.json`:

```json
{"log":{"level":"info"},"inbounds":[],"outbounds":[{"type":"direct","tag":"direct"}]}
```

In the GUI: enter `My Local` as the name, paste `/tmp/p1.json`, click
**Import file**. Expected:
- Profile appears in the list with `local · <8 hex>` metadata.
- `ls -la ~/.local/share/boxpilot/profiles/my-local-*/` shows
  `0700` directory mode and `0600` on `source.json` / `metadata.json`.

## 3. Import a directory profile with one asset

```bash
mkdir -p /tmp/p2/rules
cat > /tmp/p2/config.json <<'EOF'
{"route":{"rule_set":[{"tag":"r","type":"local","format":"binary","path":"geosite.db"}]},
 "outbounds":[{"type":"direct","tag":"d"}]}
EOF
printf 'GEO' > /tmp/p2/geosite.db
```

In the GUI: name `My Dir`, dir `/tmp/p2`, click **Import directory**.
Expected:
- Profile appears with `local-dir` source kind.
- `ls ~/.local/share/boxpilot/profiles/my-dir-*/assets/` lists
  `geosite.db` (`0600`).

## 4. Best-effort check

Click `My Dir`, set **Core path** to your managed core
(`/var/lib/boxpilot/cores/current/sing-box`), click **Best-effort check**.
Expected: `check OK` plus the core's stdout. If you mutate the textarea
to a clearly invalid config (e.g. `{"inbounds": "not an array"}`) and
**Save**, then re-run **Best-effort check**, expect `check FAILED` and
stderr output naming the offending field.

## 5. Prepare bundle (preview)

With `My Dir` still selected, click **Prepare bundle (preview)**.
Expected:
- Status updates to `bundle ready @ /tmp/.tmpXXXXXX`.
- The `manifest.json` JSON renders below, with:
  - `schema_version: 1`
  - `source_kind: "local-dir"`
  - `assets: [{"path": "geosite.db", "size": 3, ...}]`
  - `core_path_at_activation: "/var/lib/boxpilot/cores/current/sing-box"`
- `ls /tmp/.tmpXXXXXX/` (in another terminal, before closing the GUI)
  lists `config.json`, `assets/geosite.db`, `manifest.json`.

## 6. Add a remote profile (URL split test)

Pick any URL serving sing-box JSON. If you do not have a real one, run
a one-shot local server:

```bash
python3 -m http.server 8765 --directory /tmp/p2 &
SERVER=$!
```

In the GUI: name `Sub`, URL `http://localhost:8765/config.json?token=SECRET-TEST`,
click **Add remote**. Expected:
- Profile appears with `remote · <8 hex>`.
- Below the name: `http://localhost:8765/config.json?token=***` (token
  redacted in the panel).
- `cat ~/.local/share/boxpilot/remotes.json | grep token`
  shows the **un-redacted** URL with `SECRET-TEST` (this is correct;
  per §14 the user-side store keeps the full URL with `0600`).
- Click **Prepare bundle (preview)** for `Sub`. The rendered manifest's
  `source_url_redacted` shows `token=***` — confirming the system-side
  manifest never carries the secret.

```bash
kill $SERVER
```

## 7. Editor preserves unknown fields

Click `My Local`, replace the textarea contents with:

```json
{
  "log": {"level": "info", "_unknown_x": 42},
  "inbounds": [],
  "outbounds": [{"type":"direct","tag":"direct","_secret":true}]
}
```

Click **Save**, then click another profile and back. Expected: the
`_unknown_x` and `_secret` fields are still present in the textarea.
`cat ~/.local/share/boxpilot/profiles/my-local-*/source.json` confirms
the bytes on disk match.

## 8. Cleanup

```bash
rm -rf ~/.local/share/boxpilot/profiles ~/.local/share/boxpilot/remotes.json
rm -rf /tmp/p1.json /tmp/p2
```
````

- [ ] **Step 2: Commit**

```bash
git add docs/superpowers/plans/2026-04-30-profile-store-smoke-procedure.md
git commit -m "docs(plan-4): manual smoke procedure"
```

---

## Final checks before opening the PR

- [ ] All 23 tasks above are committed on the `profile-store` branch.
- [ ] `cargo test --workspace` passes locally (~204 tests).
- [ ] `cargo clippy --workspace --all-targets -- -D warnings` passes.
- [ ] `cd frontend && npm run build` passes.
- [ ] Manual smoke procedure (Task 23 doc) ran cleanly on a real desktop.
- [ ] PR title: `feat: user-side profile store + editor (plan #4)`.
- [ ] PR body lists which spec sections changed status (§5.6 directory layout, §9.1 profile bundle model, §9.2 activation manifest + bundle limits, §9.3 absolute path refusal, §14 subscription URL split, §3.2 minimal Profiles tab) and explicitly calls out what plan #5 still owns (the actual `profile.activate_bundle` IPC round-trip + the daemon-side unpack/lock/release-rename/verify pipeline).


