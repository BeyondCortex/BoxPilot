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
