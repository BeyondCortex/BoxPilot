<script setup lang="ts">
import { ref } from "vue";
import {
  profileImportFile, profileImportDir, profileImportRemote, profileRefreshRemote,
} from "../../api/profile";
import { useProfiles } from "../../composables/useProfiles";
import { useBusy } from "../../composables/useBusy";
import { useToast } from "../../composables/useToast";

const props = defineProps<{ selectedId: string | null }>();
const emit = defineEmits<{ (e: "select", id: string): void }>();

const { profiles, refresh } = useProfiles();
const { busy, run } = useBusy();
const toast = useToast();

const newName = ref("");
const newJsonPath = ref("");
const newDirPath = ref("");
const newRemoteUrl = ref("");

refresh();

async function importFile() {
  if (!newName.value || !newJsonPath.value) return;
  await run(async () => {
    try {
      await profileImportFile(newName.value, newJsonPath.value);
      newName.value = ""; newJsonPath.value = "";
      await refresh();
      toast.success("Imported");
    } catch (e: any) { toast.error(e?.message ?? String(e)); }
  });
}
async function importDir() {
  if (!newName.value || !newDirPath.value) return;
  await run(async () => {
    try {
      await profileImportDir(newName.value, newDirPath.value);
      newName.value = ""; newDirPath.value = "";
      await refresh();
      toast.success("Imported");
    } catch (e: any) { toast.error(e?.message ?? String(e)); }
  });
}
async function importRemote() {
  if (!newName.value || !newRemoteUrl.value) return;
  await run(async () => {
    try {
      await profileImportRemote(newName.value, newRemoteUrl.value);
      newName.value = ""; newRemoteUrl.value = "";
      await refresh();
      toast.success("Imported");
    } catch (e: any) { toast.error(e?.message ?? String(e)); }
  });
}
async function refreshOne(id: string) {
  await run(async () => {
    try {
      await profileRefreshRemote(id);
      await refresh();
      toast.success("Refreshed");
    } catch (e: any) { toast.error(e?.message ?? String(e)); }
  });
}
</script>

<template>
  <aside class="list">
    <h3>Profiles ({{ profiles.length }})</h3>
    <ul>
      <li v-for="p in profiles" :key="p.id" :class="{ active: p.id === props.selectedId }">
        <button class="select" @click="emit('select', p.id)">{{ p.name }}</button>
        <span class="meta">{{ p.source_kind }} · <code>{{ p.config_sha256.slice(0, 8) }}</code></span>
        <span v-if="p.remote_url_redacted" class="url" :title="p.remote_url_redacted">{{ p.remote_url_redacted }}</span>
        <button v-if="p.source_kind === 'remote'" :disabled="busy" @click="refreshOne(p.id)">↻</button>
      </li>
    </ul>
    <div class="add">
      <h4>Add</h4>
      <input v-model="newName" placeholder="Name" />
      <div class="row">
        <input v-model="newJsonPath" placeholder="/path/to/file.json" />
        <button :disabled="busy || !newName || !newJsonPath" @click="importFile">File</button>
      </div>
      <div class="row">
        <input v-model="newDirPath" placeholder="/path/to/profile-dir" />
        <button :disabled="busy || !newName || !newDirPath" @click="importDir">Dir</button>
      </div>
      <div class="row">
        <input v-model="newRemoteUrl" placeholder="https://host/...?token=" />
        <button :disabled="busy || !newName || !newRemoteUrl" @click="importRemote">Remote</button>
      </div>
    </div>
  </aside>
</template>

<style scoped>
.list { display: flex; flex-direction: column; gap: 1rem; }
.list h3 { margin: 0; font-size: 1rem; }
.list ul { list-style: none; padding: 0; margin: 0; display: flex; flex-direction: column; gap: 0.25rem; }
.list li { display: grid; grid-template-columns: 1fr auto auto; gap: 0.5rem; align-items: center; padding: 0.25rem 0.5rem; border: 1px solid #eee; border-radius: 4px; background: #fff; }
.list li.active { border-color: #333; }
.list li.active .select { font-weight: bold; }
.list li .select { background: none; border: none; padding: 0; cursor: pointer; text-align: left; }
.meta { color: #666; font-size: 0.8rem; grid-column: 1 / -1; }
.url { color: #557; font-family: monospace; font-size: 0.75rem; grid-column: 1 / -1; word-break: break-all; }
.add h4 { margin: 0 0 0.25rem; font-size: 0.85rem; }
.add input { width: 100%; box-sizing: border-box; padding: 0.3rem 0.4rem; margin-bottom: 0.25rem; }
.add .row { display: flex; gap: 0.4rem; align-items: center; }
.add .row input { flex: 1; margin-bottom: 0; }
.add .row button { padding: 0.3rem 0.6rem; border: 1px solid #bbb; border-radius: 4px; background: #f5f5f5; cursor: pointer; }
.list li button:not(.select) { padding: 0.2rem 0.4rem; border: 1px solid #bbb; border-radius: 3px; background: #f5f5f5; cursor: pointer; font-size: 0.8rem; }
</style>
