// M2: session, trace and license commands of the desktop shell.
import { invoke } from "@tauri-apps/api/core";
import { inTauri } from "./transport";

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
};
