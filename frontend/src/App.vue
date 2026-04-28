<script setup lang="ts">
import { ref } from "vue";
import { serviceStatus, isCommandError } from "./api/helper";
import type { ServiceStatusResponse } from "./api/types";

const loading = ref(false);
const status = ref<ServiceStatusResponse | null>(null);
const error = ref<{ code: string; message: string } | null>(null);

async function check() {
  loading.value = true;
  error.value = null;
  try {
    status.value = await serviceStatus();
  } catch (e) {
    if (isCommandError(e)) {
      error.value = e;
    } else {
      error.value = { code: "unknown", message: String(e) };
    }
    status.value = null;
  } finally {
    loading.value = false;
  }
}
</script>

<template>
  <main>
    <h1>BoxPilot</h1>
    <p>Plan #1 — helper round-trip smoke test.</p>
    <button :disabled="loading" @click="check">
      {{ loading ? "Checking..." : "Check service.status" }}
    </button>

    <section v-if="error" class="err">
      <h2>Error</h2>
      <code>{{ error.code }}</code>
      <p>{{ error.message }}</p>
    </section>

    <section v-if="status" class="ok">
      <h2>Service: {{ status.unit_name }}</h2>
      <pre>{{ JSON.stringify(status, null, 2) }}</pre>
    </section>
  </main>
</template>

<style>
body { font-family: system-ui, sans-serif; padding: 2rem; max-width: 60rem; margin: auto; }
button { padding: 0.5rem 1rem; font-size: 1rem; }
section.err { margin-top: 1.5rem; padding: 1rem; background: #fee; border-radius: 0.5rem; }
section.ok { margin-top: 1.5rem; padding: 1rem; background: #efe; border-radius: 0.5rem; }
pre { white-space: pre-wrap; }
</style>
