import { ref } from "vue";

export function useBusy() {
  const busy = ref(false);
  async function run<T>(fn: () => Promise<T>): Promise<T | undefined> {
    if (busy.value) return undefined;
    busy.value = true;
    try {
      return await fn();
    } finally {
      busy.value = false;
    }
  }
  return { busy, run };
}
