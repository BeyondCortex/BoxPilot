<script setup lang="ts">
import { ref } from "vue";
import {
  legacyObserveService, legacyMigratePrepare, legacyMigrateCutover,
} from "../../api/helper";
import { profileImportFile, profileImportDir } from "../../api/profile";
import { useBusy } from "../../composables/useBusy";
import { useToast } from "../../composables/useToast";
import { useHomeStatus } from "../../composables/useHomeStatus";
import type { LegacyObserveServiceResponse } from "../../api/types";

const observed = ref<LegacyObserveServiceResponse | null>(null);
const profileName = ref("imported-legacy");
const importedProfileId = ref<string | null>(null);
const cutoverDone = ref(false);
const { busy, run } = useBusy();
const toast = useToast();
const { data: home } = useHomeStatus();

async function scan() {
  await run(async () => {
    try {
      observed.value = await legacyObserveService();
      cutoverDone.value = false;
      importedProfileId.value = null;
    } catch (e: any) { toast.error(e?.message ?? String(e)); }
  });
}

async function importFromPath() {
  const o = observed.value;
  if (!o?.config_path) return;
  await run(async () => {
    try {
      // Prepare exercises the helper's path-safety classifier as defence
      // in depth; we ignore the returned bytes because importFile reads
      // the system path itself.
      await legacyMigratePrepare();
      const summary = await profileImportFile(profileName.value, o.config_path!);
      importedProfileId.value = summary.id;
      toast.success(`Imported as ${summary.name}`);
    } catch (e: any) { toast.error(e?.message ?? String(e)); }
  });
}

async function importDirFromParent() {
  const o = observed.value;
  if (!o?.config_path) return;
  const dir = o.config_path.replace(/\/[^/]+$/, "") || "/";
  await run(async () => {
    try {
      await legacyMigratePrepare();
      const summary = await profileImportDir(profileName.value, dir);
      importedProfileId.value = summary.id;
      toast.success(`Imported (with assets) as ${summary.name}`);
    } catch (e: any) { toast.error(e?.message ?? String(e)); }
  });
}

async function cutover() {
  const haveActive = home.value?.active_profile != null;
  const message = haveActive
    ? "Stop and disable the legacy unit? Your active BoxPilot profile will take over."
    : "WARNING: no BoxPilot profile is currently active. Stopping the legacy unit will leave NO sing-box running on this system. Continue anyway?";
  if (!confirm(message)) return;
  await run(async () => {
    try {
      await legacyMigrateCutover();
      cutoverDone.value = true;
      toast.success("Legacy unit stopped + disabled");
    } catch (e: any) { toast.error(e?.message ?? String(e)); }
  });
}
</script>

<template>
  <div class="legacy">
    <h3>Legacy sing-box.service</h3>
    <p class="meta">Detects an existing system-installed sing-box service and migrates its config + assets into a BoxPilot profile.</p>

    <button :disabled="busy" @click="scan">Scan</button>

    <section v-if="observed && !observed.detected" class="block ok">
      <p>No legacy <code>sing-box.service</code> found on this system.</p>
    </section>

    <section v-else-if="observed" class="block">
      <h4>Detected</h4>
      <p>Unit: <code>{{ observed.unit_name }}</code> (<code>{{ observed.unit_file_state ?? "?" }}</code>)</p>
      <p>Fragment: <code>{{ observed.fragment_path ?? "?" }}</code></p>
      <p>ExecStart: <code>{{ observed.exec_start_raw ?? "?" }}</code></p>
      <p>Config: <code>{{ observed.config_path ?? "(unparseable)" }}</code> — <strong>{{ observed.config_path_kind }}</strong></p>

      <div v-if="observed.config_path_kind === 'system_path'" class="actions">
        <label>Profile name <input v-model="profileName" /></label>
        <button :disabled="busy" @click="importFromPath">Import as profile (file)</button>
        <button :disabled="busy" @click="importDirFromParent">Import as profile (dir + assets)</button>
      </div>
      <p v-else-if="observed.config_path_kind === 'user_or_ephemeral'" class="meta warn">
        Refusing to migrate: config path is under a user / ephemeral location.
        Move it to <code>/etc</code> or import it manually via the Profiles tab.
      </p>
      <p v-else class="meta warn">
        Could not parse <code>-c</code> from ExecStart. Import the config manually via the Profiles tab.
      </p>

      <div v-if="importedProfileId" class="next">
        <p>Imported. Activate the new profile under <strong>Profiles</strong> first, then run cutover here.</p>
        <button :disabled="busy || cutoverDone" @click="cutover">
          {{ cutoverDone ? "Cutover done" : "Stop + disable legacy unit" }}
        </button>
      </div>
      <div v-else class="next">
        <p class="meta">If you've already imported the config separately, you can still cut over:</p>
        <button :disabled="busy || cutoverDone" @click="cutover">
          {{ cutoverDone ? "Cutover done" : "Stop + disable legacy unit only" }}
        </button>
      </div>
    </section>
  </div>
</template>

<style scoped>
.legacy { display: flex; flex-direction: column; gap: 0.75rem; }
.legacy h3 { margin: 0; font-size: 1rem; }
.meta { color: #666; font-size: 0.85rem; margin: 0; }
.meta.warn { background: #fff3cd; color: #5a4400; padding: 0.4rem 0.6rem; border-radius: 4px; }
.block { padding: 0.6rem 0.9rem; background: #f7f7f7; border: 1px solid #ddd; border-radius: 4px; display: flex; flex-direction: column; gap: 0.3rem; }
.block.ok { background: #e6f4ea; border-color: #b9d8c2; }
.actions { display: flex; flex-wrap: wrap; gap: 0.5rem; align-items: center; margin-top: 0.4rem; }
.actions input { padding: 0.3rem 0.4rem; }
.next { display: flex; flex-direction: column; gap: 0.4rem; margin-top: 0.4rem; }
button { padding: 0.4rem 0.9rem; border: 1px solid #bbb; border-radius: 4px; background: #f5f5f5; cursor: pointer; }
code { background: #f0f0f0; padding: 0 0.25rem; border-radius: 2px; }
</style>
