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
code { background: #f0f0f0; padding: 0 0.25rem; border-radius: 2px; }
</style>
