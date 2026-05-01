<script setup lang="ts">
import { ref } from "vue";
import {
  coreInstallManaged, coreUpgradeManaged, coreRollbackManaged, coreAdopt,
} from "../../api/helper";
import { useCores } from "../../composables/useCores";
import { useBusy } from "../../composables/useBusy";
import { useToast } from "../../composables/useToast";
import type { DiscoveredCore, VersionRequest } from "../../api/types";

const { data, refresh } = useCores();
const { busy, run } = useBusy();
const toast = useToast();

refresh();

const versionInput = ref("latest");
const adoptPath = ref("");

function buildVersionRequest(): VersionRequest {
  const v = versionInput.value.trim();
  if (!v || v === "latest") return { kind: "latest" };
  return { kind: "exact", version: v };
}

async function install() {
  await run(async () => {
    try {
      await coreInstallManaged({ version: buildVersionRequest(), architecture: { kind: "auto" } });
      await refresh();
      toast.success("Installed");
    } catch (e: any) { toast.error(e?.message ?? String(e)); }
  });
}
async function upgrade() {
  await run(async () => {
    try {
      await coreUpgradeManaged({ version: buildVersionRequest(), architecture: { kind: "auto" } });
      await refresh();
      toast.success("Upgraded");
    } catch (e: any) { toast.error(e?.message ?? String(e)); }
  });
}
async function makeActive(c: DiscoveredCore) {
  if (c.kind === "external") return;
  await run(async () => {
    try {
      await coreRollbackManaged({ to_label: c.label });
      await refresh();
      toast.success(`Switched to ${c.label}`);
    } catch (e: any) { toast.error(e?.message ?? String(e)); }
  });
}
async function adopt() {
  if (!adoptPath.value.trim()) return;
  await run(async () => {
    try {
      await coreAdopt({ source_path: adoptPath.value.trim() });
      adoptPath.value = "";
      await refresh();
      toast.success("Adopted");
    } catch (e: any) { toast.error(e?.message ?? String(e)); }
  });
}
</script>

<template>
  <div class="cores">
    <h3>Cores</h3>
    <div class="row">
      <label>Version <input v-model="versionInput" placeholder="latest or 1.10.0" /></label>
      <button :disabled="busy" @click="install">Install</button>
      <button :disabled="busy" @click="upgrade">Upgrade</button>
      <button :disabled="busy" @click="refresh">Refresh</button>
    </div>
    <table v-if="data">
      <thead>
        <tr><th></th><th>Label</th><th>Kind</th><th>Version</th><th>SHA-8</th><th></th></tr>
      </thead>
      <tbody>
        <tr v-for="c in data.cores" :key="c.label + c.path">
          <td>{{ data.current === c.label ? "●" : "" }}</td>
          <td>{{ c.label }}</td>
          <td>{{ c.kind }}</td>
          <td>{{ c.version || "?" }}</td>
          <td><code>{{ c.sha256.slice(0, 8) }}</code></td>
          <td>
            <button v-if="c.kind !== 'external' && data.current !== c.label"
                    :disabled="busy" @click="makeActive(c)">Make active</button>
          </td>
        </tr>
      </tbody>
    </table>
    <div class="adopt">
      <label>Adopt from path: <input v-model="adoptPath" placeholder="/usr/local/bin/sing-box" /></label>
      <button :disabled="busy || !adoptPath.trim()" @click="adopt">Adopt</button>
    </div>
  </div>
</template>

<style scoped>
.cores { display: flex; flex-direction: column; gap: 0.75rem; }
.row { display: flex; gap: 0.5rem; align-items: center; flex-wrap: wrap; }
.row input { padding: 0.3rem 0.4rem; }
table { border-collapse: collapse; width: 100%; background: #fff; }
th, td { padding: 0.3rem 0.5rem; text-align: left; border-bottom: 1px solid #eee; font-size: 0.9rem; }
.adopt { display: flex; gap: 0.5rem; align-items: center; }
.adopt input { flex: 1; padding: 0.3rem 0.4rem; }
button { padding: 0.4rem 0.9rem; border: 1px solid #bbb; border-radius: 4px; background: #f5f5f5; cursor: pointer; }
code { background: #f0f0f0; padding: 0 0.25rem; border-radius: 2px; }
</style>
