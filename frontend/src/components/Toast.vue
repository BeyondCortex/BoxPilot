<script setup lang="ts">
import { useToast } from "../composables/useToast";
const { entries, dismiss } = useToast();
</script>

<template>
  <div class="toast-stack">
    <div
      v-for="e in entries"
      :key="e.id"
      class="toast"
      :class="e.kind"
      @click="dismiss(e.id)"
    >
      {{ e.message }}
    </div>
  </div>
</template>

<style scoped>
.toast-stack {
  position: fixed;
  bottom: 1rem;
  right: 1rem;
  display: flex;
  flex-direction: column;
  gap: 0.5rem;
  z-index: 1000;
}
.toast {
  padding: 0.6rem 0.9rem;
  border-radius: 4px;
  background: #333;
  color: #fff;
  font-size: 0.9rem;
  cursor: pointer;
  max-width: 28rem;
}
.toast.success { background: #1e7d3a; }
.toast.error { background: #b22; }
.toast.info { background: #345; }
</style>
