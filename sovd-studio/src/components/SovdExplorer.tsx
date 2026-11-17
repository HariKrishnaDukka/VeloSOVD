import { useEffect, useMemo, useState } from "react";
import {
  AlertTriangle, CheckCircle2, ChevronRight, CircleDot,
  FileJson, Filter, RefreshCw, Send, Server, Shield,
  Terminal, Wifi, Play, Clock3,
} from "lucide-react";
import { api } from "../services/api";
import type {
  Ecu, Fault, ResourceDescriptor, OperationDescriptor, OperationExecution,
} from "../types";

type HttpMethod = "GET" | "POST";
type Resource = {
  id: string;
  label: string;
  description: string;
  method: HttpMethod;
  path: string;
  body?: string;
  simulated?: boolean;
};

function pretty(value: unknown): string {
  try {
    return JSON.stringify(value, null, 2);
  } catch {
    return String(value);
  }
}

function globalResources(): Resource[] {
  return [
    {
      id: "ecu-inventory",
      label: "ECU inventory",
      description: "Discover registered ECU components",
      method: "GET",
      path: "/api/ecus",
    },
    {
      id: "vehicle-topology",
      label: "Vehicle topology",
      description: "Central gateway, zones and ECU network mapping",
      method: "GET",
      path: "/api/topology",
    },
    {
      id: "vehicle-health",
      label: "Vehicle health",
      description: "Overall and zonal health status",
      method: "GET",
      path: "/api/health",
    },
  ];
}

function mapResource(item: ResourceDescriptor): Resource | null {
  const method = item.methods.find(
    (candidate): candidate is HttpMethod => candidate === "GET" || candidate === "POST",
  );
  if (!method || !item.path.startsWith("/api/")) return null;

  return {
    id: item.id,
    label: item.id.replaceAll("-", " "),
    description: item.description,
    method,
    path: item.path,
    body: item.id === "uds-diagnostic-request"
      ? JSON.stringify({ payload_hex: "22 F1 90" }, null, 2)
      : undefined,
    simulated: item.simulated,
  };
}

export default function SovdExplorer() {
  const [ecus, setEcus] = useState<Ecu[]>([]);
  const [ecuId, setEcuId] = useState("ECM");
  const [resourceDescriptors, setResourceDescriptors] = useState<ResourceDescriptor[]>([]);
  const [operations, setOperations] = useState<OperationDescriptor[]>([]);
  const [executions, setExecutions] = useState<OperationExecution[]>([]);
  const [operationBusy, setOperationBusy] = useState("");

  const [method, setMethod] = useState<HttpMethod>("GET");
  const [path, setPath] = useState("/api/ecus");
  const [body, setBody] = useState("");

  const [baseUrl, setBaseUrl] = useState(() => {
    try {
      return localStorage.getItem("sovd.baseUrl") || window.location.origin;
    } catch {
      return window.location.origin;
    }
  });
  const [profileName, setProfileName] = useState(() => {
    try {
      return localStorage.getItem("sovd.profileName") || "Local Rust SOVD";
    } catch {
      return "Local Rust SOVD";
    }
  });
  const [bearerToken, setBearerToken] = useState("");
  const [authEnabled, setAuthEnabled] = useState(false);

  const [response, setResponse] = useState("");
  const [status, setStatus] = useState<number | null>(null);
  const [elapsed, setElapsed] = useState<number | null>(null);
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);

  const [faults, setFaults] = useState<Fault[]>([]);
  const [faultFilter, setFaultFilter] = useState<"all" | "active" | "stored">("all");
  const [faultBusy, setFaultBusy] = useState(false);
  const [lastUpdated, setLastUpdated] = useState("");

  const [resourceBusy, setResourceBusy] = useState(false);
  const [operationError, setOperationError] = useState("");
  const [operationUpdated, setOperationUpdated] = useState("");

  const selectedEcu = useMemo(
    () => ecus.find((item) => item.id === ecuId),
    [ecus, ecuId],
  );

  const resourceList = useMemo(
    () => [
      ...globalResources(),
      ...resourceDescriptors
        .map(mapResource)
        .filter((item): item is Resource => item !== null),
    ],
    [resourceDescriptors],
  );

  const visibleFaults = faults;

  useEffect(() => {
    let alive = true;
    api.ecus()
      .then((items) => {
        if (!alive) return;
        setEcus(items);
        setEcuId((current) =>
          items.some((item) => item.id === current) ? current : (items[0]?.id ?? ""),
        );
      })
      .catch((cause: unknown) => {
        if (alive) setError(cause instanceof Error ? cause.message : "ECU discovery failed");
      });
    return () => {
      alive = false;
    };
  }, []);

  useEffect(() => {
    if (!ecuId) return;
    let alive = true;
    setResourceBusy(true);
    setFaultBusy(true);
    setOperationError("");

    const activeFilter = faultFilter === "all"
      ? undefined
      : faultFilter === "active";

    Promise.all([
  api.resources(ecuId),
  api.operations(ecuId),
  api.faults(ecuId, activeFilter),
])
  .then(async ([resourceItems, operationItems, faultItems]) => {
    const executionGroups = await Promise.all(
      operationItems.map((operation) =>
        api.executions(ecuId, operation.id),
      ),
    );

    const executionItems = executionGroups
      .flat()
      .sort((a, b) => b.created_at_unix_ms - a.created_at_unix_ms);

    if (!alive) return;
    setResourceDescriptors(resourceItems);
    setOperations(operationItems);
    setExecutions(executionItems);
    setFaults(faultItems);
    setLastUpdated(new Date().toLocaleTimeString());
    setOperationUpdated(new Date().toLocaleTimeString());
  })
      .catch((cause: unknown) => {
        if (alive) {
          const message = cause instanceof Error ? cause.message : "ECU resource discovery failed";
          setError(message);
          setOperationError(message);
        }
      })
      .finally(() => {
        if (alive) {
          setResourceBusy(false);
          setFaultBusy(false);
        }
      });

    return () => {
      alive = false;
    };
  }, [ecuId, faultFilter]);

  function saveProfile() {
    const normalized = baseUrl.trim().replace(/\/+$/, "");
    try {
      const parsed = new URL(normalized);
      if (parsed.protocol !== "http:" && parsed.protocol !== "https:") {
        throw new Error("Only HTTP and HTTPS server URLs are supported.");
      }
      localStorage.setItem("sovd.baseUrl", normalized);
      localStorage.setItem("sovd.profileName", profileName.trim() || "SOVD connection");
      setBaseUrl(normalized);
      setError("");
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "Invalid connection URL");
    }
  }

  function selectResource(resource: Resource) {
    setMethod(resource.method);
    setPath(resource.path);
    setBody(resource.body ?? "");
    setResponse("");
    setStatus(null);
    setElapsed(null);
    setError("");
  }

  async function sendRequest() {
    setError("");
    setResponse("");
    setStatus(null);
    setElapsed(null);

    const normalizedBase = baseUrl.trim().replace(/\/+$/, "");
    let target: URL;

    try {
      target = new URL(path.trim(), normalizedBase);
      const base = new URL(normalizedBase);
      if (target.origin !== base.origin || !target.pathname.startsWith("/api/")) {
        throw new Error("Requests are restricted to this server's /api/ endpoints.");
      }
      if (method !== "GET" && method !== "POST") {
        throw new Error("This backend currently supports GET and POST in this workbench.");
      }
      if (method === "POST" && body.trim()) JSON.parse(body);
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "Invalid request configuration");
      return;
    }

    const headers: Record<string, string> = { Accept: "application/json" };
    if (authEnabled && bearerToken.trim()) {
      headers.Authorization = `Bearer ${bearerToken.trim()}`;
    }
    if (method === "POST" && body.trim()) {
      headers["Content-Type"] = "application/json";
    }

    const controller = new AbortController();
    const timeout = window.setTimeout(() => controller.abort(), 10000);
    const started = performance.now();
    setBusy(true);

    try {
      const result = await fetch(target.toString(), {
        method,
        headers,
        body: method === "POST" && body.trim() ? body : undefined,
        credentials: "same-origin",
        signal: controller.signal,
      });
      const raw = await result.text();
      let formatted = raw;
      try {
        formatted = pretty(raw ? JSON.parse(raw) : null);
      } catch {
        // Preserve non-JSON response text for diagnostics.
      }
      setStatus(result.status);
      setResponse(formatted || "(empty response)");
      setElapsed(Math.round(performance.now() - started));
      if (!result.ok) setError(`HTTP ${result.status} ${result.statusText}`);
    } catch (cause) {
      setElapsed(Math.round(performance.now() - started));
      setError(
        cause instanceof Error && cause.name === "AbortError"
          ? "Request timed out after 10 seconds."
          : cause instanceof Error
            ? cause.message
            : "Request failed",
      );
    } finally {
      window.clearTimeout(timeout);
      setBusy(false);
    }
  }

  async function refreshFaults() {
    if (!ecuId) return;
    setFaultBusy(true);
    try {
      const activeFilter = faultFilter === "all"
        ? undefined
        : faultFilter === "active";
      setFaults(await api.faults(ecuId, activeFilter));
      setLastUpdated(new Date().toLocaleTimeString());
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "Fault refresh failed");
    } finally {
      setFaultBusy(false);
    }
  }

  async function refreshOperations() {
    if (!ecuId) return;
    setOperationError("");
    try {
      const items = await api.operations(ecuId);

      const executionGroups = await Promise.all(
        items.map((operation) =>
          api.executions(ecuId, operation.id),
        ),
      );

      const history = executionGroups
        .flat()
        .sort((a, b) => b.created_at_unix_ms - a.created_at_unix_ms);

      setOperations(items);
      setExecutions(history);
      setOperationUpdated(new Date().toLocaleTimeString());
    } catch (cause) {
      setOperationError(cause instanceof Error ? cause.message : "Operation refresh failed");
    }
  }

  async function executeOperation(operation: OperationDescriptor) {
    if (!operation.supports_execution || operation.method !== "POST") return;
    if (!window.confirm(
      `Execute "${operation.name}" on ${ecuId}? This operation may modify ECU diagnostic state.`,
    )) return;

    setOperationBusy(operation.id);
    setOperationError("");
    try {
      const result = await api.executeOperation(operation.execution_path);
      setExecutions((current) => [
        result,
        ...current.filter((item) => item.execution_id !== result.execution_id),
      ]);
      await refreshOperations();
      await refreshFaults();
      setOperationUpdated(new Date().toLocaleTimeString());
    } catch (cause) {
      setOperationError(cause instanceof Error ? cause.message : "Operation execution failed");
    } finally {
      setOperationBusy("");
    }
  }

  function formatTimestamp(timestamp: number): string {
    const date = new Date(timestamp);
    return Number.isNaN(date.getTime()) ? "Unknown time" : date.toLocaleString();
  }

  return (
    <div className="space-y-5">
      <section className="panel p-5">
        <div className="flex flex-wrap items-start justify-between gap-4">
          <div>
            <div className="eyebrow">SERVER CONNECTION</div>
            <h2 className="mt-1 text-xl font-bold">SOVD Explorer</h2>
            <p className="mt-1 text-sm text-slate-400">
              Discover ECU resources, inspect diagnostics and execute supported operations.
            </p>
          </div>
          <span className="flex items-center gap-2 rounded-full border border-emerald-500/20 bg-emerald-500/5 px-3 py-2 text-xs text-emerald-300">
            <Wifi size={14} /> {profileName}
          </span>
        </div>

        <div className="mt-5 grid gap-3 md:grid-cols-3">
          <label className="text-xs text-slate-400">
            CONNECTION NAME
            <input
              value={profileName}
              onChange={(event) => setProfileName(event.target.value)}
              className="mt-2 w-full rounded-lg border border-slate-700 bg-slate-950 px-3 py-2 text-sm text-slate-100"
            />
          </label>
          <label className="text-xs text-slate-400 md:col-span-2">
            SOVD SERVER BASE URL
            <input
              value={baseUrl}
              onChange={(event) => setBaseUrl(event.target.value)}
              placeholder="http://127.0.0.1:5173"
              className="mt-2 w-full rounded-lg border border-slate-700 bg-slate-950 px-3 py-2 font-mono text-sm text-slate-100"
            />
          </label>
        </div>

        <div className="mt-4 flex flex-wrap items-center gap-3">
          <label className="flex items-center gap-2 text-sm text-slate-300">
            <input
              type="checkbox"
              checked={authEnabled}
              onChange={(event) => setAuthEnabled(event.target.checked)}
            />
            Add bearer token
          </label>
          {authEnabled && (
            <input
              type="password"
              autoComplete="off"
              value={bearerToken}
              onChange={(event) => setBearerToken(event.target.value)}
              placeholder="Bearer token (kept in memory only)"
              className="min-w-0 flex-1 rounded-lg border border-slate-700 bg-slate-950 px-3 py-2 text-sm"
            />
          )}
          <button
            onClick={saveProfile}
            className="rounded-lg border border-cyan-500/30 bg-cyan-500/10 px-4 py-2 text-sm text-cyan-200"
          >
            Save profile
          </button>
        </div>
        <p className="mt-3 text-xs leading-5 text-slate-500">
          The Vite development proxy targets 127.0.0.1:18010. A different server origin
          must permit browser CORS. The current Rust API does not enforce bearer authentication;
          the token field only sends the header.
        </p>
      </section>

      <div className="grid gap-5 xl:grid-cols-[280px_minmax(0,1fr)]">
        <aside className="panel p-4">
          <div className="eyebrow">RESOURCE EXPLORER</div>
          <div className="mt-3 flex items-center gap-2">
            <Server size={16} className="text-cyan-300" />
            <span className="font-semibold">Vehicle components</span>
          </div>
          <label className="mt-4 block text-xs text-slate-400">
            SELECT ECU
            <select
              value={ecuId}
              onChange={(event) => setEcuId(event.target.value)}
              className="mt-2 w-full rounded-lg border border-slate-700 bg-slate-950 p-2.5 text-sm"
            >
              {ecus.map((item) => (
                <option key={item.id} value={item.id}>{item.id} — {item.name}</option>
              ))}
            </select>
          </label>

          {selectedEcu && (
            <div className="mt-3 rounded-lg border border-slate-700 bg-slate-950/70 p-3 text-xs">
              <div className="font-semibold text-slate-200">{selectedEcu.name}</div>
              <div className="mt-1 text-slate-500">{selectedEcu.domain}</div>
              <div className="mt-2 flex items-center gap-2 text-emerald-300">
                <CircleDot size={11} /> {selectedEcu.status}
              </div>
            </div>
          )}

          <div className="mb-2 mt-5 flex items-center justify-between gap-2">
            <span className="text-xs font-semibold uppercase tracking-wider text-slate-500">
              Discovered resources
            </span>
            <button
              onClick={() => {
                if (ecuId) {
                  setResourceBusy(true);
                  api.resources(ecuId)
                    .then(setResourceDescriptors)
                    .catch((cause: unknown) =>
                      setError(cause instanceof Error ? cause.message : "Resource refresh failed"),
                    )
                    .finally(() => setResourceBusy(false));
                }
              }}
              disabled={resourceBusy || !ecuId}
              title="Refresh resources"
              className="text-slate-400 hover:text-cyan-300 disabled:opacity-50"
            >
              <RefreshCw size={13} className={resourceBusy ? "animate-spin" : ""} />
            </button>
          </div>
          <div className="space-y-1">
            {resourceList.map((resource) => (
              <button
                key={resource.id}
                onClick={() => selectResource(resource)}
                className={`w-full rounded-lg p-3 text-left transition hover:bg-slate-800 ${
                  path === resource.path ? "bg-cyan-500/10 ring-1 ring-cyan-500/30" : ""
                }`}
              >
                <div className="flex items-center justify-between gap-2">
                  <span className="text-sm font-medium capitalize text-slate-200">{resource.label}</span>
                  <ChevronRight size={14} className="text-slate-500" />
                </div>
                <div className="mt-1 text-xs leading-4 text-slate-500">{resource.description}</div>
                <div className="mt-2 font-mono text-[10px] text-cyan-300">
                  {resource.method} {resource.path}
                </div>
                {resource.simulated && (
                  <div className="mt-1 text-[10px] text-amber-300">Simulated resource</div>
                )}
              </button>
            ))}
            {!resourceBusy && resourceList.length === globalResources().length && (
              <p className="px-2 py-3 text-xs text-slate-500">
                No ECU resources were returned by the server.
              </p>
            )}
          </div>
        </aside>

        <section className="space-y-5">
          <div className="panel p-5">
            <div className="flex items-center gap-2">
              <Terminal size={17} className="text-cyan-300" />
              <div>
                <div className="eyebrow">REQUEST WORKBENCH</div>
                <h3 className="font-semibold">Compose and send request</h3>
              </div>
            </div>

            <div className="mt-4 grid gap-3 sm:grid-cols-[110px_minmax(0,1fr)]">
              <label className="text-xs text-slate-400">
                METHOD
                <select
                  value={method}
                  onChange={(event) => setMethod(event.target.value as HttpMethod)}
                  className="mt-2 w-full rounded-lg border border-slate-700 bg-slate-950 p-3 font-mono text-sm"
                >
                  <option value="GET">GET</option>
                  <option value="POST">POST</option>
                </select>
              </label>
              <label className="min-w-0 text-xs text-slate-400">
                REQUEST PATH
                <input
                  value={path}
                  onChange={(event) => setPath(event.target.value)}
                  spellCheck={false}
                  className="mt-2 w-full rounded-lg border border-slate-700 bg-slate-950 p-3 font-mono text-sm text-slate-100"
                  placeholder="/api/ecus"
                />
              </label>
            </div>

            {method === "POST" && (
              <label className="mt-4 block text-xs text-slate-400">
                JSON PAYLOAD
                <textarea
                  value={body}
                  onChange={(event) => setBody(event.target.value)}
                  rows={6}
                  spellCheck={false}
                  className="mt-2 w-full resize-y rounded-lg border border-slate-700 bg-slate-950 p-3 font-mono text-xs text-slate-100"
                  placeholder={'{\n  "payload_hex": "22 F1 90"\n}'}
                />
              </label>
            )}

            <div className="mt-4 flex flex-wrap items-center justify-between gap-3">
              <span className="text-xs text-slate-500">
                Requests restricted to same-origin /api/ endpoints · 10 s timeout
              </span>
              <button
                disabled={busy || !path.trim()}
                onClick={() => void sendRequest()}
                className="flex items-center gap-2 rounded-lg bg-cyan-500 px-4 py-2.5 text-sm font-semibold text-slate-950 hover:bg-cyan-400 disabled:opacity-50"
              >
                {busy ? <RefreshCw size={15} className="animate-spin" /> : <Send size={15} />}
                {busy ? "Sending request..." : "Send request"}
              </button>
            </div>
          </div>

          <div className="panel min-h-64 overflow-hidden">
            <div className="flex flex-wrap items-center justify-between gap-2 border-b border-slate-700 px-4 py-3">
              <div className="flex items-center gap-2">
                <FileJson size={16} className="text-cyan-300" />
                <span className="text-sm font-semibold">Response viewer</span>
              </div>
              <div className="flex items-center gap-3 text-xs">
                {status !== null && (
                  <span className={status >= 200 && status < 300 ? "text-emerald-300" : "text-rose-300"}>
                    HTTP {status}
                  </span>
                )}
                {elapsed !== null && <span className="text-slate-500">{elapsed} ms</span>}
              </div>
            </div>
            {error && (
              <div role="alert" className="m-4 rounded-lg border border-rose-500/20 bg-rose-500/5 p-3 text-sm text-rose-300">
                {error}
              </div>
            )}
            <pre className="max-h-[520px] min-h-52 overflow-auto whitespace-pre-wrap break-words p-4 font-mono text-xs leading-5 text-slate-200">
              {response || "Select a resource or enter an API path, then send a request."}
            </pre>
          </div>
        </section>
      </div>

      <section className="panel p-5">
        <div className="flex flex-wrap items-center justify-between gap-3">
          <div>
            <div className="eyebrow">FAULT COLLECTION</div>
            <h3 className="mt-1 text-lg font-semibold">{ecuId || "No ECU selected"} · {visibleFaults.length} shown</h3>
            <p className="mt-1 text-xs text-slate-500">
              Server-side filter · Last refreshed: {lastUpdated || "not yet loaded"}
            </p>
          </div>
          <button
            onClick={() => void refreshFaults()}
            disabled={faultBusy || !ecuId}
            className="flex items-center gap-2 rounded-lg border border-slate-700 px-3 py-2 text-sm text-slate-300"
          >
            <RefreshCw size={14} className={faultBusy ? "animate-spin" : ""} />
            Refresh faults
          </button>
        </div>

        <div className="mt-4 flex flex-wrap items-center gap-2">
          <Filter size={14} className="text-slate-500" />
          {(["all", "active", "stored"] as const).map((filter) => (
            <button
              key={filter}
              onClick={() => setFaultFilter(filter)}
              className={`rounded-full px-3 py-1.5 text-xs capitalize ${
                faultFilter === filter
                  ? "bg-cyan-500/15 text-cyan-200 ring-1 ring-cyan-500/30"
                  : "bg-slate-800 text-slate-400"
              }`}
            >
              {filter}
            </button>
          ))}
        </div>

        <div className="mt-4 space-y-2">
          {visibleFaults.length === 0 ? (
            <div className="rounded-lg border border-slate-700 bg-slate-950/50 p-5 text-center text-sm text-slate-500">
              <CheckCircle2 size={20} className="mx-auto mb-2 text-emerald-400" />
              No faults match this filter.
            </div>
          ) : visibleFaults.map((fault) => (
            <div key={fault.code} className="flex flex-wrap items-center justify-between gap-3 rounded-lg border border-slate-700 bg-slate-950/50 p-3">
              <div>
                <div className="font-mono text-sm font-semibold">{fault.code}</div>
                <div className="mt-1 text-xs text-slate-400">{fault.description}</div>
              </div>
              <span className={`flex items-center gap-1 text-xs ${fault.active ? "text-amber-300" : "text-slate-500"}`}>
                <AlertTriangle size={13} />
                {fault.active ? "Active" : "Stored"}
              </span>
            </div>
          ))}
        </div>
      </section>

      <section className="panel p-5">
        <div className="flex flex-wrap items-center justify-between gap-3">
          <div>
            <div className="eyebrow">OPERATION MANAGEMENT</div>
            <h3 className="mt-1 text-lg font-semibold">{ecuId || "No ECU selected"} · {operations.length} operations</h3>
            <p className="mt-1 text-xs text-slate-500">
              Execution history refreshed: {operationUpdated || "not yet loaded"}
            </p>
          </div>
          <button
            onClick={() => void refreshOperations()}
            disabled={!ecuId || Boolean(operationBusy)}
            className="flex items-center gap-2 rounded-lg border border-slate-700 px-3 py-2 text-sm text-slate-300"
          >
            <RefreshCw size={14} />
            Refresh operations
          </button>
        </div>

        {operationError && (
          <div role="alert" className="mt-4 rounded-lg border border-rose-500/20 bg-rose-500/5 p-3 text-sm text-rose-300">
            {operationError}
          </div>
        )}

        <div className="mt-4 space-y-2">
          {operations.length === 0 ? (
            <p className="rounded-lg border border-slate-700 bg-slate-950/50 p-4 text-sm text-slate-500">
              No operations were advertised by this ECU.
            </p>
          ) : operations.map((operation) => (
            <div key={operation.id} className="flex flex-wrap items-center justify-between gap-3 rounded-lg border border-slate-700 bg-slate-950/50 p-4">
              <div className="min-w-0">
                <div className="font-semibold text-slate-200">{operation.name}</div>
                <p className="mt-1 text-xs text-slate-400">{operation.description}</p>
                <p className="mt-2 break-all font-mono text-[10px] text-slate-500">
                  {operation.method} {operation.execution_path}
                </p>
              </div>
              <button
                onClick={() => void executeOperation(operation)}
                disabled={!operation.supports_execution || operation.method !== "POST" || Boolean(operationBusy)}
                className="flex items-center gap-2 rounded-lg bg-cyan-500 px-3 py-2 text-sm font-semibold text-slate-950 disabled:opacity-50"
              >
                {operationBusy === operation.id
                  ? <RefreshCw size={14} className="animate-spin" />
                  : <Play size={14} />}
                {operationBusy === operation.id ? "Executing..." : "Execute"}
              </button>
            </div>
          ))}
        </div>

        <div className="mb-2 mt-6 flex items-center gap-2">
          <Clock3 size={15} className="text-cyan-300" />
          <h4 className="text-sm font-semibold">Execution history</h4>
          <span className="text-xs text-slate-500">({executions.length})</span>
        </div>
        <div className="space-y-2">
          {executions.length === 0 ? (
            <p className="rounded-lg border border-slate-700 bg-slate-950/50 p-4 text-sm text-slate-500">
              No operation executions recorded for this ECU.
            </p>
          ) : executions.map((execution) => (
            <div key={execution.execution_id} className="rounded-lg border border-slate-700 bg-slate-950/50 p-3">
              <div className="flex flex-wrap items-center justify-between gap-2">
                <span className="break-all font-mono text-xs text-cyan-300">{execution.execution_id}</span>
                <span className={`text-xs ${execution.success ? "text-emerald-300" : "text-rose-300"}`}>
                  {execution.status} · {execution.success ? "success" : "failed"}
                </span>
              </div>
              <div className="mt-2 text-sm text-slate-200">{execution.message}</div>
              <div className="mt-1 text-xs text-slate-500">
                {execution.operation} · {formatTimestamp(execution.created_at_unix_ms)}
              </div>
            </div>
          ))}
        </div>
      </section>

      <div className="flex items-start gap-2 rounded-lg border border-amber-500/20 bg-amber-500/5 p-3 text-xs leading-5 text-amber-200/80">
        <Shield size={15} className="mt-0.5 shrink-0" />
        Development diagnostic tool. Resource and operation availability comes from the Rust
        backend. The current implementation uses simulated ECU state and process-local
        operation history; it does not provide generic SOVD PUT resources, OAuth, or
        asynchronous cancellation.
      </div>
    </div>
  );
}
