<script setup lang="ts">
import { ref } from "vue";
import HomeView from "../views/HomeView.vue";
import ProfilesView from "../views/ProfilesView.vue";
import SettingsView from "../views/SettingsView.vue";
import Toast from "./Toast.vue";

type Tab = "home" | "profiles" | "settings";
const tab = ref<Tab>("home");
function switchTab(t: Tab) {
  tab.value = t;
}
</script>

<template>
  <div class="shell">
    <header class="topbar">
      <h1>BoxPilot</h1>
      <nav>
        <button :class="{ active: tab === 'home' }" @click="switchTab('home')">Home</button>
        <button :class="{ active: tab === 'profiles' }" @click="switchTab('profiles')">Profiles</button>
        <button :class="{ active: tab === 'settings' }" @click="switchTab('settings')">Settings</button>
      </nav>
    </header>
    <main>
      <HomeView v-if="tab === 'home'" :switch-tab="switchTab" />
      <ProfilesView v-else-if="tab === 'profiles'" />
      <SettingsView v-else-if="tab === 'settings'" />
    </main>
    <Toast />
  </div>
</template>

<style scoped>
.shell {
  font-family: system-ui, sans-serif;
  max-width: 64rem;
  margin: 0 auto;
  padding: 1rem;
}
.topbar {
  display: flex;
  align-items: baseline;
  gap: 1.5rem;
  border-bottom: 1px solid #ddd;
  padding-bottom: 0.5rem;
  margin-bottom: 1rem;
}
.topbar h1 { margin: 0; font-size: 1.4rem; }
nav { display: flex; gap: 0.5rem; }
nav button {
  padding: 0.4rem 0.9rem;
  border: 1px solid #ccc;
  background: #f7f7f7;
  cursor: pointer;
  border-radius: 4px;
}
nav button.active { background: #333; color: #fff; border-color: #333; }
main { min-height: 70vh; }
</style>
