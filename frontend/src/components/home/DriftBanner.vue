<script setup lang="ts">
import { computed } from "vue";
import type { HomeStatusResponse } from "../../api/types";

const props = defineProps<{
  data: HomeStatusResponse | null;
  switchTab: (t: "home" | "profiles" | "settings") => void;
}>();

interface Banner {
  kind: "warn" | "error";
  message: string;
  action?: { label: string; run: () => void };
}

const banners = computed<Banner[]>(() => {
  const r: Banner[] = [];
  if (!props.data) return r;
  if (props.data.active_corrupt) {
    r.push({
      kind: "error",
      message:
        "BoxPilot's /etc/boxpilot/active link is corrupt. Activate a profile to restore.",
      action: { label: "Open Profiles", run: () => props.switchTab("profiles") },
    });
  }
  if (props.data.service.controller.kind === "orphaned") {
    r.push({
      kind: "warn",
      message: `Controller uid ${props.data.service.controller.uid} no longer exists. Privileged actions will be refused until a new controller is set.`,
    });
  }
  if (
    props.data.service.unit_state.kind === "known" &&
    props.data.service.unit_state.active_state === "failed"
  ) {
    r.push({
      kind: "error",
      message: "Service is in failed state. Inspect logs and consider rolling back.",
    });
  }
  return r;
});
</script>

<template>
  <div v-if="banners.length" class="banners">
    <div v-for="(b, i) in banners" :key="i" class="banner" :class="b.kind">
      <span>{{ b.message }}</span>
      <button v-if="b.action" @click="b.action.run">{{ b.action.label }}</button>
    </div>
  </div>
</template>

<style scoped>
.banners { display: flex; flex-direction: column; gap: 0.5rem; margin-bottom: 1rem; }
.banner { padding: 0.6rem 0.9rem; border-radius: 4px; display: flex; gap: 1rem; align-items: center; }
.banner.warn { background: #fff3cd; color: #5a4400; border: 1px solid #f0d68a; }
.banner.error { background: #fde4e4; color: #6a1010; border: 1px solid #f0a0a0; }
.banner button { margin-left: auto; padding: 0.25rem 0.6rem; border-radius: 3px; border: 1px solid currentColor; background: transparent; color: inherit; cursor: pointer; }
</style>
