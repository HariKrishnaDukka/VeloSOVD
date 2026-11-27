// M2: single transport for every central API call.
//
// Desktop (Tauri): the request goes to Rust (`api_request`), which adds the
// device certificate and the user's token. The token never enters this web view.
// Browser dev mode: plain fetch through the Vite /api proxy (no login).
import { invoke } from "@tauri-apps/api/core";

export type ApiResponse = {
  status: number;
  content_type: string | null;
  body: string;
  elapsed_ms: number;
};

/** Error shape returned by studio-core (`StudioError`). */
export type StudioError = { kind: string; message?: string };

export const inTauri = (): boolean =>
  typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

type Listener = (reason: string) => void;
const sessionListeners = new Set<Listener>();

/** Called when Rust ends the session (idle, expiry, server 401). */
export function onSessionLost(listener: Listener): () => void {
  sessionListeners.add(listener);
  return () => sessionListeners.delete(listener);
}

export class SessionLostError extends Error {}

/** Ends the session in the shell when a Rust command reports it expired; true if it did. */
export function notifySessionLost(error: unknown): boolean {
  const e = error as StudioError;
  if (e?.kind === "session-expired" || e?.kind === "not-logged-in") {
    const reason = e.message ?? "Please log in again.";
    sessionListeners.forEach((l) => l(reason));
    return true;
  }
  return false;
}

export function describe(error: unknown): string {
  if (error instanceof Error) return error.message;
  const e = error as StudioError;
  if (e && typeof e.kind === "string") return e.message ? `${e.kind}: ${e.message}` : e.kind;
  return String(error);
}

export async function request(method: string, path: string, body?: string): Promise<ApiResponse> {
  if (inTauri()) {
    try {
      return await invoke<ApiResponse>("api_request", { method, path, body: body ?? null });
    } catch (error) {
      const e = error as StudioError;
      if (e?.kind === "session-expired" || e?.kind === "not-logged-in") {
        const reason = e.message ?? "Please log in again.";
        sessionListeners.forEach((l) => l(reason));
        throw new SessionLostError(reason);
      }
      throw new Error(describe(error));
    }
  }
  const started = performance.now();
  const response = await fetch(path, {
    method,
    headers: {
      Accept: "application/json",
      ...(body ? { "Content-Type": "application/json" } : {}),
    },
    body,
  });
  return {
    status: response.status,
    content_type: response.headers.get("content-type"),
    body: await response.text(),
    elapsed_ms: Math.round(performance.now() - started),
  };
}

/** Throw a readable error for >= 400 responses (uses the SOVD error body). */
export function ensureOk(response: ApiResponse): ApiResponse {
  if (response.status < 400) return response;
  let detail = "";
  try {
    const parsed = JSON.parse(response.body) as { error_code?: string; message?: string };
    detail = [parsed.error_code, parsed.message].filter(Boolean).join(": ");
  } catch {
    detail = response.body.slice(0, 200);
  }
  throw new Error(`HTTP ${response.status}${detail ? ` - ${detail}` : ""}`);
}
