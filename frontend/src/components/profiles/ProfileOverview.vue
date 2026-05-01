<script setup lang="ts">
import { computed, watch, ref } from "vue";
import { profileGetSource } from "../../api/profile";

const props = defineProps<{ profileId: string }>();

const text = ref<string>("");
const error = ref<string | null>(null);

async function reload() {
  try {
    text.value = await profileGetSource(props.profileId);
    error.value = null;
  } catch (e: any) {
    error.value = e?.message ?? String(e);
  }
}

watch(() => props.profileId, reload, { immediate: true });

const parsed = computed<any | null>(() => {
  try { return JSON.parse(text.value); }
  catch { return null; }
});

function listKind<T = any>(arr: any): T[] {
  return Array.isArray(arr) ? (arr as T[]) : [];
}

const inbounds = computed<any[]>(() => listKind(parsed.value?.inbounds));
const outbounds = computed<any[]>(() => listKind(parsed.value?.outbounds));
const ruleSets = computed<any[]>(() => listKind(parsed.value?.route?.rule_set));
const dnsServers = computed<any[]>(() => listKind(parsed.value?.dns?.servers));
const tunInbound = computed<any | null>(() => inbounds.value.find((i: any) => i?.type === "tun") ?? null);
const clashApi = computed<any | null>(() => parsed.value?.experimental?.clash_api ?? null);
</script>

<template>
  <div class="overview">
    <p v-if="error" class="err">read source: {{ error }}</p>
    <p v-else-if="!parsed" class="muted">Config is not valid JSON. Use the Editor tab to fix it.</p>
    <template v-else>
      <h4>Inbounds ({{ inbounds.length }})</h4>
      <ul>
        <li v-for="(i, idx) in inbounds" :key="idx">
          <code>{{ i.type ?? "(no type)" }}</code>
          <span v-if="i.tag"> · tag <code>{{ i.tag }}</code></span>
          <span v-if="i.listen_port"> · port {{ i.listen_port }}</span>
        </li>
      </ul>
      <h4 v-if="tunInbound">TUN settings</h4>
      <ul v-if="tunInbound" class="kv">
        <li>auto_route: <code>{{ tunInbound.auto_route ?? "(unset)" }}</code></li>
        <li>auto_redirect: <code>{{ tunInbound.auto_redirect ?? "(unset)" }}</code></li>
        <li>strict_route: <code>{{ tunInbound.strict_route ?? "(unset)" }}</code></li>
      </ul>
      <h4>Outbounds ({{ outbounds.length }})</h4>
      <ul>
        <li v-for="(o, idx) in outbounds" :key="idx">
          <code>{{ o.type ?? "(no type)" }}</code>
          <span v-if="o.tag"> · tag <code>{{ o.tag }}</code></span>
        </li>
      </ul>
      <h4>DNS servers ({{ dnsServers.length }})</h4>
      <ul>
        <li v-for="(s, idx) in dnsServers" :key="idx">
          <code>{{ s.tag ?? `#${idx}` }}</code>: <code>{{ s.address ?? s.url ?? "(?)" }}</code>
        </li>
      </ul>
      <h4>Route rule_set ({{ ruleSets.length }})</h4>
      <ul>
        <li v-for="(r, idx) in ruleSets" :key="idx">
          <code>{{ r.tag ?? `#${idx}` }}</code> ({{ r.type ?? r.format ?? "?" }})
        </li>
      </ul>
      <h4 v-if="clashApi">Clash API</h4>
      <ul v-if="clashApi" class="kv">
        <li>external_controller: <code>{{ clashApi.external_controller ?? "(unset)" }}</code></li>
        <li>secret set: <code>{{ clashApi.secret ? "yes" : "no" }}</code></li>
      </ul>
    </template>
  </div>
</template>

<style scoped>
.overview { display: flex; flex-direction: column; gap: 0.5rem; }
.overview h4 { margin: 0.5rem 0 0.25rem; font-size: 0.9rem; }
.overview ul { margin: 0; padding-left: 1.2rem; }
.overview .kv { list-style: none; padding-left: 0; }
.muted { color: #888; }
.err { background: #fde4e4; color: #6a1010; padding: 0.4rem 0.6rem; border-radius: 4px; }
code { background: #f0f0f0; padding: 0 0.25rem; border-radius: 2px; }
</style>
