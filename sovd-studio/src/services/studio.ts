// M2: session, trace and license commands of the desktop shell.
import { invoke } from "@tauri-apps/api/core";
import { inTauri, type ApiResponse } from "./transport";

export type SessionInfo = {
  user: string;
  role: "viewer" | "technician" | "engineer";
  expires_unix: number;
  idle_timeout_secs: number;
};

export type StudioInfo = {
  version: string;
  config_path: string | null;
  startup_error: string | null;
  auth_url: string | null;
  central_url: string | null;
  tls_server_name: string | null;
  device_cert: string | null;
  idle_timeout_secs: number | null;
};

export type TraceEntry = {
  seq: number;
  ts_unix_ms: number;
  user: string | null;
  method: string;
  path: string;
  status: number | null;
  elapsed_ms: number;
  request_excerpt: string | null;
  response_excerpt: string | null;
  error: string | null;
};

export type Feature =
  | "diagnostics" | "raw-uds" | "odx-import" | "sequences" | "reports" | "flashing" | "automation-api";

export type LicenseStatus = {
  licensed_to: string;
  kind: string;
  features: [Feature, boolean][];
};

// M3 (faults, operations, polling). Every rule lives in studio-core; these only call it.
export type StatusBit = { bit: number; name: string; set: boolean };
export type RunState = "running" | "completed" | "failed";
export type OperationCapabilities = {
  execute: boolean;
  stop: boolean;
  freeze: boolean;
  reset: boolean;
  status: boolean;
};
export type OperationInfo = {
  id: string;
  name: string;
  description: string;
  supports_execution: boolean;
  capabilities: OperationCapabilities | null;
};

/** Browser dev mode has no Rust core; keep equal to studio-core::polling::POLL_INTERVAL_SECS. */
const BROWSER_POLL_MS = 5000;

export const ROLE_RANK: Record<SessionInfo["role"], number> = { viewer: 0, technician: 1, engineer: 2 };

export const studio = {
  info: () => invoke<StudioInfo>("studio_info"),
  login: (username: string, password: string) => invoke<SessionInfo>("login", { username, password }),
  logout: () => invoke<void>("logout"),
  session: () => invoke<SessionInfo | null>("session_info"),
  traceSince: (since: number) =>
    inTauri() ? invoke<TraceEntry[]>("trace_since", { since }) : Promise.resolve([] as TraceEntry[]),
  traceClear: () => (inTauri() ? invoke<void>("trace_clear") : Promise.resolve()),
  license: () =>
    inTauri()
      ? invoke<LicenseStatus>("license_status")
      : Promise.resolve<LicenseStatus>({ licensed_to: "browser dev mode", kind: "dev", features: [] }),

  // M3
  pollIntervalMs: () =>
    inTauri() ? invoke<number>("poll_interval_secs").then((s) => s * 1000) : Promise.resolve(BROWSER_POLL_MS),
  decodeStatus: (status: number) => invoke<StatusBit[]>("decode_dtc_status", { status }),
  canChange: () => invoke<boolean>("can_change"),
  faultList: (ecu: string, active?: boolean) =>
    invoke<ApiResponse>("fault_list", { ecu, active: active ?? null }),
  clearFaults: (ecu: string, code: string | null, confirm: string | null) =>
    invoke<ApiResponse>("clear_faults", { ecu, code, confirm }),
  operationList: (ecu: string) => invoke<OperationInfo[]>("operation_list", { ecu }),
  operationHistory: (ecu: string, operation: string) =>
    invoke<ApiResponse>("operation_history", { ecu, operation }),
  operationStart: (ecu: string, operation: string) =>
    invoke<string>("operation_start", { ecu, operation }),
  operationStatus: (ecu: string, operation: string, execution: string) =>
    invoke<RunState>("operation_status", { ecu, operation, execution }),
  operationStop: (ecu: string, operation: string, execution: string) =>
    invoke<void>("operation_stop", { ecu, operation, execution }),
};
