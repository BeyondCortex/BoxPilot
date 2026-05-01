<script setup lang="ts">
import { useHomeStatus } from "../composables/useHomeStatus";
import ServiceCard from "../components/home/ServiceCard.vue";
import ActiveProfileCard from "../components/home/ActiveProfileCard.vue";
import CoreCard from "../components/home/CoreCard.vue";
import DriftBanner from "../components/home/DriftBanner.vue";
import LogsPanel from "../components/home/LogsPanel.vue";
import QuickActions from "../components/home/QuickActions.vue";

defineProps<{ switchTab: (t: "home" | "profiles" | "settings") => void }>();

const { data, error, refresh } = useHomeStatus();
</script>

<template>
  <div class="home">
    <p v-if="error" class="err">{{ error }}</p>
    <DriftBanner :data="data" :switch-tab="switchTab" />
    <div class="grid">
      <ServiceCard :service="data?.service ?? null" />
      <ActiveProfileCard :active="data?.active_profile ?? null" />
      <CoreCard :core="data?.core ?? null" />
    </div>
    <QuickActions :refresh="refresh" />
    <LogsPanel />
  </div>
</template>

<style scoped>
.home { display: flex; flex-direction: column; gap: 1rem; }
.grid { display: grid; grid-template-columns: repeat(auto-fit, minmax(18rem, 1fr)); gap: 1rem; }
.err { background: #fde4e4; color: #6a1010; padding: 0.5rem 0.8rem; border-radius: 4px; }
</style>
