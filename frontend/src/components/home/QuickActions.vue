<script setup lang="ts">
import { serviceStart, serviceStop, serviceRestart } from "../../api/helper";
import { useBusy } from "../../composables/useBusy";
import { useToast } from "../../composables/useToast";

const props = defineProps<{ refresh: () => Promise<void> }>();
const { busy, run } = useBusy();
const toast = useToast();

async function call(name: string, fn: () => Promise<unknown>) {
  await run(async () => {
    try {
      await fn();
      toast.success(`${name} ok`);
      await props.refresh();
    } catch (e: any) {
      toast.error(`${name}: ${e?.message ?? String(e)}`);
    }
  });
}
</script>

<template>
  <section class="card">
    <h3>Quick actions</h3>
    <div class="actions">
      <button :disabled="busy" @click="call('start', serviceStart)">Start</button>
      <button :disabled="busy" @click="call('stop', serviceStop)">Stop</button>
      <button :disabled="busy" @click="call('restart', serviceRestart)">Restart</button>
      <button :disabled="busy" @click="props.refresh">Refresh</button>
    </div>
  </section>
</template>

<style scoped>
.card { background: #fff; border: 1px solid #e5e5e5; border-radius: 6px; padding: 1rem; }
.card h3 { margin: 0 0 0.5rem 0; font-size: 1rem; }
.actions { display: flex; flex-wrap: wrap; gap: 0.5rem; }
.actions button { padding: 0.4rem 0.9rem; border: 1px solid #bbb; border-radius: 4px; background: #f5f5f5; cursor: pointer; }
.actions button:hover:not(:disabled) { background: #e8e8e8; }
</style>
