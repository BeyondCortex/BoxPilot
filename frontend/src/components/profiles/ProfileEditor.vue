<script setup lang="ts">
import { ref, watch, computed } from "vue";
import {
  profileGetSource, profileSaveSource, profileRevert, profileApplyPatchJson,
} from "../../api/profile";
import { useBusy } from "../../composables/useBusy";
import { useToast } from "../../composables/useToast";

const props = defineProps<{ profileId: string }>();
const emit = defineEmits<{ (e: "saved"): void }>();

const text = ref<string>("");
const original = ref<string>("");
const error = ref<string | null>(null);
const { busy, run } = useBusy();
const toast = useToast();

const dirty = computed(() => text.value !== original.value);

const clashAlreadyOn = computed(() => {
  try {
    const parsed = JSON.parse(text.value);
    return Boolean(parsed?.experimental?.clash_api?.external_controller);
  } catch {
    return false;
  }
});

async function load() {
  try {
    const s = await profileGetSource(props.profileId);
    text.value = s; original.value = s; error.value = null;
  } catch (e: any) {
    error.value = e?.message ?? String(e);
  }
}

watch(() => props.profileId, load, { immediate: true });

async function save() {
  await run(async () => {
    try {
      await profileSaveSource(props.profileId, text.value);
      original.value = text.value;
      toast.success("Saved");
      emit("saved");
    } catch (e: any) { toast.error(e?.message ?? String(e)); }
  });
}

async function revert() {
  await run(async () => {
    try {
      await profileRevert(props.profileId);
      await load();
      toast.success("Reverted to last-valid");
      emit("saved");
    } catch (e: any) { toast.error(e?.message ?? String(e)); }
  });
}

async function enableClash() {
  await run(async () => {
    try {
      const patch = JSON.stringify({
        experimental: {
          clash_api: { external_controller: "127.0.0.1:9090", secret: "" },
        },
      });
      await profileApplyPatchJson(props.profileId, patch);
      await load();
      toast.success("Clash API enabled on 127.0.0.1:9090");
      emit("saved");
    } catch (e: any) { toast.error(e?.message ?? String(e)); }
  });
}
</script>

<template>
  <div class="editor">
    <p v-if="error" class="err">{{ error }}</p>
    <textarea v-model="text" rows="20" spellcheck="false"></textarea>
    <div class="actions">
      <button :disabled="busy || !dirty" @click="save">Save</button>
      <button :disabled="busy" @click="revert">Revert to last-valid</button>
      <button
        :disabled="busy || clashAlreadyOn"
        :title="clashAlreadyOn ? 'Clash API is already configured' : 'Enable Clash API on 127.0.0.1:9090'"
        @click="enableClash"
      >
        Enable Clash API on loopback
      </button>
    </div>
  </div>
</template>

<style scoped>
.editor { display: flex; flex-direction: column; gap: 0.5rem; }
.editor textarea { width: 100%; box-sizing: border-box; font-family: ui-monospace, monospace; font-size: 0.85rem; padding: 0.5rem; }
.actions { display: flex; flex-wrap: wrap; gap: 0.5rem; }
.actions button { padding: 0.4rem 0.9rem; border: 1px solid #bbb; border-radius: 4px; background: #f5f5f5; cursor: pointer; }
.actions button:disabled { opacity: 0.5; cursor: default; }
.err { background: #fde4e4; color: #6a1010; padding: 0.5rem 0.8rem; border-radius: 4px; }
</style>
