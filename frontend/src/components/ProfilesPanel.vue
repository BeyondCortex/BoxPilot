<script setup lang="ts">
import { onMounted, ref } from "vue";
import {
  profileApplyPatchJson, profileCheck, profileGetSource, profileImportDir,
  profileImportFile, profileImportRemote, profileList, profilePrepareBundle,
  profileRefreshRemote, profileRevert, profileSaveSource,
} from "../api/profile";
import type { CheckResponse, PrepareBundleResponse, ProfileSummary } from "../api/types";

const profiles = ref<ProfileSummary[]>([]);
const selected = ref<string | null>(null);
const editorText = ref("");
const status = ref<string>("");
const lastBundle = ref<PrepareBundleResponse | null>(null);
const lastCheck = ref<CheckResponse | null>(null);

const newName = ref("");
const newJsonPath = ref("");
const newDirPath = ref("");
const newRemoteUrl = ref("");

const corePath = ref("/var/lib/boxpilot/cores/current/sing-box");
const coreVersion = ref("unknown");

async function refresh() {
  try { profiles.value = await profileList(); }
  catch (e) { status.value = `list failed: ${JSON.stringify(e)}`; }
}

async function selectProfile(id: string) {
  selected.value = id;
  try { editorText.value = await profileGetSource(id); }
  catch (e) { status.value = `read failed: ${JSON.stringify(e)}`; }
}

async function importFile() {
  if (!newName.value || !newJsonPath.value) return;
  try { await profileImportFile(newName.value, newJsonPath.value); newName.value = ""; newJsonPath.value = ""; await refresh(); }
  catch (e) { status.value = `import file: ${JSON.stringify(e)}`; }
}

async function importDir() {
  if (!newName.value || !newDirPath.value) return;
  try { await profileImportDir(newName.value, newDirPath.value); newName.value = ""; newDirPath.value = ""; await refresh(); }
  catch (e) { status.value = `import dir: ${JSON.stringify(e)}`; }
}

async function importRemote() {
  if (!newName.value || !newRemoteUrl.value) return;
  try { await profileImportRemote(newName.value, newRemoteUrl.value); newName.value = ""; newRemoteUrl.value = ""; await refresh(); }
  catch (e) { status.value = `import remote: ${JSON.stringify(e)}`; }
}

async function refreshRemote(id: string) {
  try { await profileRefreshRemote(id); await refresh(); }
  catch (e) { status.value = `refresh: ${JSON.stringify(e)}`; }
}

async function save() {
  if (!selected.value) return;
  try { await profileSaveSource(selected.value, editorText.value); status.value = "saved"; await refresh(); }
  catch (e) { status.value = `save: ${JSON.stringify(e)}`; }
}

async function revert() {
  if (!selected.value) return;
  try { await profileRevert(selected.value); editorText.value = await profileGetSource(selected.value); status.value = "reverted"; }
  catch (e) { status.value = `revert: ${JSON.stringify(e)}`; }
}

async function prepareBundle() {
  if (!selected.value) return;
  try {
    lastBundle.value = await profilePrepareBundle({
      profile_id: selected.value, core_path: corePath.value, core_version: coreVersion.value,
    });
    status.value = `bundle ready @ ${lastBundle.value.staging_path}`;
  } catch (e) { status.value = `bundle: ${JSON.stringify(e)}`; }
}

async function runCheck() {
  if (!selected.value) return;
  try {
    lastCheck.value = await profileCheck({ profile_id: selected.value, core_path: corePath.value });
    status.value = lastCheck.value.success ? "check OK" : "check FAILED";
  } catch (e) { status.value = `check: ${JSON.stringify(e)}`; }
}

onMounted(refresh);
</script>

<template>
  <section class="profiles">
    <h2>Profiles</h2>
    <div v-if="status" class="status">{{ status }}</div>

    <div class="add">
      <h3>Add</h3>
      <label>Name <input v-model="newName" placeholder="My Profile" /></label>
      <div class="row">
        <input v-model="newJsonPath" placeholder="/path/to/file.json" />
        <button :disabled="!newName || !newJsonPath" @click="importFile">Import file</button>
      </div>
      <div class="row">
        <input v-model="newDirPath" placeholder="/path/to/profile-dir" />
        <button :disabled="!newName || !newDirPath" @click="importDir">Import directory</button>
      </div>
      <div class="row">
        <input v-model="newRemoteUrl" placeholder="https://host/path?token=…" />
        <button :disabled="!newName || !newRemoteUrl" @click="importRemote">Add remote</button>
      </div>
    </div>

    <div class="list">
      <h3>Profiles ({{ profiles.length }})</h3>
      <ul>
        <li v-for="p in profiles" :key="p.id" :class="{ active: p.id === selected }">
          <button @click="selectProfile(p.id)">{{ p.name }}</button>
          <span class="meta">{{ p.source_kind }} · {{ p.config_sha256.slice(0, 8) }}</span>
          <span class="url" v-if="p.remote_url_redacted">{{ p.remote_url_redacted }}</span>
          <button v-if="p.source_kind === 'remote'" @click="refreshRemote(p.id)">Refresh</button>
        </li>
      </ul>
    </div>

    <div v-if="selected" class="editor">
      <h3>Editor</h3>
      <textarea v-model="editorText" rows="20" cols="80"></textarea>
      <div class="row">
        <button @click="save">Save</button>
        <button @click="revert">Revert to last-valid</button>
      </div>

      <h3>Activation</h3>
      <label>Core path <input v-model="corePath" /></label>
      <label>Core version <input v-model="coreVersion" /></label>
      <div class="row">
        <button @click="runCheck">Best-effort check</button>
        <button @click="prepareBundle">Prepare bundle (preview)</button>
      </div>
      <pre v-if="lastCheck">{{ lastCheck.success ? 'OK' : 'FAIL' }}
{{ lastCheck.stderr || lastCheck.stdout }}</pre>
      <pre v-if="lastBundle">{{ JSON.stringify(lastBundle.manifest, null, 2) }}</pre>
    </div>
  </section>
</template>

<style scoped>
.profiles { display: flex; flex-direction: column; gap: 1rem; }
.row { display: flex; gap: 0.5rem; align-items: center; }
.list ul { list-style: none; padding: 0; }
.list li { display: flex; gap: 0.5rem; align-items: center; padding: 0.25rem 0; }
.list li.active button:first-child { font-weight: bold; }
.meta { color: #888; font-size: 0.85rem; }
.url { font-family: monospace; font-size: 0.85rem; color: #555; }
textarea { font-family: monospace; }
.status { background: #ffd; padding: 0.5rem; border-radius: 4px; }
</style>
