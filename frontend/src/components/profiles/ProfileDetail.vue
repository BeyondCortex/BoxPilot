<script setup lang="ts">
import { ref } from "vue";
import ProfileOverview from "./ProfileOverview.vue";
import ProfileEditor from "./ProfileEditor.vue";
import ProfileActivate from "./ProfileActivate.vue";

const props = defineProps<{ profileId: string }>();

type DetailTab = "overview" | "editor" | "activate";
const tab = ref<DetailTab>("overview");

const bumpKey = ref(0);
function reloadEditor() {
  bumpKey.value++;
}
</script>

<template>
  <section class="detail">
    <nav class="tabs">
      <button :class="{ active: tab === 'overview' }" @click="tab = 'overview'">Overview</button>
      <button :class="{ active: tab === 'editor' }" @click="tab = 'editor'">Editor</button>
      <button :class="{ active: tab === 'activate' }" @click="tab = 'activate'">Activate</button>
    </nav>
    <ProfileOverview v-if="tab === 'overview'" :key="`ov-${props.profileId}-${bumpKey}`" :profile-id="props.profileId" />
    <ProfileEditor v-else-if="tab === 'editor'" :profile-id="props.profileId" @saved="reloadEditor" />
    <ProfileActivate v-else-if="tab === 'activate'" :profile-id="props.profileId" />
  </section>
</template>

<style scoped>
.detail { background: #fff; border: 1px solid #e5e5e5; border-radius: 6px; padding: 1rem; min-height: 24rem; display: flex; flex-direction: column; gap: 0.75rem; }
.tabs { display: flex; gap: 0.4rem; border-bottom: 1px solid #eee; padding-bottom: 0.25rem; }
.tabs button { padding: 0.3rem 0.7rem; border: 1px solid #ccc; border-radius: 4px; background: #f5f5f5; cursor: pointer; font-size: 0.85rem; }
.tabs button.active { background: #333; color: #fff; border-color: #333; }
</style>
