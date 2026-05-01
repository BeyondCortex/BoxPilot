# BoxPilot Plan #7 — GUI Shell (Home / Profiles / Settings)

**Date:** 2026-04-30
**Spec parent:** `docs/superpowers/specs/2026-04-27-boxpilot-linux-design.md` §3 / §6.6 / §10
**Status:** Design (pre-plan)

## 1. Scope

Plan #7 turns the prototype 3-tab Vue UI into the v1.0 control panel described
in spec §3. Plans #1–#6 already shipped every privileged operation the user
needs (service control, core install/upgrade/rollback/adopt, profile
import/edit/activate/rollback, legacy observe + migrate). The GUI is the only
remaining surface that is not yet usable end-to-end.

**In scope:**

- A single read-only IPC method, `home.status`, that batches data the Home
  page needs in one D-Bus round-trip (saves ~3 round-trips per render and
  avoids a TOCTOU split between service status and `boxpilot.toml`'s
  `active_*` snapshot).
- Tauri command wrappers + frontend API typings for `home.status`.
- Vue 3 frontend rewrite producing three top-level pages (Home / Profiles /
  Settings) plus the sub-tabs spec §3.3 calls for.
- Profile activate flow surfaced in the GUI: preflight check → prepare bundle
  preview → activate → outcome banner branching on the four §10
  `ActivateOutcome` variants.
- Manual rollback UI driven by the existing `profile.rollback_release`.
- Legacy detection + guided migrate (observe → prepare → import → cutover).
- Drift / repair banners surfacing the spec §6.6 `controller_orphaned` and
  §10 `ActiveCorrupt` terminal states.
- "Enable Clash API" one-click patch on the selected profile (spec §3.3 / §12)
  using the existing `profile_apply_patch_json` command.

**Out of scope (deferred):**

- Schema-aware diagnostics export with redaction (plan #8).
- `.deb` packaging, install flow polish, GUI auto-launch (plan #9).
- D-Bus signal subscriptions for live state push. Plan #7 polls every 5 s.
- Network-shape detection (TUN / proxy / TUN+proxy). Requires a helper read
  of `/etc/boxpilot/active/config.json`; deferred until we add that method.
- Specific-version dropdown listing upstream sing-box releases. The existing
  "latest | exact" text input is reused; only already-installed versions
  appear in the rollback picker because they come from `core.discover`.
- Rich structured editing of inbounds/outbounds/route rule sets. Plan #7
  ships a read-only structured **overview**; full editing stays in the JSON
  textarea per §3.2's "edit profile JSON without losing unknown fields."
  Structured TUN editing is limited to the three fields spec §3.2 names
  (`auto_route`, `auto_redirect`, `strict_route`) plus an enable/disable
  toggle for the TUN inbound itself.
- Drag-and-drop import; only path-textbox import is shipped.
- i18n / dark mode / accessibility audit. Out of scope for v1.0.

## 2. Dependencies on prior plans

| From plan | Contract this plan relies on |
|-----------|------------------------------|
| #1 | `boxpilotd` D-Bus iface chokepoint, polkit policy file format, `Helper::do_*` pattern |
| #2 | `core.discover`, `core.install_managed`, `core.upgrade_managed`, `core.rollback_managed`, `core.adopt`, `BoxpilotConfig::core_path` / `core_state` |
| #3 | `service.status`, control verbs, `service.install_managed`, `service.logs` |
| #4 | All `profile_*` Tauri commands; `profile_apply_patch_json` for the Clash API patch |
| #5 | `profile.activate_bundle`, `profile.rollback_release`, `BoxpilotConfig::active_*`/`previous_*` fields, `ActivateOutcome` |
| #6 | `legacy.observe_service`, `legacy.migrate_service` Prepare + Cutover + bytes payload |

## 3. Architecture

### 3.1 New IPC method: `home.status`

Wire form is a JSON struct that bundles existing data so the Home page renders
in one D-Bus round-trip. Auth class **ReadOnly** — same tier as
`service.status`.

```rust
// boxpilot_ipc::home::HomeStatusResponse
{
  service: ServiceStatusResponse,                  // existing
  active_profile: Option<ActiveProfileSnapshot>,   // null when no activation
  core: CoreSnapshot,                              // path/version/kind from boxpilot.toml
  active_corrupt: bool,                            // /etc/boxpilot/active broken
  schema_version: u32,                             // 1
}
```

`ActiveProfileSnapshot` mirrors the four `active_*` fields in
`BoxpilotConfig`: `profile_id`, `profile_name`, `profile_sha256`,
`release_id`, `activated_at`. `CoreSnapshot` carries `path`, `state`
(`external` / `managed-installed` / `managed-adopted`), and `version`.
The version is sourced by reusing `core::discover`'s already-cached
result and matching `DiscoveredCore.path == cfg.core_path`; if the
match misses we fall back to `"unknown"`. No fresh process spawn per
poll. `active_corrupt` is set when `/etc/boxpilot/active` does not
resolve under `releases/`, mirroring the recovery check `boxpilotd`
already runs at startup (§10 step "crash recovery").

The method does **not** trigger any state mutation and does **not** require
the controller. Non-controller users can poll it.

Polkit action ID: `app.boxpilot.helper.home.status`. Same allow tier as
`service.status` (allow_any/inactive/active = `yes`).

### 3.2 Vue source tree

```
frontend/src/
  api/
    helper.ts          — service / core / legacy / home wrappers
    profile.ts         — profile_* wrappers (existing, expanded)
    types.ts           — wire types (existing, expanded)
  composables/
    useToast.ts        — top-banner error/success queue
    useBusy.ts         — single-flight wrapper for buttons
    useHomeStatus.ts   — 5-second poll of home.status
    useProfiles.ts     — list cache + refresh
    useCores.ts        — discover cache + refresh
  views/
    HomeView.vue
    ProfilesView.vue
    SettingsView.vue
  components/
    AppShell.vue       — header + nav + slot for the active view
    Toast.vue
    home/
      ServiceCard.vue
      ActiveProfileCard.vue
      CoreCard.vue
      DriftBanner.vue
      LogsPanel.vue
      QuickActions.vue
    profiles/
      ProfileList.vue
      ProfileDetail.vue
      ProfileOverview.vue   — read-only structured overview
      ProfileEditor.vue     — textarea + Save/Revert + Clash-API toggle
      ProfileActivate.vue   — check + prepare + activate + outcome
    settings/
      CoresTab.vue
      ServiceTab.vue
      LegacyTab.vue
      AboutTab.vue
  App.vue              — picks the active view from a top-level ref
  main.ts              — bootstrap (existing)
```

No router library: spec §3 calls out three top-level pages with sub-tabs in
Settings only. A `ref<'home'|'profiles'|'settings'>` plus a sub-ref for
Settings is enough, matches the existing style, and avoids adding
`vue-router`.

### 3.3 State management

No external store (Pinia, Vuex). Each composable owns one `ref` + a refresh
function and is consumed via `provide` / `inject` from `App.vue`. Polling
intervals:

- `useHomeStatus` — 5 s while the Home view is mounted, paused otherwise.
- `useProfiles` / `useCores` — refresh on view mount and on user action; no
  background polling.

Single-flight (`useBusy`) prevents the user from double-clicking buttons that
talk to the helper — actions disabled while in-flight.

### 3.4 Error handling

Tauri commands already serialize `CommandError { code, message }`. The
frontend shows the `message` in a transient toast and keeps the `code` in a
console log for diagnostics. Two codes get dedicated UI:

- `controller_orphaned` → DriftBanner asks the user to claim the controller
  (spec §6.6 transfer flow is out of scope; v1 just surfaces the warning).
- `active_corrupt` → DriftBanner pushes the user to the Profiles tab to
  re-activate.

`ActivateOutcome` arms drive the activate-flow result panel:

| Outcome | Panel |
|---------|-------|
| `active` | green "Activated" with timing + previous_activation_id |
| `rolled_back` | yellow "Activation failed; previous release restored" + diagnostics |
| `rollback_target_missing` | red "Rollback target missing — pick another release" |
| `rollback_unstartable` | red "Rollback target also failed — service stopped" |

### 3.5 Legacy migrate flow

1. Settings → Legacy → "Scan for legacy sing-box.service" calls
   `legacy.observe_service`.
2. If `detected = false`: render "No legacy unit found." Nothing else to do.
3. If `detected = true` and `config_path_kind = "system_path"`: show details
   + "Import & disable legacy" button.
4. Click → calls `legacy.migrate_service { step: prepare }` which returns
   bytes. The frontend writes the config bytes (and any sibling assets) to a
   user-readable temp path under the existing profile-store dir
   (`<store>/imports/legacy-<unit>-<timestamp>/`), then calls
   `profile.import_file` (or `profile.import_dir` when assets exist).
5. After import succeeds, the UI shows the new profile in the list and offers
   a "Cutover (stop + disable old unit)" button which calls
   `legacy.migrate_service { step: cutover }`.
6. After cutover succeeds, the UI redirects to Profiles, selects the imported
   profile, and prompts "Activate this profile now?".
7. If `config_path_kind = "user_or_ephemeral"` or `"unknown"`: explain the
   path-safety refusal and offer "Stop & disable only" (cutover without
   prepare). Importing happens manually via the normal Profiles flow.

The temp-path write is deliberately filesystem-side, not memory-side —
keeping the user-side import path single (file/dir) means we exercise the
plan #4 path-safety classifier on the imported assets. Temp dirs are cleaned
up after import succeeds; on failure they remain for inspection.

### 3.6 Activate flow

1. Profiles → select → Activate tab.
2. Show: profile id (last 8), config sha (last 8), source kind, redacted URL.
3. Show: core path (default
   `/var/lib/boxpilot/cores/current/sing-box`), core version. Both editable
   text inputs with sane defaults from `home.status`.
4. Buttons (gated by single-flight):
   - **Best-effort check** — runs `profile_check`, shows OK / FAIL with
     stderr tail.
   - **Prepare bundle (preview)** — runs `profile_prepare_bundle`, shows the
     manifest in a collapsible JSON block.
   - **Activate** — confirms (modal: "this will restart sing-box") then runs
     `profile_activate` with default `verify_window_secs = 5`. Result
     dispatches to one of the four outcome panels.
5. After `active`, the Home view's poll picks up the new active profile on
   the next 5-second tick. The Activate tab also re-reads `home.status`
   immediately to update the Home card faster.

### 3.7 Manual rollback

Settings → Service tab has a "Roll back to a previous release" group:
text input for `target_activation_id`, a "Roll back" button, identical
outcome dispatch. v1 does not list past activation IDs — the user copies it
from the Home view's "previous_activation_id" field on the active-profile
card. (Plan #7's deferred work: a release.list method that lets us render a
real picker. v1 documents the activation_id format
`YYYY-MM-DDTHH-MM-SSZ-<6char>` so users can recognize them.)

### 3.8 Clash API enable

Profile editor surfaces a single button "Enable Clash API on loopback (port
9090)". Clicking it calls `profile_apply_patch_json` with:

```json
{
  "experimental": {
    "clash_api": {
      "external_controller": "127.0.0.1:9090",
      "secret": ""
    }
  }
}
```

The button is disabled when the JSON already contains
`experimental.clash_api.external_controller`. After patch, the editor
textarea reloads via `profile_get_source` so the user can see what changed.
Spec §12's "loopback only" requirement is satisfied by hard-coding
127.0.0.1.

## 4. Wire format

### 4.1 `boxpilot_ipc::home::HomeStatusResponse`

```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HomeStatusResponse {
    pub schema_version: u32, // 1
    pub service: ServiceStatusResponse,
    pub active_profile: Option<ActiveProfileSnapshot>,
    pub core: CoreSnapshot,
    pub active_corrupt: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActiveProfileSnapshot {
    pub profile_id: String,
    pub profile_name: Option<String>,
    pub profile_sha256: String,
    pub release_id: String,
    pub activated_at: String, // RFC3339
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CoreSnapshot {
    pub path: Option<String>,
    pub state: Option<CoreState>,
    /// Best-effort `<path> version` parse; "unknown" on failure.
    pub version: String,
}
```

`schema_version` lets future plans bump this struct without breaking the
GUI's parse — the wire pattern matches `BoxpilotConfig` and `InstallState`.

### 4.2 D-Bus method

`HomeStatus()` → `s` (JSON `HomeStatusResponse`). No request body. Same
chokepoint pattern as `service_status`.

### 4.3 Polkit action

```xml
<action id="app.boxpilot.helper.home.status">
  <description>View BoxPilot home status</description>
  <message>Authentication is required to view BoxPilot home status</message>
  <defaults>
    <allow_any>yes</allow_any>
    <allow_inactive>yes</allow_inactive>
    <allow_active>yes</allow_active>
  </defaults>
</action>
```

## 5. Risks and mitigations

| Risk | Mitigation |
|------|------------|
| 5-second poll burns battery / wakes radios on idle laptops | Pause polling when the window is hidden via `document.visibilitychange`. |
| `core::discover` is slow on cold caches | `home.status` reuses whatever `discover` already does (≤10 ms typical); if discover errors we still return service+config and set `version: "unknown"` so the rest of the page renders. |
| Legacy migrate import writes to user-readable disk before profile store accepts it | Use `tempfile::TempDir` under the existing store path; clean up on success or import error. Same fs-permission posture as plan #4's import_dir. |
| Frontend rewrite breaks the smoke procedure | Keep the existing 3 component file names (`ServicePanel.vue`, `ProfilesPanel.vue`, `CoresPanel.vue`) deleted only after all replacements compile and the smoke procedure is updated in the same PR. |
| `home.status` increases mainline polkit chatter | Auth class is ReadOnly, allow_any=yes — no auth prompt; polkit still logs but the dispatch::authorize chokepoint is unchanged. |
| User pastes garbage JSON in editor and clicks Activate | `profile_check` runs against the staged release before activation; any parse / schema error surfaces in the FAIL banner. The unmodified `experimental.clash_api` patch flow still goes through `editor::apply_patch` which preserves unknown fields. |

## 6. Open question deferred decisions

- **Service unit drift detection**: spec §3.1 mentions "drift warnings when
  the runtime state no longer matches BoxPilot metadata." For v1 we surface
  drift only for the three signals we already have (controller orphaned,
  active corrupt, unit not found / unit failed). A richer drift definition
  needs a dedicated method (e.g. compare `unit.fragment_path` against
  `paths.unit_path()`); deferred.
- **Real-time logs streaming**: spec §3.1 names "recent journal tail" only.
  v1 does on-demand fetches (button). A push subscription is a plan #8
  candidate.

## 7. Delivery checklist

- One PR titled `feat: GUI shell — Home/Profiles/Settings (plan #7)`.
- Smoke procedure document at
  `docs/superpowers/plans/2026-04-30-gui-shell-smoke-procedure.md`
  describing the 8 manual flows: home render, refresh, activate flow, rollback,
  legacy detect / migrate, clash-API patch, drift banners, repair prompt.
- All boxpilot-* unit tests pass; new `boxpilot-ipc::home` round-trip tests
  pass; `boxpilotd::iface::home_status_*` tests pass.
- `cargo fmt`, `cargo clippy --workspace --all-targets -- -D warnings`,
  `cargo test --workspace`, `cd frontend && npm run build` all green.
- `xmllint` validates the updated polkit policy.
