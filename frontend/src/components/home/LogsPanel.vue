<script setup lang="ts">
import { ref } from "vue";
import { serviceLogs } from "../../api/helper";
import { useBusy } from "../../composables/useBusy";
import { useToast } from "../../composables/useToast";

const lines = ref<string[]>([]);
const truncated = ref(false);
const open = ref(false);
const { busy, run } = useBusy();
const toast = useToast();

async function load() {
  await run(async () => {
    try {
      const r = await serviceLogs({ lines: 200 });
      lines.value = r.lines;
      truncated.value = r.truncated;
      open.value = true;
    } catch (e: any) {
      toast.error(`logs: ${e?.message ?? String(e)}`);
    }
  });
}
</script>

<template>
  <section class="card">
    <h3>Logs</h3>
    <button :disabled="busy" @click="load">
      {{ open ? "Refresh tail (200)" : "Load tail (200)" }}
    </button>
    <pre v-if="open" class="logs">{{ lines.join("\n") || "(no lines)" }}</pre>
    <p v-if="truncated" class="meta">(server clamped to 200)</p>
  </section>
</template>

<style scoped>
.card { background: #fff; border: 1px solid #e5e5e5; border-radius: 6px; padding: 1rem; }
.card h3 { margin: 0 0 0.5rem 0; font-size: 1rem; }
.logs { max-height: 24rem; overflow: auto; background: #111; color: #eee; padding: 0.5rem; font-size: 0.8rem; border-radius: 4px; }
.meta { color: #666; font-size: 0.85rem; }
button { padding: 0.4rem 0.9rem; border: 1px solid #bbb; border-radius: 4px; background: #f5f5f5; cursor: pointer; }
</style>
