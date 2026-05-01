import { ref } from "vue";
import { profileList } from "../api/profile";
import type { ProfileSummary } from "../api/types";

const profiles = ref<ProfileSummary[]>([]);
const error = ref<string | null>(null);

async function refresh() {
  try {
    profiles.value = await profileList();
    error.value = null;
  } catch (e: any) {
    error.value = e?.message ?? String(e);
  }
}

export function useProfiles() {
  return { profiles, error, refresh };
}
