<script setup lang="ts">
import { ref } from "vue";
import CoresPanel from "./components/CoresPanel.vue";
import { serviceStatus } from "./api/helper";
import type { ServiceStatusResponse } from "./api/types";

type Tab = "home" | "cores";
const tab = ref<Tab>("home");
const status = ref<ServiceStatusResponse | null>(null);
const error = ref<string | null>(null);
const loading = ref(false);

async function check() {
  loading.value = true;
  error.value = null;
  try {
    status.value = await serviceStatus();
  } catch (e: any) {
    error.value = e?.message ?? String(e);
    status.value = null;
  } finally {
    loading.value = false;
  }
}
</script>

<template>
  <main>
    <h1>BoxPilot</h1>
    <nav>
      <button :class="{ active: tab === 'home' }" @click="tab = 'home'">Home</button>
      <button :class="{ active: tab === 'cores' }" @click="tab = 'cores'">Settings → Cores</button>
    </nav>
    <section v-if="tab === 'home'">
      <button :disabled="loading" @click="check">
        {{ loading ? "Checking..." : "Check service.status" }}
      </button>
      <pre v-if="status">{{ JSON.stringify(status, null, 2) }}</pre>
      <p v-if="error" class="err">{{ error }}</p>
    </section>
    <CoresPanel v-else-if="tab === 'cores'" />
  </main>
</template>

<style>
body { font-family: system-ui, sans-serif; padding: 2rem; max-width: 60rem; margin: auto; }
nav { display: flex; gap: 0.5rem; margin: 1rem 0; }
nav button { padding: 0.5rem 1rem; }
nav button.active { background: #333; color: #fff; }
.err { color: #c00; }
</style>
