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

export type SourceKind = "local" | "local-dir" | "remote";

export interface ProfileSummary {
  id: string;
  name: string;
  source_kind: SourceKind;
  remote_id: string | null;
  created_at: string;
  updated_at: string;
  last_valid_activation_id: string | null;
  config_sha256: string;
  remote_url_redacted: string | null;
}

export interface AssetEntry {
  path: string;
  sha256: string;
  size: number;
}

export interface ActivationManifest {
  schema_version: number;
  activation_id: string;
  profile_id: string;
  profile_sha256: string;
  config_sha256: string;
  source_kind: SourceKind;
  source_url_redacted: string | null;
  core_path_at_activation: string;
  core_version_at_activation: string;
  created_at: string;
  assets: AssetEntry[];
}

export interface PrepareBundleRequest {
  profile_id: string;
  core_path: string;
  core_version: string;
}

export interface PrepareBundleResponse {
  staging_path: string;
  manifest: ActivationManifest;
}

export interface CheckRequest { profile_id: string; core_path: string; }
export interface CheckResponse { success: boolean; stdout: string; stderr: string; }
