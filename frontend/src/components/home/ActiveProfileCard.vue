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
