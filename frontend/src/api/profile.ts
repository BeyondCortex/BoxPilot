import { invoke } from "@tauri-apps/api/core";
import type {
  CheckRequest, CheckResponse,
  PrepareBundleRequest, PrepareBundleResponse,
  ProfileSummary,
} from "./types";

export async function profileList(): Promise<ProfileSummary[]> {
  return await invoke<ProfileSummary[]>("profile_list");
}
export async function profileGetSource(id: string): Promise<string> {
  return await invoke<string>("profile_get_source", { id });
}
export async function profileImportFile(name: string, path: string): Promise<ProfileSummary> {
  return await invoke<ProfileSummary>("profile_import_file", { name, path });
}
export async function profileImportDir(name: string, dir: string): Promise<ProfileSummary> {
  return await invoke<ProfileSummary>("profile_import_dir", { name, dir });
}
export async function profileImportRemote(name: string, url: string): Promise<ProfileSummary> {
  return await invoke<ProfileSummary>("profile_import_remote", { name, url });
}
export async function profileRefreshRemote(id: string): Promise<ProfileSummary> {
  return await invoke<ProfileSummary>("profile_refresh_remote", { id });
}
export async function profileSaveSource(id: string, source: string): Promise<void> {
  await invoke<void>("profile_save_source", { id, source });
}
export async function profileApplyPatchJson(id: string, patchJson: string): Promise<void> {
  await invoke<void>("profile_apply_patch_json", { id, patchJson });
}
export async function profileRevert(id: string): Promise<void> {
  await invoke<void>("profile_revert", { id });
}
export async function profilePrepareBundle(req: PrepareBundleRequest): Promise<PrepareBundleResponse> {
  return await invoke<PrepareBundleResponse>("profile_prepare_bundle", { request: req });
}
export async function profileCheck(req: CheckRequest): Promise<CheckResponse> {
  return await invoke<CheckResponse>("profile_check", { request: req });
}
