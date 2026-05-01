import { ref } from "vue";
import { coreDiscover } from "../api/helper";
import type { CoreDiscoverResponse } from "../api/types";

const data = ref<CoreDiscoverResponse | null>(null);
const error = ref<string | null>(null);

async function refresh() {
  try {
    data.value = await coreDiscover();
    error.value = null;
  } catch (e: any) {
    error.value = e?.message ?? String(e);
  }
}

export function useCores() {
  return { data, error, refresh };
}
