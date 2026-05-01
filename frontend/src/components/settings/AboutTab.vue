<script setup lang="ts">
import { ref } from "vue";
import { useHomeStatus } from "../../composables/useHomeStatus";
import { diagnosticsExport, isCommandError } from "../../api/helper";

const { data } = useHomeStatus();

const exporting = ref(false);
const exportResult = ref<string | null>(null);
const exportError = ref<string | null>(null);

async function onExport() {
  exporting.value = true;
  exportResult.value = null;
  exportError.value = null;
  try {
    const r = await diagnosticsExport();
    const sizeMb = (r.bundle_size_bytes / (1024 * 1024)).toFixed(2);
    exportResult.value = `${r.bundle_path} (${sizeMb} MiB)`;
  } catch (e) {
    if (isCommandError(e)) {
      exportError.value = `${e.code}: ${e.message}`;
    } else {
      exportError.value = String(e);
    }
  } finally {
    exporting.value = false;
  }
}
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
      <li>Diagnostics bundles: <code>/var/cache/boxpilot/diagnostics/</code></li>
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

    <h4>Diagnostics</h4>
    <p class="muted">
      Exports a redacted <code>tar.gz</code> bundle. Server addresses, passwords,
      UUIDs, private keys, and clash-api secrets are stripped per spec §14.
    </p>
    <button :disabled="exporting" @click="onExport">
      {{ exporting ? "Exporting…" : "Export diagnostics" }}
    </button>
    <p v-if="exportResult" class="ok">Exported: <code>{{ exportResult }}</code></p>
    <p v-if="exportError" class="err">Failed: {{ exportError }}</p>
  </div>
</template>

<style scoped>
.about { display: flex; flex-direction: column; gap: 0.5rem; }
.about h3 { margin: 0; font-size: 1rem; }
.about h4 { margin: 0.5rem 0 0.2rem; font-size: 0.9rem; }
.about ul { margin: 0; padding-left: 1.2rem; font-size: 0.9rem; }
.muted { color: #888; }
.ok { color: #2a7a2a; }
.err { color: #c33; }
button { align-self: flex-start; }
code { background: #f0f0f0; padding: 0 0.25rem; border-radius: 2px; }
</style>
