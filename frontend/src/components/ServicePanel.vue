<script setup lang="ts">
import { onMounted, ref } from "vue";
import {
  serviceStatus, serviceStart, serviceStop, serviceRestart,
  serviceEnable, serviceDisable, serviceInstallManaged, serviceLogs,
} from "../api/helper";
import type { ServiceStatusResponse, ServiceLogsResponse } from "../api/types";

const status = ref<ServiceStatusResponse | null>(null);
const logs = ref<ServiceLogsResponse | null>(null);
const busy = ref(false);
const error = ref<string | null>(null);

async function refresh() {
  busy.value = true; error.value = null;
  try { status.value = await serviceStatus(); }
  catch (e: any) { error.value = e?.message ?? String(e); }
  finally { busy.value = false; }
}

async function run<T>(fn: () => Promise<T>) {
  busy.value = true; error.value = null;
  try { await fn(); await refresh(); }
  catch (e: any) { error.value = e?.message ?? String(e); }
  finally { busy.value = false; }
}

async function loadLogs() {
  busy.value = true; error.value = null;
  try { logs.value = await serviceLogs({ lines: 200 }); }
  catch (e: any) { error.value = e?.message ?? String(e); }
  finally { busy.value = false; }
}

onMounted(refresh);
</script>

<template>
  <section class="service-panel">
    <h2>Service</h2>
    <p v-if="error" class="err">{{ error }}</p>
    <pre v-if="status">{{ JSON.stringify(status, null, 2) }}</pre>
    <div class="actions">
      <button :disabled="busy" @click="refresh">Refresh</button>
      <button :disabled="busy" @click="run(serviceInstallManaged)">Install unit</button>
      <button :disabled="busy" @click="run(serviceEnable)">Enable</button>
      <button :disabled="busy" @click="run(serviceDisable)">Disable</button>
      <button :disabled="busy" @click="run(serviceStart)">Start</button>
      <button :disabled="busy" @click="run(serviceStop)">Stop</button>
      <button :disabled="busy" @click="run(serviceRestart)">Restart</button>
    </div>
    <h3>Logs</h3>
    <button :disabled="busy" @click="loadLogs">Tail last 200 lines</button>
    <pre v-if="logs" class="logs">{{ logs.lines.join("\n") }}</pre>
  </section>
</template>

<style scoped>
.service-panel { padding: 1rem; }
.actions { display: flex; flex-wrap: wrap; gap: 0.5rem; margin: 1rem 0; }
.actions button { padding: 0.5rem 1rem; }
.err { color: #c00; }
.logs { max-height: 24rem; overflow: auto; background: #111; color: #eee; padding: 0.5rem; font-size: 0.85em; }
</style>
