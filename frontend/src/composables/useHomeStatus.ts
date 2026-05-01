import { onMounted, onUnmounted, ref } from "vue";
import { homeStatus } from "../api/helper";
import type { HomeStatusResponse } from "../api/types";

const POLL_MS = 5000;

// Module-scope singletons so every consumer sees the same data and a
// single shared poll loop, no matter how many components mount the
// composable. Matches the pattern in useProfiles / useCores.
const data = ref<HomeStatusResponse | null>(null);
const error = ref<string | null>(null);
let mountCount = 0;
let timer: number | null = null;
let stopped = false;

async function refresh() {
  try {
    data.value = await homeStatus();
    error.value = null;
  } catch (e: any) {
    error.value = e?.message ?? String(e);
  }
}

function schedule() {
  if (stopped) return;
  if (document.hidden) {
    timer = window.setTimeout(schedule, POLL_MS);
    return;
  }
  refresh().finally(() => {
    if (!stopped) timer = window.setTimeout(schedule, POLL_MS);
  });
}

function onVisibility() {
  if (!document.hidden) refresh();
}

function start() {
  if (mountCount === 0) {
    stopped = false;
    document.addEventListener("visibilitychange", onVisibility);
    schedule();
  }
  mountCount++;
}

function stop() {
  mountCount = Math.max(0, mountCount - 1);
  if (mountCount === 0) {
    stopped = true;
    if (timer !== null) {
      window.clearTimeout(timer);
      timer = null;
    }
    document.removeEventListener("visibilitychange", onVisibility);
  }
}

export function useHomeStatus() {
  onMounted(start);
  onUnmounted(stop);
  return { data, error, refresh };
}
