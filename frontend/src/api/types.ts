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
