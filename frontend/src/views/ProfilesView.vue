<script setup lang="ts">
import { ref } from "vue";
import ProfileList from "../components/profiles/ProfileList.vue";
import ProfileDetail from "../components/profiles/ProfileDetail.vue";

const selectedId = ref<string | null>(null);
function onSelect(id: string) { selectedId.value = id; }
</script>

<template>
  <div class="profiles">
    <ProfileList :selected-id="selectedId" @select="onSelect" />
    <section v-if="!selectedId" class="empty">
      <p>Select a profile from the list, or import a new one.</p>
    </section>
    <ProfileDetail v-else :key="selectedId" :profile-id="selectedId" />
  </div>
</template>

<style scoped>
.profiles { display: grid; grid-template-columns: 22rem 1fr; gap: 1rem; align-items: start; }
.empty { background: #fff; border: 1px dashed #ccc; padding: 2rem; border-radius: 6px; color: #888; min-height: 18rem; display: flex; align-items: center; justify-content: center; }
</style>
