<script setup lang="ts">
import { ref } from "vue";
import CoresPanel from "./components/CoresPanel.vue";
import ServicePanel from "./components/ServicePanel.vue";

type Tab = "home" | "cores";
const tab = ref<Tab>("home");
</script>

<template>
  <main>
    <h1>BoxPilot</h1>
    <nav>
      <button :class="{ active: tab === 'home' }" @click="tab = 'home'">Home</button>
      <button :class="{ active: tab === 'cores' }" @click="tab = 'cores'">Settings → Cores</button>
    </nav>
    <ServicePanel v-if="tab === 'home'" />
    <CoresPanel v-else-if="tab === 'cores'" />
  </main>
</template>

<style>
body { font-family: system-ui, sans-serif; padding: 2rem; max-width: 60rem; margin: auto; }
nav { display: flex; gap: 0.5rem; margin: 1rem 0; }
nav button { padding: 0.5rem 1rem; }
nav button.active { background: #333; color: #fff; }
</style>
