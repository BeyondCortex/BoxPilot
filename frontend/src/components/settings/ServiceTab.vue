<script setup lang="ts">
import { ref } from "vue";
import {
  serviceInstallManaged, serviceEnable, serviceDisable,
  serviceStart, serviceStop, serviceRestart,
} from "../../api/helper";
import { profileRollback } from "../../api/profile";
import { useHomeStatus } from "../../composables/useHomeStatus";
import { useBusy } from "../../composables/useBusy";
import { useToast } from "../../composables/useToast";

const { data: home, refresh } = useHomeStatus();
const { busy, run } = useBusy();
const toast = useToast();

async function call(name: string, fn: () => Promise<unknown>) {
  await run(async () => {
    try {
      await fn();
      toast.success(`${name} ok`);
      await refresh();
    } catch (e: any) { toast.error(`${name}: ${e?.message ?? String(e)}`); }
  });
}

const rollbackId = ref("");
async function rollback() {
  const target = rollbackId.value.trim();
  if (!target) return;
  if (!confirm(`Roll back to ${target}? This restarts sing-box.`)) return;
  await run(async () => {
    try {
      const r = await profileRollback({ target_activation_id: target });
      switch (r.outcome) {
        case "active": toast.success(`Rolled back to ${r.activation_id}`); break;
        case "rolled_back": toast.error("Rollback failed; previous restored"); break;
        case "rollback_target_missing": toast.error("Target not found on disk"); break;
        case "rollback_unstartable": toast.error("Target also failed; service stopped"); break;
      }
      await refresh();
    } catch (e: any) { toast.error(e?.message ?? String(e)); }
  });
}
</script>

<template>
  <div class="service">
    <h3>Service unit</h3>
    <p class="meta">Unit: <code>{{ home?.service?.unit_name ?? "?" }}</code></p>
    <p class="meta">State:
      <code v-if="home?.service?.unit_state.kind === 'known'">
        {{ home.service.unit_state.active_state }} ({{ home.service.unit_state.sub_state }})
      </code>
      <code v-else>not installed</code>
    </p>
    <div class="actions">
      <button :disabled="busy" @click="call('install unit', serviceInstallManaged)">Install / re-install unit</button>
      <button :disabled="busy" @click="call('enable', serviceEnable)">Enable</button>
      <button :disabled="busy" @click="call('disable', serviceDisable)">Disable</button>
      <button :disabled="busy" @click="call('start', serviceStart)">Start</button>
      <button :disabled="busy" @click="call('stop', serviceStop)">Stop</button>
      <button :disabled="busy" @click="call('restart', serviceRestart)">Restart</button>
    </div>

    <h3>Manual rollback</h3>
    <p class="meta">
      Paste an activation_id (format <code>YYYY-MM-DDTHH-MM-SSZ-xxxxxx</code>).
      You can copy it from the Activate tab's "previous" field after a fresh activation.
    </p>
    <div class="row">
      <input v-model="rollbackId" placeholder="2026-04-30T00-00-00Z-abc123" />
      <button :disabled="busy || !rollbackId.trim()" @click="rollback">Roll back</button>
    </div>
  </div>
</template>

<style scoped>
.service { display: flex; flex-direction: column; gap: 0.75rem; }
.service h3 { margin: 0; font-size: 1rem; }
.meta { color: #666; font-size: 0.85rem; margin: 0; }
.actions { display: flex; flex-wrap: wrap; gap: 0.5rem; }
.actions button { padding: 0.4rem 0.9rem; border: 1px solid #bbb; border-radius: 4px; background: #f5f5f5; cursor: pointer; }
.row { display: flex; gap: 0.5rem; }
.row input { flex: 1; padding: 0.3rem 0.4rem; font-family: ui-monospace, monospace; }
.row button { padding: 0.4rem 0.9rem; border: 1px solid #b22; border-radius: 4px; background: #fde4e4; color: #6a1010; cursor: pointer; }
code { background: #f0f0f0; padding: 0 0.25rem; border-radius: 2px; }
</style>
