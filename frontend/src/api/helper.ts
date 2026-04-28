import { invoke } from "@tauri-apps/api/core";
import type { ServiceStatusResponse, CommandError } from "./types";

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
