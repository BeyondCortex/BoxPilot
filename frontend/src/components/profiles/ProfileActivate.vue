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
