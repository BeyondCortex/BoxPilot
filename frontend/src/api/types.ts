export type UnitState =
  | { kind: "not_found" }
  | {
      kind: "known";
      active_state: string;
      sub_state: string;
      load_state: string;
      n_restarts: number;
      exec_main_status: number;
    };

export type ControllerStatus =
  | { kind: "unset" }
  | { kind: "set"; uid: number; username: string }
  | { kind: "orphaned"; uid: number };

export interface ServiceStatusResponse {
  unit_name: string;
  unit_state: UnitState;
  controller: ControllerStatus;
}

export interface CommandError {
  code: string;
  message: string;
}

export type CoreKind = "external" | "managed-installed" | "managed-adopted";

export interface CoreSource {
  url: string | null;
  source_path: string | null;
  upstream_sha256_match: boolean | null;
  computed_sha256: string;
}

export interface DiscoveredCore {
  kind: CoreKind;
  path: string;
  version: string;
  sha256: string;
  installed_at: string | null;
  source: CoreSource | null;
  label: string;
}

export interface CoreDiscoverResponse {
  cores: DiscoveredCore[];
  current: string | null;
}

export type VersionRequest = { kind: "latest" } | { kind: "exact"; version: string };
export type ArchRequest = { kind: "auto" } | { kind: "exact"; arch: string };

export interface CoreInstallRequest {
  version: VersionRequest;
  architecture: ArchRequest;
}

export interface CoreInstallResponse {
  installed: DiscoveredCore;
  became_current: boolean;
  upstream_sha256_match: boolean | null;
  claimed_controller: boolean;
}

export interface CoreRollbackRequest { to_label: string; }
export interface CoreAdoptRequest { source_path: string; }

export interface ServiceControlResponse { unit_state: UnitState; }

export interface ServiceInstallManagedResponse {
  unit_state: UnitState;
  generated_unit_path: string;
  claimed_controller: boolean;
}

export interface ServiceLogsRequest { lines: number; }
export interface ServiceLogsResponse {
  lines: string[];
  truncated: boolean;
}
