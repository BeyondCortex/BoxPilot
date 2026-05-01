# BoxPilot Plan #8 — Diagnostics Export with Schema-Aware Redaction

**Date:** 2026-04-30
**Spec parent:** `docs/superpowers/specs/2026-04-27-boxpilot-linux-design.md` §5.5 / §6.3 / §14
**Status:** Design (pre-plan)

## 1. Scope

Plan #8 turns the existing `diagnostics.export_redacted` stub into a working
support-bundle generator that follows §14's redaction contract. Plans #1–#7 left
two specific TODOs that this plan retires:

- `iface.rs::diagnostics_export_redacted` — currently routes through `do_stub`
  and returns `NotImplemented`.
- `profile/checker.rs::redact_secrets` — a coarse line-drop heuristic with the
  comment "Plan #8 will replace this with §14 schema-aware redaction".

In scope:

- New module `boxpilot-ipc::redact` exporting
  `redact_singbox_config(&mut serde_json::Value)` — a JSON-path walker that
  zeroes the §14 sensitive fields and applies default-deny at known sensitive
  containers.
- New module tree `crates/boxpilotd/src/diagnostics/{mod,bundle,gc,sysinfo}.rs`
  composing a `boxpilot-diagnostics-<ts>.tar.gz` archive under
  `/var/cache/boxpilot/diagnostics/` and enforcing the §5.5 100 MiB LRU cap.
- IPC types `boxpilot_ipc::diagnostics::DiagnosticsExportResponse` plus a
  `Diagnostics{IoFailed,EncodeFailed}` HelperError variant for the two
  daemon-only failure modes.
- Tauri command `diagnostics_export` and frontend wrapper.
- "Export diagnostics" button on the existing About tab; clicking it calls the
  helper and surfaces the bundle path + size in a toast with an "Open folder"
  affordance.
- Unit tests across the redactor (positive + negative paths), the bundle
  composer, and GC.

Out of scope (deferred to plan #9):

- `.deb` packaging, install-flow polish, GUI auto-launch.
- A "share to web" / upload affordance — v1 stops at "tell the user the path".
- Streaming the bundle back over D-Bus / a memfd. Path-based delivery aligns
  with §5.5's "the user is given a path to share" and avoids re-reading the
  bytes twice for no benefit.
- Diagnostics retention policy beyond LRU. A "delete all diagnostics" button is
  trivial follow-up but not required for v1.
- Bundle format versioning beyond `schema_version: 1` on the manifest. Future
  plans can bump it without breaking GUI parse.

## 2. Dependencies on prior plans

| From plan | Contract this plan relies on |
|-----------|------------------------------|
| #1 | D-Bus iface chokepoint, `dispatch::authorize`, `Paths` helper, polkit policy file format. The `diagnostics.export_redacted` polkit action and `HelperMethod` variant are already declared. |
| #2 | `install-state.json` schema (already non-sensitive) for the bundle. |
| #3 | `service.status` snapshot shape, `target_service` from `BoxpilotConfig`. |
| #5 | `/etc/boxpilot/active` symlink + `manifest.json` shape (already carries `source_url_redacted`). |
| #7 | `AboutTab.vue` exists; the toast composable already shows transient banners. |

The user-side `boxpilot-profile::redact::redact_url_for_display` from plan #4
stays untouched. Plan #8's walker operates on `serde_json::Value` and does not
parse URLs — the manifest's `source_url_redacted` is already redacted upstream.

## 3. Architecture

### 3.1 Module placement

`boxpilotd` does not depend on `boxpilot-profile` (system-side vs. user-side
split is intentional). The schema-aware walker therefore lives in the only
crate both consumers depend on:

```text
boxpilot-ipc/src/
  redact.rs         (new)  — schema-aware sing-box JSON walker
```

`boxpilot-profile::redact.rs` keeps its URL-only redaction; the user side does
not re-export the schema-aware walker because today's profile-store flows
already emit only the fields they need (manifest carries `source_url_redacted`,
the editor preserves unknown fields without copying them off-disk). If a future
plan wants to redact a profile JSON before showing it in the editor, it can
import from `boxpilot_ipc::redact` directly.

### 3.2 Schema-aware redaction walker

```rust
// boxpilot_ipc::redact

/// Walks `value` in-place and replaces sensitive sing-box fields with
/// `"***"`. Operates on a `&mut serde_json::Value` so callers can choose
/// to redact a clone (for diagnostics) without disturbing the original.
///
/// Replaces `outbounds[*]` and `inbounds[*].users[*]` keys listed in §14
/// and applies *default-deny* under those containers: any field whose key
/// is not on the public-allowlist is replaced with `"***"`. The
/// public-allowlist for `outbounds[*]` is the structural sing-box keys
/// that have no secret value (`type`, `tag`, `network`, `transport`,
/// `tls.server_name`, `multiplex.*` modulo `password`, …).
pub fn redact_singbox_config(value: &mut serde_json::Value);
```

Field-level rules:

| JSON path | Action |
|-----------|--------|
| `outbounds[*].password` | replace with `"***"` |
| `outbounds[*].uuid` | replace with `"***"` |
| `outbounds[*].private_key` | replace with `"***"` |
| `outbounds[*].server` | replace with `"***"` |
| `outbounds[*].server_port` | replace with `0` |
| `outbounds[*].password_*` (any key matching `^password`) | replace with `"***"` |
| `outbounds[*].<unknown>` (anything not on the public-allowlist) | replace with `"***"` |
| `inbounds[*].users[*].password` / `.uuid` / `.<unknown>` | replace with `"***"` |
| `inbounds[*].users[*].name` | passthrough (usernames are not §14-sensitive) |
| `dns.servers[*].address` | host portion replaced with `"***"`, scheme/port preserved |
| `experimental.clash_api.secret` | replace with `"***"` |
| `experimental.clash_api.external_controller` | passthrough (already public) |
| `endpoints[*].private_key` (WireGuard) | replace with `"***"` |
| `endpoints[*].peer_public_key` | passthrough |
| Anything else | passthrough |

Public allowlist for `outbounds[*]` (the keys that survive default-deny):

```
type, tag, network, transport, transport.*, multiplex.enabled,
multiplex.protocol, multiplex.max_connections, multiplex.min_streams,
multiplex.max_streams, multiplex.padding, tls.enabled,
tls.server_name, tls.insecure, tls.alpn, tls.utls.enabled,
tls.utls.fingerprint, tls.reality.enabled, tls.reality.public_key
(only when listed by user — see note), tls.reality.short_id,
domain_strategy, detour, fallback, fallback_delay,
udp_over_tcp, udp_over_tcp.enabled, udp_over_tcp.version, packet_encoding,
override_address, override_port (these last two redacted: §14 server-redaction
extends to override targets)
```

Note: `tls.reality.public_key` is in the public allowlist *only* in the sense
that its key name is allowed; the `default-deny` rule does not strip it. If a
user considers a reality public key sensitive in their threat model, that's a
plan #N+ decision — the §14 list does not include it.

The walker uses iterative `Vec`-based traversal to bound stack depth at
`BUNDLE_MAX_NESTING_DEPTH` (32, already defined in `boxpilot-ipc::profile`).
Anything deeper is replaced with `"***"` and a `tracing::warn` is emitted.
This both enforces a hard cap and matches the §9.2 unpacker's invariant.

### 3.3 Bundle composer

Helper-side module `crates/boxpilotd/src/diagnostics/`:

```text
diagnostics/
  mod.rs       — public entry: compose(&Paths, &Systemd, &Journal) -> Result<DiagnosticsExportResponse>
  bundle.rs    — tar.gz writer, file collection, per-entry redaction
  gc.rs        — LRU eviction down to 100 MiB before writing the new bundle
  sysinfo.rs   — kernel uname + /etc/os-release reader
```

`compose()` builds the archive in a `tempfile::NamedTempFile` co-located in
`/var/cache/boxpilot/diagnostics/` (so the final rename stays on the same
filesystem), then `rename(2)`-s it into place. The temp suffix is filtered out
of the GC scan so a partial bundle never appears as "current".

Bundle layout:

```text
boxpilot-diagnostics-<YYYY-MM-DDTHH-MM-SSZ>/
  diagnostics-manifest.json     — schema_version, generated_at, file list
  active-config.json            — redacted via §3.2
  boxpilot.toml                 — verbatim (controller_uid is non-sensitive)
  install-state.json            — verbatim (no secrets per §5.4)
  service-unit.txt              — verbatim systemd unit fragment
  service-status.json           — service.status response captured live
  manifest.json                 — copy of /etc/boxpilot/active/manifest.json
  journal-tail.txt              — 200 most recent journal lines, line-drop redacted
  system-info.json              — kernel, distro, boxpilot version, daemon uptime
```

If a source file is missing or unreadable, the composer writes a placeholder
`<name>.unavailable.txt` containing the failure cause string. This keeps the
bundle internally consistent — the manifest always lists the fixed file set
and the user can see *why* something is missing rather than just an empty
slot.

### 3.4 GC

`gc::evict_to_cap(dir: &Path, cap_bytes: u64)` runs *before* writing a new
bundle:

1. List `*.tar.gz` entries in the directory (ignoring tempfiles whose name
   starts with `.`).
2. Sort by mtime ascending (oldest first).
3. While the total size is over `cap_bytes`, delete the oldest. If a deletion
   fails (e.g. ENOENT race), log and continue with the next.

Cap is `100 * 1024 * 1024` bytes per §5.5. The cap is a workspace constant
exported from `boxpilot_ipc::diagnostics`.

### 3.5 IPC method

```rust
// boxpilot_ipc::diagnostics

pub const DIAGNOSTICS_BUNDLE_CAP_BYTES: u64 = 100 * 1024 * 1024;
pub const DIAGNOSTICS_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiagnosticsExportResponse {
    pub schema_version: u32,        // 1
    pub bundle_path: String,        // absolute path on disk
    pub bundle_size_bytes: u64,
    pub generated_at: String,       // RFC3339 UTC
}
```

D-Bus method: `DiagnosticsExportRedacted()` → `s` (JSON
`DiagnosticsExportResponse`). No request body. Authorization class
**Mutating** (writes to `/var/cache`, takes the global lock to serialize with
activations that might be writing `manifest.json` concurrently).

Polkit action `app.boxpilot.helper.diagnostics.export-redacted` is already
declared with `auth_admin_keep` defaults. The 49-boxpilot.rules file already
promotes the controller to a less-prompting tier. No polkit change needed.

### 3.6 Failure modes

Two new HelperError variants:

```rust
#[error("diagnostics i/o failed at {step}: {cause}")]
DiagnosticsIoFailed { step: String, cause: String },

#[error("diagnostics encode failed: {cause}")]
DiagnosticsEncodeFailed { cause: String },
```

Mapped to D-Bus error names `app.boxpilot.Helper1.DiagnosticsIoFailed` and
`app.boxpilot.Helper1.DiagnosticsEncodeFailed` in `iface.rs::to_zbus_err`.

Per-file collection failures do **not** raise either of these — they fall
through to the `<name>.unavailable.txt` placeholder. The two error variants
are reserved for "could not write the tarball at all" and "could not encode
the manifest".

### 3.7 Replacing the journal-tail stub

`profile/checker.rs::redact_secrets` is renamed/moved to
`crates/boxpilotd/src/diagnostics/bundle.rs::redact_journal_lines` (single
shared call site between the activation flow's stderr-tail and the
diagnostics bundle's journal-tail). The line-drop heuristic itself is
unchanged for v1 — text-stage redaction is fundamentally a "drop suspicious
lines" exercise; schema-aware walking only applies to JSON. The comment in
`checker.rs` is updated to remove the "plan #8 will replace this" claim and
to point to the shared helper.

## 4. Data flow

```text
GUI: AboutTab.vue → "Export diagnostics" button (single-flight)
    ↓
Tauri: diagnostics_export command → helper_client.diagnostics_export_redacted
    ↓
zbus: DiagnosticsExportRedacted()
    ↓
boxpilotd::iface::diagnostics_export_redacted
    ├─ dispatch::authorize (polkit, lock)
    └─ diagnostics::compose(&ctx.paths, &*ctx.systemd, &*ctx.journal)
       ├─ gc::evict_to_cap(&diag_dir, 100 MiB)
       ├─ collect:
       │    active config + redact_singbox_config
       │    boxpilot.toml verbatim
       │    install-state.json verbatim
       │    /etc/systemd/system/<unit>.service verbatim
       │    service.status snapshot
       │    /etc/boxpilot/active/manifest.json
       │    journal tail (200 lines) + redact_journal_lines
       │    system info
       ├─ build tar.gz in NamedTempFile under /var/cache/boxpilot/diagnostics/
       ├─ persist (rename) → boxpilot-diagnostics-<ts>.tar.gz
       └─ return { bundle_path, bundle_size_bytes, generated_at, schema_version: 1 }
    ↓
GUI: toast "Diagnostics exported to <path> (<size>)"
     + "Open folder" button calls Tauri's existing `opener` plugin
```

## 5. Wire format

### 5.1 `boxpilot_ipc::diagnostics`

```rust
pub const DIAGNOSTICS_SCHEMA_VERSION: u32 = 1;
pub const DIAGNOSTICS_BUNDLE_CAP_BYTES: u64 = 100 * 1024 * 1024;
pub const JOURNAL_TAIL_LINES: usize = 200;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiagnosticsExportResponse {
    pub schema_version: u32,
    pub bundle_path: String,
    pub bundle_size_bytes: u64,
    pub generated_at: String,
}
```

### 5.2 Bundle manifest (inside the tarball)

```json
{
  "schema_version": 1,
  "generated_at": "2026-04-30T22:00:00Z",
  "boxpilot_version": "0.1.0",
  "files": [
    {"name": "active-config.json", "redacted": true,  "size": 1234},
    {"name": "boxpilot.toml",      "redacted": false, "size": 567},
    ...
  ],
  "host": {
    "kernel": "Linux 6.6.0",
    "os_id": "ubuntu",
    "os_version_id": "24.04",
    "os_pretty_name": "Ubuntu 24.04 LTS"
  }
}
```

The manifest is the only inside-bundle file the GUI parses (currently it
doesn't, but a future plan may). All other files are user-facing artifacts
the support engineer reads directly.

## 6. Testing

Unit tests:

- `boxpilot_ipc::redact` — round-trips of canned configs covering each §14
  field, plus default-deny under `outbounds[*]` and `inbounds[*].users[*]`.
- `diagnostics::bundle` — fixture filesystem, asserts the tar contains the
  expected files, asserts redaction applied to `active-config.json`,
  asserts placeholder text written when sources missing.
- `diagnostics::gc` — synthetic `*.tar.gz` files with set mtimes, asserts
  oldest-deleted-first until under cap, asserts tempfiles are not touched.
- `iface::diagnostics_export_redacted` — happy path returns response,
  polkit-deny returns NotAuthorized, lock contention returns Busy.

Integration:

- Smoke procedure step that runs the helper, opens the resulting tarball,
  and grep-fails if it finds any of the canary sensitive substrings from
  the test fixture.

## 7. Risks and mitigations

| Risk | Mitigation |
|------|------------|
| User reads bundle and finds a leaked secret we missed | Default-deny under known sensitive containers means new sing-box outbound types do not silently leak. The walker logs each redacted key at `tracing::debug` so test fixtures can assert coverage. |
| Bundle balloons past 100 MiB on a misconfigured journal | Journal tail capped at 200 lines × ~1 KiB each ≈ 200 KiB. Other files combined < 1 MiB typical. The 100 MiB cap is for *retention* of multiple bundles, not a single export. |
| GC race: another helper call writes to the dir mid-eviction | The global lock `/run/boxpilot/lock` is held for the entire `compose()`. Concurrent helper calls block, not race. |
| Path-based delivery leaks bundle to non-controller local users | Bundle is `0640 root:root` under `/var/cache/boxpilot/diagnostics/` (root-owned dir mode `0750`). Non-controller local users cannot read it. |
| User shares the bundle with a third party expecting it scrubbed | The bundle manifest's `files[].redacted` flag is true for `active-config.json` and false for everything else. Documentation in README + the toast string after export both name the redaction scope. |
| The §14 list grows but the walker doesn't | Default-deny under sensitive containers absorbs new fields. Known additions land here as a focused PR. The test suite has a "regression fixture" file the user can extend without touching the walker. |

## 8. Acceptance

Plan #8 ships when:

1. `cargo test -p boxpilot-ipc` covers `redact_singbox_config` for every §14
   row above.
2. `cargo test -p boxpilotd diagnostics::` is green.
3. `cargo clippy --workspace --all-targets -- -D warnings` is green.
4. `cd frontend && npm run build` is green.
5. `xmllint` validates the polkit policy (no change expected; sanity check).
6. The smoke procedure in
   `docs/superpowers/plans/2026-04-30-diagnostics-export-smoke-procedure.md`
   passes on the dev VM: button click → file appears → tarball contains the
   manifest + `active-config.json` with `"***"` substituted into the canary
   secrets fixture.
