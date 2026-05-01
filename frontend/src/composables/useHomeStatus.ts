import { onMounted, onUnmounted, ref } from "vue";
import { homeStatus } from "../api/helper";
import type { HomeStatusResponse } from "../api/types";

const POLL_MS = 5000;

export function useHomeStatus() {
  const data = ref<HomeStatusResponse | null>(null);
  const error = ref<string | null>(null);
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

  onMounted(() => {
    document.addEventListener("visibilitychange", onVisibility);
    schedule();
  });

  onUnmounted(() => {
    stopped = true;
    if (timer !== null) window.clearTimeout(timer);
    document.removeEventListener("visibilitychange", onVisibility);
  });

  return { data, error, refresh };
}
