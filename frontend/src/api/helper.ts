import { invoke } from "@tauri-apps/api/core";
import type {
  CoreAdoptRequest, CoreDiscoverResponse, CoreInstallRequest,
  CoreInstallResponse, CoreRollbackRequest, ServiceStatusResponse, CommandError
} from "./types";

export async function serviceStatus(): Promise<ServiceStatusResponse> {
  return await invoke<ServiceStatusResponse>("helper_service_status");
}

export async function ping(): Promise<string> {
  return await invoke<string>("helper_ping");
}

export function isCommandError(e: unknown): e is CommandError {
  return (
    typeof e === "object" &&
    e !== null &&
    "code" in (e as Record<string, unknown>) &&
    "message" in (e as Record<string, unknown>)
  );
}

export async function coreDiscover(): Promise<CoreDiscoverResponse> {
  return await invoke<CoreDiscoverResponse>("helper_core_discover");
}
export async function coreInstallManaged(req: CoreInstallRequest): Promise<CoreInstallResponse> {
  return await invoke<CoreInstallResponse>("helper_core_install_managed", { request: req });
}
export async function coreUpgradeManaged(req: CoreInstallRequest): Promise<CoreInstallResponse> {
  return await invoke<CoreInstallResponse>("helper_core_upgrade_managed", { request: req });
}
export async function coreRollbackManaged(req: CoreRollbackRequest): Promise<CoreInstallResponse> {
  return await invoke<CoreInstallResponse>("helper_core_rollback_managed", { request: req });
}
export async function coreAdopt(req: CoreAdoptRequest): Promise<CoreInstallResponse> {
  return await invoke<CoreInstallResponse>("helper_core_adopt", { request: req });
}
