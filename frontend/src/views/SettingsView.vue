<script setup lang="ts">
import { ref } from "vue";
import CoresTab from "../components/settings/CoresTab.vue";
import ServiceTab from "../components/settings/ServiceTab.vue";
import LegacyTab from "../components/settings/LegacyTab.vue";
import AboutTab from "../components/settings/AboutTab.vue";

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
