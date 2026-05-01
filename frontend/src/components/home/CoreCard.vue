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
