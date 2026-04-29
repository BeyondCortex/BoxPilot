<script setup lang="ts">
import { ref, onMounted } from "vue";
import { coreAdopt, coreDiscover, coreInstallManaged, coreRollbackManaged } from "../api/helper";
import type { CoreDiscoverResponse, DiscoveredCore } from "../api/types";

const data = ref<CoreDiscoverResponse | null>(null);
const status = ref<string>("idle");
const error = ref<string | null>(null);
const adoptPath = ref("");

async function refresh() {
  status.value = "loading…";
  error.value = null;
  try {
    data.value = await coreDiscover();
    status.value = "idle";
  } catch (e: any) {
    error.value = e?.message ?? String(e);
    status.value = "error";
  }
}

async function installLatest() {
  status.value = "installing latest…";
  error.value = null;
  try {
    await coreInstallManaged({
      version: { kind: "latest" },
      architecture: { kind: "auto" },
    });
    await refresh();
  } catch (e: any) {
    error.value = e?.message ?? String(e);
    status.value = "error";
  }
}

async function makeActive(c: DiscoveredCore) {
  if (c.kind === "external") return;
  status.value = `switching to ${c.label}…`;
  try {
    await coreRollbackManaged({ to_label: c.label });
    await refresh();
  } catch (e: any) {
    error.value = e?.message ?? String(e);
    status.value = "error";
  }
}

async function adopt() {
  if (!adoptPath.value.trim()) return;
  status.value = `adopting ${adoptPath.value}…`;
  try {
    await coreAdopt({ source_path: adoptPath.value.trim() });
    adoptPath.value = "";
    await refresh();
  } catch (e: any) {
    error.value = e?.message ?? String(e);
    status.value = "error";
  }
}

onMounted(refresh);
</script>

<template>
  <section class="cores-panel">
    <h2>Cores</h2>
    <div class="actions">
      <button @click="installLatest" :disabled="status !== 'idle'">Install latest sing-box</button>
      <button @click="refresh" :disabled="status !== 'idle'">Refresh</button>
    </div>
    <p v-if="error" class="err">{{ error }}</p>
    <p v-else class="status">Status: {{ status }}</p>
    <table v-if="data">
      <thead><tr><th></th><th>Label</th><th>Kind</th><th>Version</th><th>SHA</th><th></th></tr></thead>
      <tbody>
        <tr v-for="c in data.cores" :key="c.label + c.path">
          <td>{{ data.current === c.label ? "●" : "" }}</td>
          <td>{{ c.label }}</td>
          <td>{{ c.kind }}</td>
          <td>{{ c.version }}</td>
          <td><code>{{ c.sha256.slice(0, 12) }}…</code></td>
          <td>
            <button v-if="c.kind !== 'external' && data.current !== c.label"
                    @click="makeActive(c)" :disabled="status !== 'idle'">Make active</button>
          </td>
        </tr>
      </tbody>
    </table>
    <div class="adopt">
      <label>Adopt from path:
        <input v-model="adoptPath" placeholder="/usr/local/bin/sing-box"/>
      </label>
      <button @click="adopt" :disabled="status !== 'idle' || !adoptPath.trim()">Adopt</button>
    </div>
  </section>
</template>

<style scoped>
.cores-panel { padding: 1rem; }
.actions { display: flex; gap: 0.5rem; margin-bottom: 1rem; }
table { width: 100%; border-collapse: collapse; margin: 1rem 0; }
th, td { padding: 0.25rem 0.5rem; text-align: left; border-bottom: 1px solid #eee; }
.err { color: #c00; }
.status { color: #666; }
.adopt { display: flex; gap: 0.5rem; align-items: center; }
.adopt input { flex: 1; padding: 0.25rem; }
</style>
