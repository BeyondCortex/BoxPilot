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
