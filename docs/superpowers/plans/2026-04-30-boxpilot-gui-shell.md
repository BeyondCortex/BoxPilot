# BoxPilot GUI Shell (plan #7) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Turn the prototype 3-tab Vue UI into the v1.0 Home / Profiles / Settings shell from spec §3, exposing every privileged operation already shipped by plans #1–#6 plus a single new read-only IPC method (`home.status`).

**Architecture:** One new IPC method (`home.status`) batches the data the Home page needs in a single round-trip; the rest of the work is a Vue 3 frontend rewrite organised into `views/`, `components/`, and `composables/`. State lives in lightweight composables consumed via `provide`/`inject`; no `vue-router`, no Pinia. Polling pauses while the window is hidden.

**Tech Stack:** Rust 2021 / `zbus` 5 / Tauri 2 / Vue 3.5 (Composition API) / TypeScript 5 / Vite 5. No new workspace deps; no new npm deps.

**Spec reference:** `docs/superpowers/specs/2026-04-30-gui-shell-design.md`. Use the parent design at `docs/superpowers/specs/2026-04-27-boxpilot-linux-design.md` (§3 / §6.6 / §10) when in doubt.

---

## File map

**Create (Rust):**
- `crates/boxpilot-ipc/src/home.rs`
- (Tests added inline at bottom of `home.rs`.)

**Modify (Rust):**
- `crates/boxpilot-ipc/src/lib.rs` — declare/re-export the `home` module.
- `crates/boxpilot-ipc/src/method.rs` — add `HelperMethod::HomeStatus`.
- `crates/boxpilotd/src/iface.rs` — add `HomeStatus` D-Bus method + `do_home_status` body.
- `crates/boxpilot-tauri/src/helper_client.rs` — add `home_status` client wrapper.
- `crates/boxpilot-tauri/src/commands.rs` — add `helper_home_status` Tauri command.
- `crates/boxpilot-tauri/src/lib.rs` — register the new command.
- `packaging/linux/polkit-1/actions/app.boxpilot.helper.policy` — add the new action.

**Create (Frontend):**
- `frontend/src/composables/useToast.ts`
- `frontend/src/composables/useBusy.ts`
- `frontend/src/composables/useHomeStatus.ts`
- `frontend/src/composables/useProfiles.ts`
- `frontend/src/composables/useCores.ts`
- `frontend/src/components/AppShell.vue`
- `frontend/src/components/Toast.vue`
- `frontend/src/views/HomeView.vue`
- `frontend/src/views/ProfilesView.vue`
- `frontend/src/views/SettingsView.vue`
- `frontend/src/components/home/ServiceCard.vue`
- `frontend/src/components/home/ActiveProfileCard.vue`
- `frontend/src/components/home/CoreCard.vue`
- `frontend/src/components/home/DriftBanner.vue`
- `frontend/src/components/home/LogsPanel.vue`
- `frontend/src/components/home/QuickActions.vue`
- `frontend/src/components/profiles/ProfileList.vue`
- `frontend/src/components/profiles/ProfileDetail.vue`
- `frontend/src/components/profiles/ProfileOverview.vue`
- `frontend/src/components/profiles/ProfileEditor.vue`
- `frontend/src/components/profiles/ProfileActivate.vue`
- `frontend/src/components/settings/CoresTab.vue`
- `frontend/src/components/settings/ServiceTab.vue`
- `frontend/src/components/settings/LegacyTab.vue`
- `frontend/src/components/settings/AboutTab.vue`

**Modify (Frontend):**
- `frontend/src/api/types.ts` — add `HomeStatusResponse`, `ActiveProfileSnapshot`, `CoreSnapshot`.
- `frontend/src/api/helper.ts` — add `homeStatus` wrapper.
- `frontend/src/App.vue` — replace with the new shell selector.
- `frontend/src/main.ts` — unchanged unless needed.

**Delete (Frontend):**
- `frontend/src/components/ServicePanel.vue`
- `frontend/src/components/ProfilesPanel.vue`
- `frontend/src/components/CoresPanel.vue`

**Create (Docs):**
- `docs/superpowers/plans/2026-04-30-gui-shell-smoke-procedure.md`

---

## Task 1: IPC types — `home.rs`

**Files:**
- Create: `crates/boxpilot-ipc/src/home.rs`
- Modify: `crates/boxpilot-ipc/src/lib.rs`

- [ ] **Step 1: Create `home.rs` with types**

Write `crates/boxpilot-ipc/src/home.rs`:

```rust
use crate::config::CoreState;
use crate::response::ServiceStatusResponse;
use serde::{Deserialize, Serialize};

pub const HOME_STATUS_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HomeStatusResponse {
    pub schema_version: u32,
    pub service: ServiceStatusResponse,
    #[serde(default)]
    pub active_profile: Option<ActiveProfileSnapshot>,
    pub core: CoreSnapshot,
    pub active_corrupt: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActiveProfileSnapshot {
    pub profile_id: String,
    #[serde(default)]
    pub profile_name: Option<String>,
    pub profile_sha256: String,
    pub release_id: String,
    pub activated_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CoreSnapshot {
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub state: Option<CoreState>,
    pub version: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::response::UnitState;
    use pretty_assertions::assert_eq;

    fn sample_service() -> ServiceStatusResponse {
        ServiceStatusResponse {
            unit_name: "boxpilot-sing-box.service".into(),
            unit_state: UnitState::NotFound,
            controller: crate::response::ControllerStatus::Unset,
        }
    }

    #[test]
    fn home_status_round_trips_with_active_profile() {
        let r = HomeStatusResponse {
            schema_version: HOME_STATUS_SCHEMA_VERSION,
            service: sample_service(),
            active_profile: Some(ActiveProfileSnapshot {
                profile_id: "p-1".into(),
                profile_name: Some("Daily".into()),
                profile_sha256: "abc".into(),
                release_id: "rel-1".into(),
                activated_at: "2026-04-30T00:00:00-07:00".into(),
            }),
            core: CoreSnapshot {
                path: Some("/var/lib/boxpilot/cores/current/sing-box".into()),
                state: Some(CoreState::ManagedInstalled),
                version: "1.10.0".into(),
            },
            active_corrupt: false,
        };
        let s = serde_json::to_string(&r).unwrap();
        let back: HomeStatusResponse = serde_json::from_str(&s).unwrap();
        assert_eq!(back, r);
    }

    #[test]
    fn home_status_round_trips_unactivated() {
        let r = HomeStatusResponse {
            schema_version: HOME_STATUS_SCHEMA_VERSION,
            service: sample_service(),
            active_profile: None,
            core: CoreSnapshot {
                path: None,
                state: None,
                version: "unknown".into(),
            },
            active_corrupt: false,
        };
        let s = serde_json::to_string(&r).unwrap();
        let back: HomeStatusResponse = serde_json::from_str(&s).unwrap();
        assert_eq!(back, r);
    }

    #[test]
    fn home_status_active_corrupt_flag_round_trips() {
        let r = HomeStatusResponse {
            schema_version: HOME_STATUS_SCHEMA_VERSION,
            service: sample_service(),
            active_profile: None,
            core: CoreSnapshot {
                path: None,
                state: None,
                version: "unknown".into(),
            },
            active_corrupt: true,
        };
        let s = serde_json::to_string(&r).unwrap();
        let back: HomeStatusResponse = serde_json::from_str(&s).unwrap();
        assert!(back.active_corrupt);
    }
}
```

- [ ] **Step 2: Re-export from `lib.rs`**

Find the existing `pub mod legacy;` block in `crates/boxpilot-ipc/src/lib.rs` and add ABOVE the `#[cfg(test)]` block at the bottom:

```rust
pub mod home;
pub use home::{
    ActiveProfileSnapshot, CoreSnapshot, HomeStatusResponse, HOME_STATUS_SCHEMA_VERSION,
};
```

- [ ] **Step 3: Run boxpilot-ipc tests**

Run: `cargo test -p boxpilot-ipc`
Expected: all green, +3 tests in `home::tests`.

- [ ] **Step 4: Commit**

```bash
git add crates/boxpilot-ipc/src/home.rs crates/boxpilot-ipc/src/lib.rs
git commit -m "feat(ipc): home.status response types"
```

---

## Task 2: HelperMethod variant and polkit action

**Files:**
- Modify: `crates/boxpilot-ipc/src/method.rs`
- Modify: `packaging/linux/polkit-1/actions/app.boxpilot.helper.policy`

- [ ] **Step 1: Add the `HomeStatus` variant**

Open `crates/boxpilot-ipc/src/method.rs`. Find the existing `#[serde(rename = "service.status")] ServiceStatus,` line. Add immediately AFTER `LegacyMigrateService`:

```rust
    #[serde(rename = "home.status")]
    HomeStatus,
```

Then, in the same file, find `pub const ALL: [HelperMethod; 19]` and bump it to `20`, appending `HelperMethod::HomeStatus,` to the array.

In the `as_logical` match, append:

```rust
            HomeStatus => "home.status",
```

In the `auth_class` match, the `ServiceStatus | ServiceLogs | CoreDiscover | LegacyObserveService` arm should be extended to include `HomeStatus`:

```rust
            ServiceStatus | ServiceLogs | CoreDiscover | LegacyObserveService | HomeStatus => {
                AuthClass::ReadOnly
            }
```

In the `polkit_action_id` match, append:

```rust
            HomeStatus => "app.boxpilot.helper.home.status",
```

In the `count_matches_spec` test (top of `mod tests`), bump the assertion to `20`:

```rust
        assert_eq!(HelperMethod::ALL.len(), 20);
```

- [ ] **Step 2: Add the polkit action**

Open `packaging/linux/polkit-1/actions/app.boxpilot.helper.policy`. After the `<action id="app.boxpilot.helper.legacy.observe-service">` block (which is in the read-only section), insert:

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

- [ ] **Step 3: Verify polkit XML still parses**

Run: `xmllint --noout packaging/linux/polkit-1/actions/app.boxpilot.helper.policy`
Expected: no output (success).

- [ ] **Step 4: Run boxpilot-ipc method tests**

Run: `cargo test -p boxpilot-ipc method`
Expected: all green; the count assertion now expects 20.

- [ ] **Step 5: Commit**

```bash
git add crates/boxpilot-ipc/src/method.rs packaging/linux/polkit-1/actions/app.boxpilot.helper.policy
git commit -m "feat(ipc): home.status HelperMethod + polkit action"
```

---

## Task 3: `boxpilotd` — implement `home.status`

**Files:**
- Modify: `crates/boxpilotd/src/iface.rs`

- [ ] **Step 1: Add the D-Bus method to the iface**

Open `crates/boxpilotd/src/iface.rs`. Find the `service_status` D-Bus method (it sits early in the `#[interface(name = "app.boxpilot.Helper1")] impl Helper { ... }` block, around line 86). Insert AFTER the closing brace of `service_status` and BEFORE `service_start`:

```rust
    async fn home_status(
        &self,
        #[zbus(header)] header: zbus::message::Header<'_>,
    ) -> zbus::fdo::Result<String> {
        let sender = extract_sender(&header)?;
        let resp = self.do_home_status(&sender).await.map_err(to_zbus_err)?;
        serde_json::to_string(&resp).map_err(|e| {
            zbus::fdo::Error::Failed(format!("app.boxpilot.Helper1.Ipc: serialize: {e}"))
        })
    }
```

- [ ] **Step 2: Add the `do_home_status` body**

In the same file, find `async fn do_service_status` (around line 369). Insert immediately AFTER its closing brace and BEFORE `do_service_start`:

```rust
    async fn do_home_status(
        &self,
        sender_bus_name: &str,
    ) -> Result<boxpilot_ipc::HomeStatusResponse, HelperError> {
        let call =
            dispatch::authorize(&self.ctx, sender_bus_name, HelperMethod::HomeStatus).await?;

        // Service: same shape as do_service_status, but using the call we
        // just authorized so we don't re-run polkit.
        let cfg = self.ctx.load_config().await?;
        let unit_name = cfg.target_service.clone();
        let unit_state = self.ctx.systemd.unit_state(&unit_name).await?;
        let controller = call.controller.to_status();
        let service = boxpilot_ipc::ServiceStatusResponse {
            unit_name,
            unit_state,
            controller,
        };

        // Active profile: read straight from boxpilot.toml.
        let active_profile = match (
            cfg.active_profile_id.as_ref(),
            cfg.active_profile_sha256.as_ref(),
            cfg.active_release_id.as_ref(),
            cfg.activated_at.as_ref(),
        ) {
            (Some(id), Some(sha), Some(rel), Some(at)) => {
                Some(boxpilot_ipc::ActiveProfileSnapshot {
                    profile_id: id.clone(),
                    profile_name: cfg.active_profile_name.clone(),
                    profile_sha256: sha.clone(),
                    release_id: rel.clone(),
                    activated_at: at.clone(),
                })
            }
            _ => None,
        };

        // Core: discover and find the entry whose path matches cfg.core_path.
        // Discovery failure is non-fatal — the rest of the page still renders.
        let core_version = match self.discover_for_home().await {
            Ok(list) => cfg
                .core_path
                .as_deref()
                .and_then(|p| list.cores.iter().find(|c| c.path == p))
                .map(|c| c.version.clone())
                .unwrap_or_else(|| "unknown".to_string()),
            Err(_) => "unknown".to_string(),
        };
        let core = boxpilot_ipc::CoreSnapshot {
            path: cfg.core_path.clone(),
            state: cfg.core_state,
            version: core_version,
        };

        // Active corrupt: /etc/boxpilot/active should resolve under
        // releases/. Mirrors the daemon-startup recovery check.
        let paths = self.ctx.paths.clone();
        let active_corrupt = match tokio::fs::read_link(paths.active_symlink()).await {
            Ok(target) => {
                let releases = paths.releases_dir();
                !target.starts_with(&releases) || tokio::fs::metadata(&target).await.is_err()
            }
            Err(_) => false, // never activated: not corrupt, just absent
        };

        Ok(boxpilot_ipc::HomeStatusResponse {
            schema_version: boxpilot_ipc::HOME_STATUS_SCHEMA_VERSION,
            service,
            active_profile,
            core,
            active_corrupt,
        })
    }

    async fn discover_for_home(
        &self,
    ) -> Result<boxpilot_ipc::CoreDiscoverResponse, HelperError> {
        let deps = crate::core::discover::DiscoverDeps {
            paths: self.ctx.paths.clone(),
            fs: &*self.ctx.fs_meta,
            version_checker: &*self.ctx.version_checker,
        };
        crate::core::discover::discover(&deps).await
    }
```

The pattern matches `do_service_status` exactly: `dispatch::authorize` returns
a `call` whose `controller` field can be `to_status()`'d into the IPC type.

- [ ] **Step 3: Add unit tests for `do_home_status`**

In `crates/boxpilotd/src/iface.rs`, find the existing `#[cfg(test)] mod tests`
block (around line 704) and append two new tests next to the existing
`service_status_*` tests. They mirror those exactly, using the same
`ctx_with` / `CannedAuthority` builders:

```rust
    #[tokio::test]
    async fn home_status_returns_unactivated_when_config_lacks_active_fields() {
        let tmp = tempdir().unwrap();
        let ctx = Arc::new(ctx_with(
            &tmp,
            None,
            CannedAuthority::allowing(&["app.boxpilot.helper.home.status"]),
            UnitState::NotFound,
            &[(":1.42", 1000)],
        ));
        let h = Helper::new(ctx);
        let resp = h.do_home_status(":1.42").await.unwrap();
        assert!(resp.active_profile.is_none());
        assert_eq!(resp.schema_version, 1);
        assert!(!resp.active_corrupt);
        assert_eq!(resp.service.unit_name, "boxpilot-sing-box.service");
    }

    #[tokio::test]
    async fn home_status_denied_by_polkit_returns_not_authorized() {
        let tmp = tempdir().unwrap();
        let ctx = Arc::new(ctx_with(
            &tmp,
            None,
            CannedAuthority::denying(&["app.boxpilot.helper.home.status"]),
            UnitState::NotFound,
            &[(":1.42", 1000)],
        ));
        let h = Helper::new(ctx);
        let r = h.do_home_status(":1.42").await;
        assert!(matches!(r, Err(HelperError::NotAuthorized)));
    }
```

- [ ] **Step 4: Run boxpilotd tests**

Run: `cargo test -p boxpilotd`
Expected: all existing tests still pass, +2 new home_status tests pass.

- [ ] **Step 5: Commit**

```bash
git add crates/boxpilotd/src/iface.rs
git commit -m "feat(boxpilotd): home.status method"
```

---

## Task 4: Tauri command + helper_client wrapper

**Files:**
- Modify: `crates/boxpilot-tauri/src/helper_client.rs`
- Modify: `crates/boxpilot-tauri/src/commands.rs`
- Modify: `crates/boxpilot-tauri/src/lib.rs`

- [ ] **Step 1: Add `home_status` to `helper_client.rs`**

Open `crates/boxpilot-tauri/src/helper_client.rs`. Two changes:

(a) Inside the `trait Helper` proxy block (the `#[proxy(...)] trait Helper { ... }`), add:

```rust
    #[zbus(name = "HomeStatus")]
    fn home_status(&self) -> zbus::Result<String>;
```

Place it right after the existing `fn service_status` line.

(b) Inside the `impl HelperClient` block, add a wrapper method right after `service_status`:

```rust
    pub async fn home_status(&self) -> Result<boxpilot_ipc::HomeStatusResponse, ClientError> {
        let proxy = HelperProxy::new(&self.conn).await?;
        let json = proxy.home_status().await?;
        serde_json::from_str(&json).map_err(|e| ClientError::Decode(e.to_string()))
    }
```

- [ ] **Step 2: Add `helper_home_status` Tauri command**

Open `crates/boxpilot-tauri/src/commands.rs`. After `helper_service_status`, insert:

```rust
#[tauri::command]
pub async fn helper_home_status() -> Result<boxpilot_ipc::HomeStatusResponse, CommandError> {
    let client = HelperClient::connect().await?;
    Ok(client.home_status().await?)
}
```

- [ ] **Step 3: Register the command**

Open `crates/boxpilot-tauri/src/lib.rs`. In `tauri::generate_handler![...]` array, after `commands::helper_service_status,`, add:

```rust
            commands::helper_home_status,
```

- [ ] **Step 4: Build the workspace**

Run: `cargo build --workspace`
Expected: success.

- [ ] **Step 5: Commit**

```bash
git add crates/boxpilot-tauri
git commit -m "feat(tauri): home_status command + helper_client wrapper"
```

---

## Task 5: Frontend types + API wrappers

**Files:**
- Modify: `frontend/src/api/types.ts`
- Modify: `frontend/src/api/helper.ts`

- [ ] **Step 1: Add the new TypeScript types**

Open `frontend/src/api/types.ts`. Append at the bottom:

```ts
export type CoreState = "external" | "managed-installed" | "managed-adopted";

export interface ActiveProfileSnapshot {
  profile_id: string;
  profile_name: string | null;
  profile_sha256: string;
  release_id: string;
  activated_at: string;
}

export interface CoreSnapshot {
  path: string | null;
  state: CoreState | null;
  version: string;
}

export interface HomeStatusResponse {
  schema_version: number;
  service: ServiceStatusResponse;
  active_profile: ActiveProfileSnapshot | null;
  core: CoreSnapshot;
  active_corrupt: boolean;
}

export interface RollbackArgs {
  target_activation_id: string;
  verify_window_secs?: number | null;
}

export interface ActivateRequest {
  profile_id: string;
  core_path: string;
  core_version: string;
  verify_window_secs?: number | null;
}

export interface ActivateResponse {
  outcome: "active" | "rolled_back" | "rollback_target_missing" | "rollback_unstartable";
  activation_id: string;
  previous_activation_id: string | null;
  n_restarts_pre: number;
  n_restarts_post: number;
  window_used_ms: number;
}
```

- [ ] **Step 2: Add API wrappers**

Open `frontend/src/api/helper.ts`. Add to the imports list at the top:

```ts
  HomeStatusResponse,
```

Append at the bottom:

```ts
export async function homeStatus(): Promise<HomeStatusResponse> {
  return await invoke<HomeStatusResponse>("helper_home_status");
}
```

Open `frontend/src/api/profile.ts`. Add `ActivateRequest, ActivateResponse, RollbackArgs` to the type import list. Append:

```ts
export async function profileActivate(req: ActivateRequest): Promise<ActivateResponse> {
  return await invoke<ActivateResponse>("profile_activate", { request: req });
}
export async function profileRollback(req: RollbackArgs): Promise<ActivateResponse> {
  return await invoke<ActivateResponse>("profile_rollback", { request: req });
}
```

- [ ] **Step 3: Build the frontend**

Run: `cd frontend && npm run build`
Expected: success (vue-tsc + vite build, no new errors).

- [ ] **Step 4: Commit**

```bash
git add frontend/src/api
git commit -m "feat(frontend): home_status + activate/rollback API wrappers"
```

---

## Task 6: Composables

**Files:**
- Create: `frontend/src/composables/useToast.ts`
- Create: `frontend/src/composables/useBusy.ts`
- Create: `frontend/src/composables/useHomeStatus.ts`
- Create: `frontend/src/composables/useProfiles.ts`
- Create: `frontend/src/composables/useCores.ts`

- [ ] **Step 1: Write `useToast.ts`**

```ts
import { ref } from "vue";

export interface ToastEntry {
  id: number;
  kind: "info" | "success" | "error";
  message: string;
}

const entries = ref<ToastEntry[]>([]);
let nextId = 1;

function push(kind: ToastEntry["kind"], message: string) {
  const id = nextId++;
  entries.value.push({ id, kind, message });
  setTimeout(() => {
    entries.value = entries.value.filter((e) => e.id !== id);
  }, kind === "error" ? 8000 : 4000);
}

export function useToast() {
  return {
    entries,
    info: (m: string) => push("info", m),
    success: (m: string) => push("success", m),
    error: (m: string) => push("error", m),
    dismiss: (id: number) => {
      entries.value = entries.value.filter((e) => e.id !== id);
    },
  };
}
```

- [ ] **Step 2: Write `useBusy.ts`**

```ts
import { ref } from "vue";

export function useBusy() {
  const busy = ref(false);
  async function run<T>(fn: () => Promise<T>): Promise<T | undefined> {
    if (busy.value) return undefined;
    busy.value = true;
    try {
      return await fn();
    } finally {
      busy.value = false;
    }
  }
  return { busy, run };
}
```

- [ ] **Step 3: Write `useHomeStatus.ts`**

```ts
import { onMounted, onUnmounted, ref } from "vue";
import { homeStatus } from "../api/helper";
import type { HomeStatusResponse } from "../api/types";

const POLL_MS = 5000;

export function useHomeStatus() {
  const data = ref<HomeStatusResponse | null>(null);
  const error = ref<string | null>(null);
  let timer: number | null = null;
  let stopped = false;

  async function refresh() {
    try {
      data.value = await homeStatus();
      error.value = null;
    } catch (e: any) {
      error.value = e?.message ?? String(e);
    }
  }

  function schedule() {
    if (stopped) return;
    if (document.hidden) {
      timer = window.setTimeout(schedule, POLL_MS);
      return;
    }
    refresh().finally(() => {
      if (!stopped) timer = window.setTimeout(schedule, POLL_MS);
    });
  }

  function onVisibility() {
    if (!document.hidden) refresh();
  }

  onMounted(() => {
    document.addEventListener("visibilitychange", onVisibility);
    schedule();
  });

  onUnmounted(() => {
    stopped = true;
    if (timer !== null) window.clearTimeout(timer);
    document.removeEventListener("visibilitychange", onVisibility);
  });

  return { data, error, refresh };
}
```

- [ ] **Step 4: Write `useProfiles.ts`**

```ts
import { ref } from "vue";
import { profileList } from "../api/profile";
import type { ProfileSummary } from "../api/types";

const profiles = ref<ProfileSummary[]>([]);
const error = ref<string | null>(null);

async function refresh() {
  try {
    profiles.value = await profileList();
    error.value = null;
  } catch (e: any) {
    error.value = e?.message ?? String(e);
  }
}

export function useProfiles() {
  return { profiles, error, refresh };
}
```

- [ ] **Step 5: Write `useCores.ts`**

```ts
import { ref } from "vue";
import { coreDiscover } from "../api/helper";
import type { CoreDiscoverResponse } from "../api/types";

const data = ref<CoreDiscoverResponse | null>(null);
const error = ref<string | null>(null);

async function refresh() {
  try {
    data.value = await coreDiscover();
    error.value = null;
  } catch (e: any) {
    error.value = e?.message ?? String(e);
  }
}

export function useCores() {
  return { data, error, refresh };
}
```

- [ ] **Step 6: Build the frontend**

Run: `cd frontend && npm run build`
Expected: success.

- [ ] **Step 7: Commit**

```bash
git add frontend/src/composables
git commit -m "feat(frontend): composables (toast/busy/home/profiles/cores)"
```

---

## Task 7: AppShell + Toast + new App.vue

**Files:**
- Create: `frontend/src/components/AppShell.vue`
- Create: `frontend/src/components/Toast.vue`
- Modify: `frontend/src/App.vue`

- [ ] **Step 1: Write `Toast.vue`**

```vue
<script setup lang="ts">
import { useToast } from "../composables/useToast";
const { entries, dismiss } = useToast();
</script>

<template>
  <div class="toast-stack">
    <div
      v-for="e in entries"
      :key="e.id"
      class="toast"
      :class="e.kind"
      @click="dismiss(e.id)"
    >
      {{ e.message }}
    </div>
  </div>
</template>

<style scoped>
.toast-stack {
  position: fixed;
  bottom: 1rem;
  right: 1rem;
  display: flex;
  flex-direction: column;
  gap: 0.5rem;
  z-index: 1000;
}
.toast {
  padding: 0.6rem 0.9rem;
  border-radius: 4px;
  background: #333;
  color: #fff;
  font-size: 0.9rem;
  cursor: pointer;
  max-width: 28rem;
}
.toast.success { background: #1e7d3a; }
.toast.error { background: #b22; }
.toast.info { background: #345; }
</style>
```

- [ ] **Step 2: Write `AppShell.vue`**

```vue
<script setup lang="ts">
import { ref } from "vue";
import HomeView from "../views/HomeView.vue";
import ProfilesView from "../views/ProfilesView.vue";
import SettingsView from "../views/SettingsView.vue";
import Toast from "./Toast.vue";

type Tab = "home" | "profiles" | "settings";
const tab = ref<Tab>("home");
</script>

<template>
  <div class="shell">
    <header class="topbar">
      <h1>BoxPilot</h1>
      <nav>
        <button :class="{ active: tab === 'home' }" @click="tab = 'home'">Home</button>
        <button :class="{ active: tab === 'profiles' }" @click="tab = 'profiles'">Profiles</button>
        <button :class="{ active: tab === 'settings' }" @click="tab = 'settings'">Settings</button>
      </nav>
    </header>
    <main>
      <HomeView v-if="tab === 'home'" :switch-tab="(t: Tab) => (tab = t)" />
      <ProfilesView v-else-if="tab === 'profiles'" />
      <SettingsView v-else-if="tab === 'settings'" />
    </main>
    <Toast />
  </div>
</template>

<style scoped>
.shell {
  font-family: system-ui, sans-serif;
  max-width: 64rem;
  margin: 0 auto;
  padding: 1rem;
}
.topbar {
  display: flex;
  align-items: baseline;
  gap: 1.5rem;
  border-bottom: 1px solid #ddd;
  padding-bottom: 0.5rem;
  margin-bottom: 1rem;
}
.topbar h1 { margin: 0; font-size: 1.4rem; }
nav { display: flex; gap: 0.5rem; }
nav button {
  padding: 0.4rem 0.9rem;
  border: 1px solid #ccc;
  background: #f7f7f7;
  cursor: pointer;
  border-radius: 4px;
}
nav button.active { background: #333; color: #fff; border-color: #333; }
main { min-height: 70vh; }
</style>
```

- [ ] **Step 3: Replace `App.vue`**

Replace the entire contents of `frontend/src/App.vue` with:

```vue
<script setup lang="ts">
import AppShell from "./components/AppShell.vue";
</script>

<template>
  <AppShell />
</template>

<style>
body {
  margin: 0;
  background: #fafafa;
  color: #222;
}
</style>
```

- [ ] **Step 4: Build (will fail until views exist)**

Run: `cd frontend && npm run build`
Expected: FAIL — `views/HomeView.vue` etc. do not exist yet. This is fine; proceed to the next task.

- [ ] **Step 5: Commit**

```bash
git add frontend/src/App.vue frontend/src/components/AppShell.vue frontend/src/components/Toast.vue
git commit -m "feat(frontend): app shell + toast"
```

---

## Task 8: HomeView + home/* components

**Files:**
- Create: `frontend/src/views/HomeView.vue`
- Create: `frontend/src/components/home/ServiceCard.vue`
- Create: `frontend/src/components/home/ActiveProfileCard.vue`
- Create: `frontend/src/components/home/CoreCard.vue`
- Create: `frontend/src/components/home/DriftBanner.vue`
- Create: `frontend/src/components/home/LogsPanel.vue`
- Create: `frontend/src/components/home/QuickActions.vue`

- [ ] **Step 1: Write `ServiceCard.vue`**

```vue
<script setup lang="ts">
import type { ServiceStatusResponse } from "../../api/types";

defineProps<{ service: ServiceStatusResponse | null }>();

function dotColor(s: ServiceStatusResponse | null): string {
  if (!s || s.unit_state.kind === "not_found") return "#999";
  switch (s.unit_state.active_state) {
    case "active": return "#1e7d3a";
    case "failed": return "#b22";
    case "activating":
    case "deactivating":
    case "reloading": return "#cc8800";
    default: return "#888";
  }
}
</script>

<template>
  <section class="card">
    <h3>Service</h3>
    <div v-if="!service" class="muted">Loading…</div>
    <template v-else>
      <p class="row">
        <span class="dot" :style="{ background: dotColor(service) }"></span>
        <strong>{{ service.unit_name }}</strong>
        <span v-if="service.unit_state.kind === 'not_found'">not installed</span>
        <span v-else>
          {{ service.unit_state.active_state }} ({{ service.unit_state.sub_state }})
        </span>
      </p>
      <p v-if="service.unit_state.kind === 'known'" class="meta">
        load: {{ service.unit_state.load_state }} ·
        restarts: {{ service.unit_state.n_restarts }} ·
        exit: {{ service.unit_state.exec_main_status }}
      </p>
      <p class="meta">
        controller:
        <span v-if="service.controller.kind === 'set'">{{ service.controller.username }} (uid {{ service.controller.uid }})</span>
        <span v-else-if="service.controller.kind === 'orphaned'">orphaned uid {{ service.controller.uid }}</span>
        <span v-else>unset</span>
      </p>
    </template>
  </section>
</template>

<style scoped>
.card { background: #fff; border: 1px solid #e5e5e5; border-radius: 6px; padding: 1rem; }
.card h3 { margin: 0 0 0.5rem 0; font-size: 1rem; }
.row { display: flex; gap: 0.5rem; align-items: center; margin: 0.25rem 0; }
.dot { width: 10px; height: 10px; border-radius: 50%; display: inline-block; }
.meta { color: #666; font-size: 0.85rem; margin: 0.25rem 0; }
.muted { color: #888; }
</style>
```

- [ ] **Step 2: Write `ActiveProfileCard.vue`**

```vue
<script setup lang="ts">
import type { ActiveProfileSnapshot } from "../../api/types";

defineProps<{ active: ActiveProfileSnapshot | null }>();
</script>

<template>
  <section class="card">
    <h3>Active profile</h3>
    <p v-if="!active" class="muted">No profile activated yet.</p>
    <template v-else>
      <p class="row"><strong>{{ active.profile_name ?? active.profile_id }}</strong></p>
      <p class="meta">id: <code>{{ active.profile_id.slice(0, 12) }}…</code></p>
      <p class="meta">sha: <code>{{ active.profile_sha256.slice(0, 12) }}…</code></p>
      <p class="meta">release: <code>{{ active.release_id }}</code></p>
      <p class="meta">activated: {{ active.activated_at }}</p>
    </template>
  </section>
</template>

<style scoped>
.card { background: #fff; border: 1px solid #e5e5e5; border-radius: 6px; padding: 1rem; }
.card h3 { margin: 0 0 0.5rem 0; font-size: 1rem; }
.row { margin: 0.25rem 0; }
.meta { color: #666; font-size: 0.85rem; margin: 0.25rem 0; }
.muted { color: #888; }
code { background: #f0f0f0; padding: 0 0.25rem; border-radius: 2px; }
</style>
```

- [ ] **Step 3: Write `CoreCard.vue`**

```vue
<script setup lang="ts">
import type { CoreSnapshot } from "../../api/types";

defineProps<{ core: CoreSnapshot | null }>();
</script>

<template>
  <section class="card">
    <h3>Core</h3>
    <p v-if="!core" class="muted">Loading…</p>
    <template v-else>
      <p class="row">
        <strong>sing-box</strong> <code>{{ core.version }}</code>
      </p>
      <p class="meta">path: <code>{{ core.path ?? "(none)" }}</code></p>
      <p class="meta">state: {{ core.state ?? "(unset)" }}</p>
    </template>
  </section>
</template>

<style scoped>
.card { background: #fff; border: 1px solid #e5e5e5; border-radius: 6px; padding: 1rem; }
.card h3 { margin: 0 0 0.5rem 0; font-size: 1rem; }
.row { margin: 0.25rem 0; }
.meta { color: #666; font-size: 0.85rem; margin: 0.25rem 0; }
.muted { color: #888; }
code { background: #f0f0f0; padding: 0 0.25rem; border-radius: 2px; }
</style>
```

- [ ] **Step 4: Write `DriftBanner.vue`**

```vue
<script setup lang="ts">
import type { HomeStatusResponse } from "../../api/types";

const props = defineProps<{
  data: HomeStatusResponse | null;
  switchTab: (t: "home" | "profiles" | "settings") => void;
}>();

interface Banner { kind: "warn" | "error"; message: string; action?: { label: string; run: () => void } }

function banners(): Banner[] {
  const r: Banner[] = [];
  if (!props.data) return r;
  if (props.data.active_corrupt) {
    r.push({
      kind: "error",
      message:
        "BoxPilot's /etc/boxpilot/active link is corrupt. Activate a profile to restore.",
      action: { label: "Open Profiles", run: () => props.switchTab("profiles") },
    });
  }
  if (props.data.service.controller.kind === "orphaned") {
    r.push({
      kind: "warn",
      message: `Controller uid ${props.data.service.controller.uid} no longer exists. Privileged actions will be refused until a new controller is set.`,
    });
  }
  if (
    props.data.service.unit_state.kind === "known" &&
    props.data.service.unit_state.active_state === "failed"
  ) {
    r.push({
      kind: "error",
      message: "Service is in failed state. Inspect logs and consider rolling back.",
    });
  }
  return r;
}
</script>

<template>
  <div v-if="banners().length" class="banners">
    <div v-for="(b, i) in banners()" :key="i" class="banner" :class="b.kind">
      <span>{{ b.message }}</span>
      <button v-if="b.action" @click="b.action.run">{{ b.action.label }}</button>
    </div>
  </div>
</template>

<style scoped>
.banners { display: flex; flex-direction: column; gap: 0.5rem; margin-bottom: 1rem; }
.banner { padding: 0.6rem 0.9rem; border-radius: 4px; display: flex; gap: 1rem; align-items: center; }
.banner.warn { background: #fff3cd; color: #5a4400; border: 1px solid #f0d68a; }
.banner.error { background: #fde4e4; color: #6a1010; border: 1px solid #f0a0a0; }
.banner button { margin-left: auto; padding: 0.25rem 0.6rem; border-radius: 3px; border: 1px solid currentColor; background: transparent; color: inherit; cursor: pointer; }
</style>
```

- [ ] **Step 5: Write `LogsPanel.vue`**

```vue
<script setup lang="ts">
import { ref } from "vue";
import { serviceLogs } from "../../api/helper";
import { useBusy } from "../../composables/useBusy";
import { useToast } from "../../composables/useToast";

const lines = ref<string[]>([]);
const truncated = ref(false);
const open = ref(false);
const { busy, run } = useBusy();
const toast = useToast();

async function load() {
  await run(async () => {
    try {
      const r = await serviceLogs({ lines: 200 });
      lines.value = r.lines;
      truncated.value = r.truncated;
      open.value = true;
    } catch (e: any) {
      toast.error(`logs: ${e?.message ?? String(e)}`);
    }
  });
}
</script>

<template>
  <section class="card">
    <h3>Logs</h3>
    <button :disabled="busy" @click="load">
      {{ open ? "Refresh tail (200)" : "Load tail (200)" }}
    </button>
    <pre v-if="open" class="logs">{{ lines.join("\n") || "(no lines)" }}</pre>
    <p v-if="truncated" class="meta">(server clamped to 200)</p>
  </section>
</template>

<style scoped>
.card { background: #fff; border: 1px solid #e5e5e5; border-radius: 6px; padding: 1rem; }
.card h3 { margin: 0 0 0.5rem 0; font-size: 1rem; }
.logs { max-height: 24rem; overflow: auto; background: #111; color: #eee; padding: 0.5rem; font-size: 0.8rem; border-radius: 4px; }
.meta { color: #666; font-size: 0.85rem; }
</style>
```

- [ ] **Step 6: Write `QuickActions.vue`**

```vue
<script setup lang="ts">
import { serviceStart, serviceStop, serviceRestart } from "../../api/helper";
import { useBusy } from "../../composables/useBusy";
import { useToast } from "../../composables/useToast";

const props = defineProps<{ refresh: () => Promise<void> }>();
const { busy, run } = useBusy();
const toast = useToast();

async function call(name: string, fn: () => Promise<unknown>) {
  await run(async () => {
    try {
      await fn();
      toast.success(`${name} ok`);
      await props.refresh();
    } catch (e: any) {
      toast.error(`${name}: ${e?.message ?? String(e)}`);
    }
  });
}
</script>

<template>
  <section class="card">
    <h3>Quick actions</h3>
    <div class="actions">
      <button :disabled="busy" @click="call('start', serviceStart)">Start</button>
      <button :disabled="busy" @click="call('stop', serviceStop)">Stop</button>
      <button :disabled="busy" @click="call('restart', serviceRestart)">Restart</button>
      <button :disabled="busy" @click="props.refresh">Refresh</button>
    </div>
  </section>
</template>

<style scoped>
.card { background: #fff; border: 1px solid #e5e5e5; border-radius: 6px; padding: 1rem; }
.card h3 { margin: 0 0 0.5rem 0; font-size: 1rem; }
.actions { display: flex; flex-wrap: wrap; gap: 0.5rem; }
.actions button { padding: 0.4rem 0.9rem; border: 1px solid #bbb; border-radius: 4px; background: #f5f5f5; cursor: pointer; }
.actions button:hover:not(:disabled) { background: #e8e8e8; }
</style>
```

- [ ] **Step 7: Write `HomeView.vue`**

```vue
<script setup lang="ts">
import { useHomeStatus } from "../composables/useHomeStatus";
import ServiceCard from "../components/home/ServiceCard.vue";
import ActiveProfileCard from "../components/home/ActiveProfileCard.vue";
import CoreCard from "../components/home/CoreCard.vue";
import DriftBanner from "../components/home/DriftBanner.vue";
import LogsPanel from "../components/home/LogsPanel.vue";
import QuickActions from "../components/home/QuickActions.vue";

defineProps<{ switchTab: (t: "home" | "profiles" | "settings") => void }>();

const { data, error, refresh } = useHomeStatus();
</script>

<template>
  <div class="home">
    <p v-if="error" class="err">{{ error }}</p>
    <DriftBanner :data="data" :switch-tab="switchTab" />
    <div class="grid">
      <ServiceCard :service="data?.service ?? null" />
      <ActiveProfileCard :active="data?.active_profile ?? null" />
      <CoreCard :core="data?.core ?? null" />
    </div>
    <QuickActions :refresh="refresh" />
    <LogsPanel />
  </div>
</template>

<style scoped>
.home { display: flex; flex-direction: column; gap: 1rem; }
.grid { display: grid; grid-template-columns: repeat(auto-fit, minmax(18rem, 1fr)); gap: 1rem; }
.err { background: #fde4e4; color: #6a1010; padding: 0.5rem 0.8rem; border-radius: 4px; }
</style>
```

- [ ] **Step 8: Verify HomeView builds**

Run: `cd frontend && npm run build`
Expected: still fails (ProfilesView/SettingsView missing). Confirm error mentions only those, not the home/* files.

- [ ] **Step 9: Commit**

```bash
git add frontend/src/views/HomeView.vue frontend/src/components/home
git commit -m "feat(frontend): home view (service/profile/core cards + drift + logs)"
```

---

## Task 9: ProfilesView + ProfileList

**Files:**
- Create: `frontend/src/views/ProfilesView.vue`
- Create: `frontend/src/components/profiles/ProfileList.vue`

- [ ] **Step 1: Write `ProfileList.vue`**

```vue
<script setup lang="ts">
import { ref } from "vue";
import {
  profileImportFile, profileImportDir, profileImportRemote, profileRefreshRemote,
} from "../../api/profile";
import { useProfiles } from "../../composables/useProfiles";
import { useBusy } from "../../composables/useBusy";
import { useToast } from "../../composables/useToast";

const props = defineProps<{ selectedId: string | null }>();
const emit = defineEmits<{ (e: "select", id: string): void }>();

const { profiles, refresh } = useProfiles();
const { busy, run } = useBusy();
const toast = useToast();

const newName = ref("");
const newJsonPath = ref("");
const newDirPath = ref("");
const newRemoteUrl = ref("");

refresh();

async function importFile() {
  if (!newName.value || !newJsonPath.value) return;
  await run(async () => {
    try {
      await profileImportFile(newName.value, newJsonPath.value);
      newName.value = ""; newJsonPath.value = "";
      await refresh();
      toast.success("Imported");
    } catch (e: any) { toast.error(e?.message ?? String(e)); }
  });
}
async function importDir() {
  if (!newName.value || !newDirPath.value) return;
  await run(async () => {
    try {
      await profileImportDir(newName.value, newDirPath.value);
      newName.value = ""; newDirPath.value = "";
      await refresh();
      toast.success("Imported");
    } catch (e: any) { toast.error(e?.message ?? String(e)); }
  });
}
async function importRemote() {
  if (!newName.value || !newRemoteUrl.value) return;
  await run(async () => {
    try {
      await profileImportRemote(newName.value, newRemoteUrl.value);
      newName.value = ""; newRemoteUrl.value = "";
      await refresh();
      toast.success("Imported");
    } catch (e: any) { toast.error(e?.message ?? String(e)); }
  });
}
async function refreshOne(id: string) {
  await run(async () => {
    try {
      await profileRefreshRemote(id);
      await refresh();
      toast.success("Refreshed");
    } catch (e: any) { toast.error(e?.message ?? String(e)); }
  });
}
</script>

<template>
  <aside class="list">
    <h3>Profiles ({{ profiles.length }})</h3>
    <ul>
      <li v-for="p in profiles" :key="p.id" :class="{ active: p.id === props.selectedId }">
        <button class="select" @click="emit('select', p.id)">{{ p.name }}</button>
        <span class="meta">{{ p.source_kind }} · <code>{{ p.config_sha256.slice(0, 8) }}</code></span>
        <span v-if="p.remote_url_redacted" class="url" :title="p.remote_url_redacted">{{ p.remote_url_redacted }}</span>
        <button v-if="p.source_kind === 'remote'" :disabled="busy" @click="refreshOne(p.id)">↻</button>
      </li>
    </ul>
    <div class="add">
      <h4>Add</h4>
      <input v-model="newName" placeholder="Name" />
      <div class="row">
        <input v-model="newJsonPath" placeholder="/path/to/file.json" />
        <button :disabled="busy || !newName || !newJsonPath" @click="importFile">File</button>
      </div>
      <div class="row">
        <input v-model="newDirPath" placeholder="/path/to/profile-dir" />
        <button :disabled="busy || !newName || !newDirPath" @click="importDir">Dir</button>
      </div>
      <div class="row">
        <input v-model="newRemoteUrl" placeholder="https://host/...?token=" />
        <button :disabled="busy || !newName || !newRemoteUrl" @click="importRemote">Remote</button>
      </div>
    </div>
  </aside>
</template>

<style scoped>
.list { display: flex; flex-direction: column; gap: 1rem; }
.list h3 { margin: 0; font-size: 1rem; }
.list ul { list-style: none; padding: 0; margin: 0; display: flex; flex-direction: column; gap: 0.25rem; }
.list li { display: grid; grid-template-columns: 1fr auto auto; gap: 0.5rem; align-items: center; padding: 0.25rem 0.5rem; border: 1px solid #eee; border-radius: 4px; background: #fff; }
.list li.active { border-color: #333; }
.list li.active .select { font-weight: bold; }
.list li .select { background: none; border: none; padding: 0; cursor: pointer; text-align: left; }
.meta { color: #666; font-size: 0.8rem; grid-column: 1 / -1; }
.url { color: #557; font-family: monospace; font-size: 0.75rem; grid-column: 1 / -1; word-break: break-all; }
.add h4 { margin: 0 0 0.25rem; font-size: 0.85rem; }
.add input { width: 100%; box-sizing: border-box; padding: 0.3rem 0.4rem; margin-bottom: 0.25rem; }
.add .row { display: flex; gap: 0.4rem; align-items: center; }
.add .row input { flex: 1; margin-bottom: 0; }
</style>
```

- [ ] **Step 2: Write a stub `ProfilesView.vue` that compiles**

The full view will be filled in by tasks 10–12. For now just plug in `ProfileList`:

```vue
<script setup lang="ts">
import { ref } from "vue";
import ProfileList from "../components/profiles/ProfileList.vue";

const selectedId = ref<string | null>(null);
function onSelect(id: string) { selectedId.value = id; }
</script>

<template>
  <div class="profiles">
    <ProfileList :selected-id="selectedId" @select="onSelect" />
    <section class="detail">
      <p v-if="!selectedId" class="muted">Select a profile.</p>
      <p v-else>Selected: <code>{{ selectedId }}</code> (detail tabs coming up).</p>
    </section>
  </div>
</template>

<style scoped>
.profiles { display: grid; grid-template-columns: 22rem 1fr; gap: 1rem; align-items: start; }
.detail { background: #fff; border: 1px solid #e5e5e5; border-radius: 6px; padding: 1rem; min-height: 24rem; }
.muted { color: #888; }
</style>
```

- [ ] **Step 3: Build (still expects SettingsView; OK)**

Run: `cd frontend && npm run build`
Expected: fails only on SettingsView import.

- [ ] **Step 4: Commit**

```bash
git add frontend/src/views/ProfilesView.vue frontend/src/components/profiles/ProfileList.vue
git commit -m "feat(frontend): profile list + view stub"
```

---

## Task 10: ProfileOverview + ProfileEditor (with Clash-API)

**Files:**
- Create: `frontend/src/components/profiles/ProfileOverview.vue`
- Create: `frontend/src/components/profiles/ProfileEditor.vue`

- [ ] **Step 1: Write `ProfileOverview.vue`**

A read-only structured view. We parse the JSON client-side; if it fails to parse we fall back to a hint to use the editor.

```vue
<script setup lang="ts">
import { computed, watch, ref } from "vue";
import { profileGetSource } from "../../api/profile";

const props = defineProps<{ profileId: string }>();

const text = ref<string>("");
const error = ref<string | null>(null);

async function reload() {
  try {
    text.value = await profileGetSource(props.profileId);
    error.value = null;
  } catch (e: any) {
    error.value = e?.message ?? String(e);
  }
}

watch(() => props.profileId, reload, { immediate: true });

const parsed = computed(() => {
  try { return JSON.parse(text.value); }
  catch { return null; }
});

function listKind<T = any>(arr: any): T[] {
  return Array.isArray(arr) ? (arr as T[]) : [];
}

const inbounds = computed<any[]>(() => listKind(parsed.value?.inbounds));
const outbounds = computed<any[]>(() => listKind(parsed.value?.outbounds));
const ruleSets = computed<any[]>(() => listKind(parsed.value?.route?.rule_set));
const dnsServers = computed<any[]>(() => listKind(parsed.value?.dns?.servers));
const tunInbound = computed<any | null>(() => inbounds.value.find((i: any) => i?.type === "tun") ?? null);
const clashApi = computed<any | null>(() => parsed.value?.experimental?.clash_api ?? null);
</script>

<template>
  <div class="overview">
    <p v-if="error" class="err">read source: {{ error }}</p>
    <p v-else-if="!parsed" class="muted">Config is not valid JSON. Use the Editor tab to fix it.</p>
    <template v-else>
      <h4>Inbounds ({{ inbounds.length }})</h4>
      <ul>
        <li v-for="(i, idx) in inbounds" :key="idx">
          <code>{{ i.type ?? "(no type)" }}</code>
          <span v-if="i.tag"> · tag <code>{{ i.tag }}</code></span>
          <span v-if="i.listen_port"> · port {{ i.listen_port }}</span>
        </li>
      </ul>
      <h4 v-if="tunInbound">TUN settings</h4>
      <ul v-if="tunInbound" class="kv">
        <li>auto_route: <code>{{ tunInbound.auto_route ?? "(unset)" }}</code></li>
        <li>auto_redirect: <code>{{ tunInbound.auto_redirect ?? "(unset)" }}</code></li>
        <li>strict_route: <code>{{ tunInbound.strict_route ?? "(unset)" }}</code></li>
      </ul>
      <h4>Outbounds ({{ outbounds.length }})</h4>
      <ul>
        <li v-for="(o, idx) in outbounds" :key="idx">
          <code>{{ o.type ?? "(no type)" }}</code>
          <span v-if="o.tag"> · tag <code>{{ o.tag }}</code></span>
        </li>
      </ul>
      <h4>DNS servers ({{ dnsServers.length }})</h4>
      <ul>
        <li v-for="(s, idx) in dnsServers" :key="idx">
          <code>{{ s.tag ?? `#${idx}` }}</code>: <code>{{ s.address ?? s.url ?? "(?)" }}</code>
        </li>
      </ul>
      <h4>Route rule_set ({{ ruleSets.length }})</h4>
      <ul>
        <li v-for="(r, idx) in ruleSets" :key="idx">
          <code>{{ r.tag ?? `#${idx}` }}</code> ({{ r.type ?? r.format ?? "?" }})
        </li>
      </ul>
      <h4 v-if="clashApi">Clash API</h4>
      <ul v-if="clashApi" class="kv">
        <li>external_controller: <code>{{ clashApi.external_controller ?? "(unset)" }}</code></li>
        <li>secret set: <code>{{ clashApi.secret ? "yes" : "no" }}</code></li>
      </ul>
    </template>
  </div>
</template>

<style scoped>
.overview { display: flex; flex-direction: column; gap: 0.5rem; }
.overview h4 { margin: 0.5rem 0 0.25rem; font-size: 0.9rem; }
.overview ul { margin: 0; padding-left: 1.2rem; }
.overview .kv { list-style: none; padding-left: 0; }
.muted { color: #888; }
.err { background: #fde4e4; color: #6a1010; padding: 0.4rem 0.6rem; border-radius: 4px; }
code { background: #f0f0f0; padding: 0 0.25rem; border-radius: 2px; }
</style>
```

- [ ] **Step 2: Write `ProfileEditor.vue`**

```vue
<script setup lang="ts">
import { ref, watch, computed } from "vue";
import {
  profileGetSource, profileSaveSource, profileRevert, profileApplyPatchJson,
} from "../../api/profile";
import { useBusy } from "../../composables/useBusy";
import { useToast } from "../../composables/useToast";

const props = defineProps<{ profileId: string }>();
const emit = defineEmits<{ (e: "saved"): void }>();

const text = ref<string>("");
const original = ref<string>("");
const error = ref<string | null>(null);
const { busy, run } = useBusy();
const toast = useToast();

const dirty = computed(() => text.value !== original.value);

const clashAlreadyOn = computed(() => {
  try {
    const parsed = JSON.parse(text.value);
    return Boolean(parsed?.experimental?.clash_api?.external_controller);
  } catch { return false; }
});

async function load() {
  try {
    const s = await profileGetSource(props.profileId);
    text.value = s; original.value = s; error.value = null;
  } catch (e: any) {
    error.value = e?.message ?? String(e);
  }
}

watch(() => props.profileId, load, { immediate: true });

async function save() {
  await run(async () => {
    try {
      await profileSaveSource(props.profileId, text.value);
      original.value = text.value;
      toast.success("Saved");
      emit("saved");
    } catch (e: any) { toast.error(e?.message ?? String(e)); }
  });
}

async function revert() {
  await run(async () => {
    try {
      await profileRevert(props.profileId);
      await load();
      toast.success("Reverted to last-valid");
      emit("saved");
    } catch (e: any) { toast.error(e?.message ?? String(e)); }
  });
}

async function enableClash() {
  await run(async () => {
    try {
      const patch = JSON.stringify({
        experimental: {
          clash_api: { external_controller: "127.0.0.1:9090", secret: "" },
        },
      });
      await profileApplyPatchJson(props.profileId, patch);
      await load();
      toast.success("Clash API enabled on 127.0.0.1:9090");
      emit("saved");
    } catch (e: any) { toast.error(e?.message ?? String(e)); }
  });
}
</script>

<template>
  <div class="editor">
    <p v-if="error" class="err">{{ error }}</p>
    <textarea v-model="text" rows="20" spellcheck="false"></textarea>
    <div class="actions">
      <button :disabled="busy || !dirty" @click="save">Save</button>
      <button :disabled="busy" @click="revert">Revert to last-valid</button>
      <button
        :disabled="busy || clashAlreadyOn"
        :title="clashAlreadyOn ? 'Clash API is already configured' : 'Enable Clash API on 127.0.0.1:9090'"
        @click="enableClash"
      >
        Enable Clash API on loopback
      </button>
    </div>
  </div>
</template>

<style scoped>
.editor { display: flex; flex-direction: column; gap: 0.5rem; }
.editor textarea { width: 100%; box-sizing: border-box; font-family: ui-monospace, monospace; font-size: 0.85rem; padding: 0.5rem; }
.actions { display: flex; flex-wrap: wrap; gap: 0.5rem; }
.actions button { padding: 0.4rem 0.9rem; border: 1px solid #bbb; border-radius: 4px; background: #f5f5f5; cursor: pointer; }
.actions button:disabled { opacity: 0.5; cursor: default; }
.err { background: #fde4e4; color: #6a1010; padding: 0.5rem 0.8rem; border-radius: 4px; }
</style>
```

- [ ] **Step 3: Commit**

```bash
git add frontend/src/components/profiles/ProfileOverview.vue frontend/src/components/profiles/ProfileEditor.vue
git commit -m "feat(frontend): profile overview + editor (with clash-api toggle)"
```

---

## Task 11: ProfileActivate + ProfileDetail + wire into ProfilesView

**Files:**
- Create: `frontend/src/components/profiles/ProfileActivate.vue`
- Create: `frontend/src/components/profiles/ProfileDetail.vue`
- Modify: `frontend/src/views/ProfilesView.vue`

- [ ] **Step 1: Write `ProfileActivate.vue`**

```vue
<script setup lang="ts">
import { ref, watch } from "vue";
import {
  profileCheck, profilePrepareBundle, profileActivate,
} from "../../api/profile";
import { useBusy } from "../../composables/useBusy";
import { useToast } from "../../composables/useToast";
import { useHomeStatus } from "../../composables/useHomeStatus";
import type { ActivateResponse, CheckResponse, PrepareBundleResponse } from "../../api/types";

const props = defineProps<{ profileId: string }>();

const { busy, run } = useBusy();
const toast = useToast();
const { data: home, refresh: refreshHome } = useHomeStatus();

const corePath = ref("/var/lib/boxpilot/cores/current/sing-box");
const coreVersion = ref("unknown");
const verifyWindow = ref<number | null>(null);

watch(home, (v) => {
  if (v?.core?.path) corePath.value = v.core.path;
  if (v?.core?.version) coreVersion.value = v.core.version;
}, { immediate: true });

const check = ref<CheckResponse | null>(null);
const bundle = ref<PrepareBundleResponse | null>(null);
const result = ref<ActivateResponse | null>(null);
const showJson = ref(false);

async function runCheck() {
  await run(async () => {
    try {
      check.value = await profileCheck({ profile_id: props.profileId, core_path: corePath.value });
      if (check.value.success) toast.success("Check OK");
      else toast.error("Check FAILED");
    } catch (e: any) { toast.error(e?.message ?? String(e)); }
  });
}

async function prepare() {
  await run(async () => {
    try {
      bundle.value = await profilePrepareBundle({
        profile_id: props.profileId,
        core_path: corePath.value,
        core_version: coreVersion.value,
      });
      toast.success(`Bundle ready: ${bundle.value.staging_path}`);
    } catch (e: any) { toast.error(e?.message ?? String(e)); }
  });
}

async function activate() {
  if (!confirm("Activating will restart sing-box. Continue?")) return;
  await run(async () => {
    try {
      result.value = await profileActivate({
        profile_id: props.profileId,
        core_path: corePath.value,
        core_version: coreVersion.value,
        verify_window_secs: verifyWindow.value,
      });
      switch (result.value.outcome) {
        case "active": toast.success("Activated"); break;
        case "rolled_back": toast.error("Activation failed; rolled back"); break;
        case "rollback_target_missing": toast.error("Rollback target missing"); break;
        case "rollback_unstartable": toast.error("Rollback target also failed"); break;
      }
      await refreshHome();
    } catch (e: any) { toast.error(e?.message ?? String(e)); }
  });
}
</script>

<template>
  <div class="activate">
    <div class="row">
      <label>Core path <input v-model="corePath" /></label>
      <label>Core version <input v-model="coreVersion" /></label>
      <label>Verify window (s) <input type="number" min="1" max="30" v-model.number="verifyWindow" placeholder="5" /></label>
    </div>
    <div class="actions">
      <button :disabled="busy" @click="runCheck">Best-effort check</button>
      <button :disabled="busy" @click="prepare">Prepare bundle preview</button>
      <button :disabled="busy" class="primary" @click="activate">Activate</button>
    </div>

    <section v-if="check" class="block" :class="check.success ? 'ok' : 'err'">
      <h4>Check {{ check.success ? "OK" : "FAILED" }}</h4>
      <pre>{{ check.stderr || check.stdout || "(no output)" }}</pre>
    </section>

    <section v-if="bundle" class="block">
      <h4>
        Bundle preview
        <button class="link" @click="showJson = !showJson">{{ showJson ? "Hide" : "Show" }} manifest</button>
      </h4>
      <p>Staging: <code>{{ bundle.staging_path }}</code></p>
      <p>Activation id: <code>{{ bundle.manifest.activation_id }}</code></p>
      <p>Assets: {{ bundle.manifest.assets.length }}</p>
      <pre v-if="showJson">{{ JSON.stringify(bundle.manifest, null, 2) }}</pre>
    </section>

    <section v-if="result" class="block" :class="result.outcome === 'active' ? 'ok' : 'err'">
      <h4>{{ result.outcome.replaceAll("_", " ") }}</h4>
      <p>activation_id: <code>{{ result.activation_id }}</code></p>
      <p v-if="result.previous_activation_id">previous: <code>{{ result.previous_activation_id }}</code></p>
      <p>verify: pre={{ result.n_restarts_pre }}, post={{ result.n_restarts_post }}, window={{ result.window_used_ms }} ms</p>
    </section>
  </div>
</template>

<style scoped>
.activate { display: flex; flex-direction: column; gap: 0.75rem; }
.row { display: flex; gap: 1rem; flex-wrap: wrap; }
.row label { display: flex; flex-direction: column; font-size: 0.85rem; color: #444; }
.row input { padding: 0.3rem 0.4rem; min-width: 14rem; }
.actions { display: flex; gap: 0.5rem; flex-wrap: wrap; }
.actions button { padding: 0.4rem 0.9rem; border: 1px solid #bbb; border-radius: 4px; background: #f5f5f5; cursor: pointer; }
.actions button.primary { background: #1e7d3a; color: #fff; border-color: #1e7d3a; }
.block { padding: 0.6rem 0.9rem; border-radius: 4px; background: #f7f7f7; border: 1px solid #ddd; }
.block.ok { background: #e6f4ea; border-color: #b9d8c2; }
.block.err { background: #fde4e4; border-color: #f0a0a0; }
.block h4 { margin: 0 0 0.3rem; font-size: 0.95rem; display: flex; gap: 0.5rem; align-items: baseline; }
.block pre { background: #111; color: #eee; padding: 0.5rem; border-radius: 4px; max-height: 18rem; overflow: auto; font-size: 0.8rem; }
button.link { background: none; border: none; color: #1565c0; cursor: pointer; font-size: 0.8rem; padding: 0; }
code { background: #f0f0f0; padding: 0 0.25rem; border-radius: 2px; }
</style>
```

- [ ] **Step 2: Write `ProfileDetail.vue`**

```vue
<script setup lang="ts">
import { ref } from "vue";
import ProfileOverview from "./ProfileOverview.vue";
import ProfileEditor from "./ProfileEditor.vue";
import ProfileActivate from "./ProfileActivate.vue";

const props = defineProps<{ profileId: string }>();

type DetailTab = "overview" | "editor" | "activate";
const tab = ref<DetailTab>("overview");

function reloadEditor() {
  // Force ProfileOverview to re-fetch by toggling a key.
  bumpKey.value++;
}
const bumpKey = ref(0);
</script>

<template>
  <section class="detail">
    <nav class="tabs">
      <button :class="{ active: tab === 'overview' }" @click="tab = 'overview'">Overview</button>
      <button :class="{ active: tab === 'editor' }" @click="tab = 'editor'">Editor</button>
      <button :class="{ active: tab === 'activate' }" @click="tab = 'activate'">Activate</button>
    </nav>
    <ProfileOverview v-if="tab === 'overview'" :key="`ov-${props.profileId}-${bumpKey}`" :profile-id="props.profileId" />
    <ProfileEditor v-else-if="tab === 'editor'" :profile-id="props.profileId" @saved="reloadEditor" />
    <ProfileActivate v-else-if="tab === 'activate'" :profile-id="props.profileId" />
  </section>
</template>

<style scoped>
.detail { background: #fff; border: 1px solid #e5e5e5; border-radius: 6px; padding: 1rem; min-height: 24rem; display: flex; flex-direction: column; gap: 0.75rem; }
.tabs { display: flex; gap: 0.4rem; border-bottom: 1px solid #eee; padding-bottom: 0.25rem; }
.tabs button { padding: 0.3rem 0.7rem; border: 1px solid #ccc; border-radius: 4px; background: #f5f5f5; cursor: pointer; font-size: 0.85rem; }
.tabs button.active { background: #333; color: #fff; border-color: #333; }
</style>
```

- [ ] **Step 3: Wire `ProfileDetail` into `ProfilesView.vue`**

Replace `frontend/src/views/ProfilesView.vue`:

```vue
<script setup lang="ts">
import { ref } from "vue";
import ProfileList from "../components/profiles/ProfileList.vue";
import ProfileDetail from "../components/profiles/ProfileDetail.vue";

const selectedId = ref<string | null>(null);
function onSelect(id: string) { selectedId.value = id; }
</script>

<template>
  <div class="profiles">
    <ProfileList :selected-id="selectedId" @select="onSelect" />
    <section v-if="!selectedId" class="empty">
      <p>Select a profile from the list, or import a new one.</p>
    </section>
    <ProfileDetail v-else :key="selectedId" :profile-id="selectedId" />
  </div>
</template>

<style scoped>
.profiles { display: grid; grid-template-columns: 22rem 1fr; gap: 1rem; align-items: start; }
.empty { background: #fff; border: 1px dashed #ccc; padding: 2rem; border-radius: 6px; color: #888; min-height: 18rem; display: flex; align-items: center; justify-content: center; }
</style>
```

- [ ] **Step 4: Commit**

```bash
git add frontend/src/components/profiles frontend/src/views/ProfilesView.vue
git commit -m "feat(frontend): profile detail (overview/editor/activate tabs)"
```

---

## Task 12: SettingsView shell + CoresTab

**Files:**
- Create: `frontend/src/views/SettingsView.vue`
- Create: `frontend/src/components/settings/CoresTab.vue`

- [ ] **Step 1: Write `CoresTab.vue`**

```vue
<script setup lang="ts">
import { ref, watch } from "vue";
import {
  coreInstallManaged, coreUpgradeManaged, coreRollbackManaged, coreAdopt,
} from "../../api/helper";
import { useCores } from "../../composables/useCores";
import { useBusy } from "../../composables/useBusy";
import { useToast } from "../../composables/useToast";
import type { DiscoveredCore } from "../../api/types";

const { data, refresh } = useCores();
const { busy, run } = useBusy();
const toast = useToast();

refresh();

const versionInput = ref("latest");
const adoptPath = ref("");

function buildVersionRequest() {
  const v = versionInput.value.trim();
  if (!v || v === "latest") return { kind: "latest" as const };
  return { kind: "exact" as const, version: v };
}

async function install() {
  await run(async () => {
    try {
      await coreInstallManaged({ version: buildVersionRequest(), architecture: { kind: "auto" } });
      await refresh();
      toast.success("Installed");
    } catch (e: any) { toast.error(e?.message ?? String(e)); }
  });
}
async function upgrade() {
  await run(async () => {
    try {
      await coreUpgradeManaged({ version: buildVersionRequest(), architecture: { kind: "auto" } });
      await refresh();
      toast.success("Upgraded");
    } catch (e: any) { toast.error(e?.message ?? String(e)); }
  });
}
async function makeActive(c: DiscoveredCore) {
  if (c.kind === "external") return;
  await run(async () => {
    try {
      await coreRollbackManaged({ to_label: c.label });
      await refresh();
      toast.success(`Switched to ${c.label}`);
    } catch (e: any) { toast.error(e?.message ?? String(e)); }
  });
}
async function adopt() {
  if (!adoptPath.value.trim()) return;
  await run(async () => {
    try {
      await coreAdopt({ source_path: adoptPath.value.trim() });
      adoptPath.value = "";
      await refresh();
      toast.success("Adopted");
    } catch (e: any) { toast.error(e?.message ?? String(e)); }
  });
}
</script>

<template>
  <div class="cores">
    <h3>Cores</h3>
    <div class="row">
      <label>Version <input v-model="versionInput" placeholder="latest or 1.10.0" /></label>
      <button :disabled="busy" @click="install">Install</button>
      <button :disabled="busy" @click="upgrade">Upgrade</button>
      <button :disabled="busy" @click="refresh">Refresh</button>
    </div>
    <table v-if="data">
      <thead>
        <tr><th></th><th>Label</th><th>Kind</th><th>Version</th><th>SHA-8</th><th></th></tr>
      </thead>
      <tbody>
        <tr v-for="c in data.cores" :key="c.label + c.path">
          <td>{{ data.current === c.label ? "●" : "" }}</td>
          <td>{{ c.label }}</td>
          <td>{{ c.kind }}</td>
          <td>{{ c.version || "?" }}</td>
          <td><code>{{ c.sha256.slice(0, 8) }}</code></td>
          <td>
            <button v-if="c.kind !== 'external' && data.current !== c.label"
                    :disabled="busy" @click="makeActive(c)">Make active</button>
          </td>
        </tr>
      </tbody>
    </table>
    <div class="adopt">
      <label>Adopt from path: <input v-model="adoptPath" placeholder="/usr/local/bin/sing-box" /></label>
      <button :disabled="busy || !adoptPath.trim()" @click="adopt">Adopt</button>
    </div>
  </div>
</template>

<style scoped>
.cores { display: flex; flex-direction: column; gap: 0.75rem; }
.row { display: flex; gap: 0.5rem; align-items: center; flex-wrap: wrap; }
.row input { padding: 0.3rem 0.4rem; }
table { border-collapse: collapse; width: 100%; background: #fff; }
th, td { padding: 0.3rem 0.5rem; text-align: left; border-bottom: 1px solid #eee; font-size: 0.9rem; }
.adopt { display: flex; gap: 0.5rem; align-items: center; }
.adopt input { flex: 1; padding: 0.3rem 0.4rem; }
button { padding: 0.4rem 0.9rem; border: 1px solid #bbb; border-radius: 4px; background: #f5f5f5; cursor: pointer; }
code { background: #f0f0f0; padding: 0 0.25rem; border-radius: 2px; }
</style>
```

- [ ] **Step 2: Write `SettingsView.vue` (with sub-tabs)**

```vue
<script setup lang="ts">
import { ref, defineAsyncComponent } from "vue";
import CoresTab from "../components/settings/CoresTab.vue";

const ServiceTab = defineAsyncComponent(() => import("../components/settings/ServiceTab.vue"));
const LegacyTab = defineAsyncComponent(() => import("../components/settings/LegacyTab.vue"));
const AboutTab = defineAsyncComponent(() => import("../components/settings/AboutTab.vue"));

type SubTab = "cores" | "service" | "legacy" | "about";
const tab = ref<SubTab>("cores");
</script>

<template>
  <div class="settings">
    <nav class="subtabs">
      <button :class="{ active: tab === 'cores' }" @click="tab = 'cores'">Cores</button>
      <button :class="{ active: tab === 'service' }" @click="tab = 'service'">Service</button>
      <button :class="{ active: tab === 'legacy' }" @click="tab = 'legacy'">Legacy</button>
      <button :class="{ active: tab === 'about' }" @click="tab = 'about'">About</button>
    </nav>
    <section class="panel">
      <CoresTab v-if="tab === 'cores'" />
      <ServiceTab v-else-if="tab === 'service'" />
      <LegacyTab v-else-if="tab === 'legacy'" />
      <AboutTab v-else-if="tab === 'about'" />
    </section>
  </div>
</template>

<style scoped>
.settings { display: flex; flex-direction: column; gap: 1rem; }
.subtabs { display: flex; gap: 0.5rem; border-bottom: 1px solid #ddd; padding-bottom: 0.5rem; }
.subtabs button { padding: 0.3rem 0.7rem; border: 1px solid #ccc; border-radius: 4px; background: #f5f5f5; cursor: pointer; }
.subtabs button.active { background: #333; color: #fff; border-color: #333; }
.panel { background: #fff; border: 1px solid #e5e5e5; border-radius: 6px; padding: 1rem; min-height: 24rem; }
</style>
```

`defineAsyncComponent` is used because the Service/Legacy/About files don't exist yet — without async, vue-tsc would fail at build time. Once those files exist (next tasks), the imports will resolve normally at runtime.

Stub the missing files so the dev tree resolves now:

Write `frontend/src/components/settings/ServiceTab.vue`:
```vue
<template><p>Service tab — coming up.</p></template>
```
Write `frontend/src/components/settings/LegacyTab.vue`:
```vue
<template><p>Legacy tab — coming up.</p></template>
```
Write `frontend/src/components/settings/AboutTab.vue`:
```vue
<template><p>About tab — coming up.</p></template>
```

- [ ] **Step 3: Build the frontend (full tree)**

Run: `cd frontend && npm run build`
Expected: success.

- [ ] **Step 4: Commit**

```bash
git add frontend/src/views/SettingsView.vue frontend/src/components/settings
git commit -m "feat(frontend): settings shell + cores tab (+ stub service/legacy/about)"
```

---

## Task 13: ServiceTab — install + control + manual rollback

**Files:**
- Modify: `frontend/src/components/settings/ServiceTab.vue`

- [ ] **Step 1: Replace `ServiceTab.vue`**

```vue
<script setup lang="ts">
import { ref } from "vue";
import {
  serviceInstallManaged, serviceEnable, serviceDisable,
  serviceStart, serviceStop, serviceRestart,
} from "../../api/helper";
import { profileRollback } from "../../api/profile";
import { useHomeStatus } from "../../composables/useHomeStatus";
import { useBusy } from "../../composables/useBusy";
import { useToast } from "../../composables/useToast";

const { data: home, refresh } = useHomeStatus();
const { busy, run } = useBusy();
const toast = useToast();

async function call(name: string, fn: () => Promise<unknown>) {
  await run(async () => {
    try {
      await fn();
      toast.success(`${name} ok`);
      await refresh();
    } catch (e: any) { toast.error(`${name}: ${e?.message ?? String(e)}`); }
  });
}

const rollbackId = ref("");
async function rollback() {
  const target = rollbackId.value.trim();
  if (!target) return;
  if (!confirm(`Roll back to ${target}? This restarts sing-box.`)) return;
  await run(async () => {
    try {
      const r = await profileRollback({ target_activation_id: target });
      switch (r.outcome) {
        case "active": toast.success(`Rolled back to ${r.activation_id}`); break;
        case "rolled_back": toast.error("Rollback failed; previous restored"); break;
        case "rollback_target_missing": toast.error("Target not found on disk"); break;
        case "rollback_unstartable": toast.error("Target also failed; service stopped"); break;
      }
      await refresh();
    } catch (e: any) { toast.error(e?.message ?? String(e)); }
  });
}
</script>

<template>
  <div class="service">
    <h3>Service unit</h3>
    <p class="meta">Unit: <code>{{ home?.service?.unit_name ?? "?" }}</code></p>
    <p class="meta">State:
      <code v-if="home?.service?.unit_state.kind === 'known'">
        {{ home.service.unit_state.active_state }} ({{ home.service.unit_state.sub_state }})
      </code>
      <code v-else>not installed</code>
    </p>
    <div class="actions">
      <button :disabled="busy" @click="call('install unit', serviceInstallManaged)">Install / re-install unit</button>
      <button :disabled="busy" @click="call('enable', serviceEnable)">Enable</button>
      <button :disabled="busy" @click="call('disable', serviceDisable)">Disable</button>
      <button :disabled="busy" @click="call('start', serviceStart)">Start</button>
      <button :disabled="busy" @click="call('stop', serviceStop)">Stop</button>
      <button :disabled="busy" @click="call('restart', serviceRestart)">Restart</button>
    </div>

    <h3>Manual rollback</h3>
    <p class="meta">
      Paste an activation_id (format <code>YYYY-MM-DDTHH-MM-SSZ-xxxxxx</code>).
      You can copy it from the Home page's previous activation, or from logs.
    </p>
    <div class="row">
      <input v-model="rollbackId" placeholder="2026-04-30T00-00-00Z-abc123" />
      <button :disabled="busy || !rollbackId.trim()" @click="rollback">Roll back</button>
    </div>
  </div>
</template>

<style scoped>
.service { display: flex; flex-direction: column; gap: 0.75rem; }
.service h3 { margin: 0; font-size: 1rem; }
.meta { color: #666; font-size: 0.85rem; margin: 0; }
.actions { display: flex; flex-wrap: wrap; gap: 0.5rem; }
.actions button { padding: 0.4rem 0.9rem; border: 1px solid #bbb; border-radius: 4px; background: #f5f5f5; cursor: pointer; }
.row { display: flex; gap: 0.5rem; }
.row input { flex: 1; padding: 0.3rem 0.4rem; font-family: ui-monospace, monospace; }
.row button { padding: 0.4rem 0.9rem; border: 1px solid #b22; border-radius: 4px; background: #fde4e4; color: #6a1010; cursor: pointer; }
code { background: #f0f0f0; padding: 0 0.25rem; border-radius: 2px; }
</style>
```

- [ ] **Step 2: Build**

Run: `cd frontend && npm run build`
Expected: success.

- [ ] **Step 3: Commit**

```bash
git add frontend/src/components/settings/ServiceTab.vue
git commit -m "feat(frontend): service tab (install + control + manual rollback)"
```

---

## Task 14: LegacyTab — observe + migrate flow

**Files:**
- Modify: `frontend/src/components/settings/LegacyTab.vue`

- [ ] **Step 1: Replace `LegacyTab.vue`**

The migrate flow does Prepare → write the bytes to a temp path under the user's profile-store dir → import → optional Cutover. We don't have direct fs access from the renderer, so we route through Tauri's filesystem; for v1 we keep this simpler by adding a small in-place flow: Prepare returns bytes → we render a "writeable" import card asking the user to confirm a target path (default: home dir) → we use the Tauri write APIs.

To avoid pulling in `@tauri-apps/plugin-fs`, we keep it simpler: Prepare's bytes are presented to the user; the user clicks "Save & Import as profile" which calls Prepare again, writes via a small new Tauri command. To stay on the published API surface, we instead **import directly via `profileImportFile` against a path the user picks**, leveraging the fact that the legacy daemon already validated the source path under §8 path-safety. The user copies the path string from the observe response and clicks "Import as profile" — we call `profileImportFile(name, path)`.

```vue
<script setup lang="ts">
import { ref } from "vue";
import {
  legacyObserveService, legacyMigratePrepare, legacyMigrateCutover,
} from "../../api/helper";
import { profileImportFile, profileImportDir } from "../../api/profile";
import { useBusy } from "../../composables/useBusy";
import { useToast } from "../../composables/useToast";
import type { LegacyObserveServiceResponse } from "../../api/types";

const observed = ref<LegacyObserveServiceResponse | null>(null);
const profileName = ref("imported-legacy");
const importedProfileId = ref<string | null>(null);
const cutoverDone = ref(false);
const { busy, run } = useBusy();
const toast = useToast();

async function scan() {
  await run(async () => {
    try {
      observed.value = await legacyObserveService();
      cutoverDone.value = false;
      importedProfileId.value = null;
    } catch (e: any) { toast.error(e?.message ?? String(e)); }
  });
}

async function importFromPath() {
  const o = observed.value;
  if (!o?.config_path) return;
  await run(async () => {
    try {
      // Prepare also returns the bytes; we don't strictly need them here
      // (we read straight from the system path), but calling Prepare
      // exercises the helper's path-safety classifier as a defence in depth.
      await legacyMigratePrepare();
      const summary = await profileImportFile(profileName.value, o.config_path!);
      importedProfileId.value = summary.id;
      toast.success(`Imported as ${summary.name}`);
    } catch (e: any) { toast.error(e?.message ?? String(e)); }
  });
}

async function importDirFromParent() {
  const o = observed.value;
  if (!o?.config_path) return;
  const dir = o.config_path.replace(/\/[^/]+$/, "") || "/";
  await run(async () => {
    try {
      await legacyMigratePrepare();
      const summary = await profileImportDir(profileName.value, dir);
      importedProfileId.value = summary.id;
      toast.success(`Imported (with assets) as ${summary.name}`);
    } catch (e: any) { toast.error(e?.message ?? String(e)); }
  });
}

async function cutover() {
  if (!confirm("Stop and disable the legacy unit? Make sure you have an active BoxPilot profile first.")) return;
  await run(async () => {
    try {
      await legacyMigrateCutover();
      cutoverDone.value = true;
      toast.success("Legacy unit stopped + disabled");
    } catch (e: any) { toast.error(e?.message ?? String(e)); }
  });
}
</script>

<template>
  <div class="legacy">
    <h3>Legacy sing-box.service</h3>
    <p class="meta">Detects an existing system-installed sing-box service and migrates its config + assets into a BoxPilot profile.</p>

    <button :disabled="busy" @click="scan">Scan</button>

    <section v-if="observed && !observed.detected" class="block ok">
      <p>No legacy <code>sing-box.service</code> found on this system.</p>
    </section>

    <section v-else-if="observed" class="block">
      <h4>Detected</h4>
      <p>Unit: <code>{{ observed.unit_name }}</code> (<code>{{ observed.unit_file_state ?? "?" }}</code>)</p>
      <p>Fragment: <code>{{ observed.fragment_path ?? "?" }}</code></p>
      <p>ExecStart: <code>{{ observed.exec_start_raw ?? "?" }}</code></p>
      <p>Config: <code>{{ observed.config_path ?? "(unparseable)" }}</code> — <strong>{{ observed.config_path_kind }}</strong></p>

      <div v-if="observed.config_path_kind === 'system_path'" class="actions">
        <label>Profile name <input v-model="profileName" /></label>
        <button :disabled="busy" @click="importFromPath">Import as profile (file)</button>
        <button :disabled="busy" @click="importDirFromParent">Import as profile (dir + assets)</button>
      </div>
      <p v-else-if="observed.config_path_kind === 'user_or_ephemeral'" class="meta warn">
        Refusing to migrate: config path is under a user / ephemeral location.
        Move it to <code>/etc</code> or import it manually via the Profiles tab.
      </p>
      <p v-else class="meta warn">
        Could not parse <code>-c</code> from ExecStart. Import the config manually via the Profiles tab.
      </p>

      <div v-if="importedProfileId" class="next">
        <p>Imported. Activate the new profile under <strong>Profiles</strong> first, then run cutover here.</p>
        <button :disabled="busy || cutoverDone" @click="cutover">
          {{ cutoverDone ? "Cutover done" : "Stop + disable legacy unit" }}
        </button>
      </div>
      <div v-else class="next">
        <p class="meta">If you've already imported the config separately, you can still cut over:</p>
        <button :disabled="busy || cutoverDone" @click="cutover">
          {{ cutoverDone ? "Cutover done" : "Stop + disable legacy unit only" }}
        </button>
      </div>
    </section>
  </div>
</template>

<style scoped>
.legacy { display: flex; flex-direction: column; gap: 0.75rem; }
.legacy h3 { margin: 0; font-size: 1rem; }
.meta { color: #666; font-size: 0.85rem; margin: 0; }
.meta.warn { background: #fff3cd; color: #5a4400; padding: 0.4rem 0.6rem; border-radius: 4px; }
.block { padding: 0.6rem 0.9rem; background: #f7f7f7; border: 1px solid #ddd; border-radius: 4px; display: flex; flex-direction: column; gap: 0.3rem; }
.block.ok { background: #e6f4ea; border-color: #b9d8c2; }
.actions { display: flex; flex-wrap: wrap; gap: 0.5rem; align-items: center; margin-top: 0.4rem; }
.actions input { padding: 0.3rem 0.4rem; }
.next { display: flex; flex-direction: column; gap: 0.4rem; margin-top: 0.4rem; }
button { padding: 0.4rem 0.9rem; border: 1px solid #bbb; border-radius: 4px; background: #f5f5f5; cursor: pointer; }
code { background: #f0f0f0; padding: 0 0.25rem; border-radius: 2px; }
</style>
```

- [ ] **Step 2: Build**

Run: `cd frontend && npm run build`
Expected: success.

- [ ] **Step 3: Commit**

```bash
git add frontend/src/components/settings/LegacyTab.vue
git commit -m "feat(frontend): legacy tab (observe + import + cutover)"
```

---

## Task 15: AboutTab + drop old components

**Files:**
- Modify: `frontend/src/components/settings/AboutTab.vue`
- Delete: `frontend/src/components/CoresPanel.vue`
- Delete: `frontend/src/components/ProfilesPanel.vue`
- Delete: `frontend/src/components/ServicePanel.vue`

- [ ] **Step 1: Replace `AboutTab.vue`**

```vue
<script setup lang="ts">
import { useHomeStatus } from "../../composables/useHomeStatus";

const { data } = useHomeStatus();
</script>

<template>
  <div class="about">
    <h3>About BoxPilot</h3>
    <p>Linux desktop control panel for system-level <code>sing-box</code>.</p>
    <h4>Storage</h4>
    <ul>
      <li>Helper config: <code>/etc/boxpilot/boxpilot.toml</code></li>
      <li>Active release: <code>/etc/boxpilot/active</code> → <code>/etc/boxpilot/releases/&lt;activation_id&gt;</code></li>
      <li>Managed cores: <code>/var/lib/boxpilot/cores/&lt;version&gt;/sing-box</code></li>
      <li>Install ledger: <code>/var/lib/boxpilot/install-state.json</code></li>
      <li>Unit backups (legacy migrate): <code>/var/lib/boxpilot/backups/units/</code></li>
    </ul>
    <h4>Current state</h4>
    <ul v-if="data">
      <li>Service unit: <code>{{ data.service.unit_name }}</code></li>
      <li>Core path: <code>{{ data.core.path ?? "(unset)" }}</code></li>
      <li>Core state: <code>{{ data.core.state ?? "(unset)" }}</code></li>
      <li>Active release: <code>{{ data.active_profile?.release_id ?? "(none)" }}</code></li>
      <li>Schema version: <code>{{ data.schema_version }}</code></li>
    </ul>
    <p v-else class="muted">Loading…</p>
  </div>
</template>

<style scoped>
.about { display: flex; flex-direction: column; gap: 0.5rem; }
.about h3 { margin: 0; font-size: 1rem; }
.about h4 { margin: 0.5rem 0 0.2rem; font-size: 0.9rem; }
.about ul { margin: 0; padding-left: 1.2rem; font-size: 0.9rem; }
.muted { color: #888; }
code { background: #f0f0f0; padding: 0 0.25rem; border-radius: 2px; }
</style>
```

- [ ] **Step 2: Delete old components**

```bash
rm frontend/src/components/CoresPanel.vue frontend/src/components/ProfilesPanel.vue frontend/src/components/ServicePanel.vue
```

- [ ] **Step 3: Build**

Run: `cd frontend && npm run build`
Expected: success.

- [ ] **Step 4: Commit**

```bash
git add frontend/src/components/settings/AboutTab.vue
git rm frontend/src/components/CoresPanel.vue frontend/src/components/ProfilesPanel.vue frontend/src/components/ServicePanel.vue
git commit -m "feat(frontend): about tab + drop legacy panels"
```

---

## Task 16: Smoke procedure document

**Files:**
- Create: `docs/superpowers/plans/2026-04-30-gui-shell-smoke-procedure.md`

- [ ] **Step 1: Write the smoke procedure**

```markdown
# Plan #7 — GUI Shell Smoke Procedure

**Goal:** Validate every flow plan #7 ships against a real system that has
plans #1–#6 deployed.

**Prereqs**

1. `boxpilotd` installed via `make install-helper`.
2. `cargo tauri dev` run from `crates/boxpilot-tauri` (use `make run-gui` from
   the workspace root).
3. A system user with `sudo` available (polkit will prompt for admin auth on
   mutating actions).

## 1. Home page render

- Open BoxPilot. The "Home" tab loads automatically.
- Expect three cards: Service / Active profile / Core. The poll runs every
  5 s; values update without manual refresh.
- Click "Refresh" in Quick actions. Toast says "ok".
- Hide the window for >5 s (tab the desktop). When you bring it back, the
  Home page should refresh immediately on visibility change.

## 2. Service controls

- In Quick actions click Stop → Start → Restart. Each shows a toast and
  the service card updates.
- Open Logs panel → "Load tail (200)". Expect 200 lines or fewer.

## 3. Profile import + activate

- Profiles tab → enter a name + path to a known-good config.json → "File"
  button. Expect import success.
- Select the new profile. Overview tab shows inbounds/outbounds counts.
- Editor tab shows the source. Click "Save" — disabled because clean.
- Activate tab → "Best-effort check" → green OK. → "Prepare bundle preview"
  → manifest renders. → "Activate" → confirm modal. Outcome:
  - Happy path: green "active" panel, Home card updates within 5 s.
  - Bad config: yellow "rolled_back" panel.

## 4. Manual rollback

- Settings → Service → Manual rollback.
- Paste a previous activation_id (visible in the activate panel as
  "previous"). Click "Roll back". Expect outcome panel.

## 5. Clash API toggle

- Profiles → select profile → Editor tab.
- Click "Enable Clash API on loopback". Editor reloads with
  `experimental.clash_api.external_controller = "127.0.0.1:9090"`.
- Button is now disabled because the field is set.
- Re-activate to apply.

## 6. Legacy detection + migrate

- (Only on a system with a legacy `sing-box.service`.)
- Settings → Legacy → Scan. Card shows fragment_path / config_path / kind.
- For `system_path`: enter a profile name → "Import as profile (file)" or
  "(dir + assets)". Profile appears in Profiles tab.
- Activate the imported profile.
- Back to Settings → Legacy → "Stop + disable legacy unit". Confirm. Toast
  says ok; cutover button disables.

## 7. Drift / repair banners

- Force `controller_orphaned` (delete the user that owns the controller, or
  point `controller_uid` at a missing UID via root edit of
  `/etc/boxpilot/boxpilot.toml`). Reload Home — yellow banner.
- Force `active_corrupt` (as root, point `/etc/boxpilot/active` at a
  non-release path). Reload Home — red banner with "Open Profiles" action.

## 8. Build + lint

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
( cd frontend && npm ci && npm run build )
xmllint --noout packaging/linux/dbus/system.d/app.boxpilot.helper.conf
xmllint --noout packaging/linux/polkit-1/actions/app.boxpilot.helper.policy
```

All must pass.
```

- [ ] **Step 2: Commit**

```bash
git add docs/superpowers/plans/2026-04-30-gui-shell-smoke-procedure.md
git commit -m "docs(plan-7): smoke procedure"
```

---

## Task 17: Final lint, format, test pass

- [ ] **Step 1: Run the full project gate**

Run: `bash scripts/check.sh`

If anything fails, fix it inline and re-run until clean.

- [ ] **Step 2: If any formatting/lint changes were applied, commit them**

```bash
git add -A
git commit -m "chore(plan-7): cargo fmt + clippy fixes"
```

(Skip this commit if `git status` is clean.)

---

## Self-review checklist (run after the plan is done)

1. **Spec coverage:** every section in `2026-04-30-gui-shell-design.md`
   maps to a task. The new `home.status` IPC method is implemented and
   wired (Tasks 1–4); Vue rewrite covers Home (Task 8), Profiles
   (Tasks 9–11), Settings (Tasks 12–15); smoke procedure documented
   (Task 16); CI gate clean (Task 17).
2. **Placeholder scan:** No `TBD`, `TODO`, "implement later", "similar to
   X" without code, or vague "add error handling" steps. All steps
   either reference verbatim code or give exact commands.
3. **Type consistency:** `HomeStatusResponse`, `ActiveProfileSnapshot`,
   `CoreSnapshot` types match between Rust (Task 1), TypeScript
   (Task 5), and Vue components (Tasks 8–15). The `ActivateOutcome`
   string union mirrors `boxpilot_ipc::ActivateOutcome`'s `snake_case`
   serialization. Tauri command names match between Rust (`commands.rs`,
   `lib.rs`) and frontend (`invoke("helper_home_status")`).
