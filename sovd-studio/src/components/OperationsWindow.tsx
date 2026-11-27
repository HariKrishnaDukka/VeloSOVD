// M3: Operations window. Lists the operations of one ECU with what each declares it can do,
// starts them (Technician and up) and shows the execution history. Every rule (role,
// paths, capabilities, status parsing) lives in studio-core and is unit-tested there; this
// window only draws and calls the Rust commands.
//
// In C terms: the display layer. It never builds a URL, never decides a permission.
import { useCallback, useEffect, useRef, useState } from "react";
import type { IDockviewPanelProps } from "dockview-react";
import { Lock, RefreshCw } from "lucide-react";
import { z } from "zod";
import { api } from "../services/api";
import { studio, type OperationInfo } from "../services/studio";
import { describe, inTauri, notifySessionLost } from "../services/transport";
import type { Ecu } from "../types";

const ExecutionSchema = z.object({
  execution_id: z.string(),
  status: z.string(),
  message: z.string(),
  created_at_unix_ms: z.number(),
});
type Execution = z.infer<typeof ExecutionSchema>;

const CAPABILITIES = ["execute", "status", "stop", "freeze", "reset"] as const;

function StateCell({ status }: { status: string }) {
  if (status === "running")
    return (
      <span className="flex items-center gap-2 font-semibold text-amber-300">
        <span className="h-3 w-3 rounded-full border-2 border-amber-300 border-t-transparent" />
        Running
      </span>
    );
  if (status === "completed")
    return (
      <span className="flex items-center gap-2 font-semibold text-emerald-300">
        <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="3" aria-hidden="true">
          <path d="M4 12l5 5L20 6" />
        </svg>
        Completed
      </span>
    );
  if (status === "failed")
    return (
      <span className="flex items-center gap-2 font-semibold text-rose-300">
        <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="3" aria-hidden="true">
          <path d="M5 5l14 14M19 5L5 19" />
        </svg>
        Failed
      </span>
    );
  return <span className="text-slate-400">{status}</span>;
}

export default function OperationsWindow({ api: panelApi }: IDockviewPanelProps) {
  const [ecus, setEcus] = useState<Ecu[]>([]);
  const [ecuId, setEcuId] = useState("");
  const [ops, setOps] = useState<OperationInfo[]>([]);
  const [opId, setOpId] = useState("");
  const [history, setHistory] = useState<Execution[]>([]);
  const [mayChange, setMayChange] = useState(false);
  const [user, setUser] = useState("");
  const [pollMs, setPollMs] = useState(5000);
  const [visible, setVisible] = useState(panelApi.isVisible);
  const [updated, setUpdated] = useState("");
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  const [confirmStart, setConfirmStart] = useState(false);
  const [runningId, setRunningId] = useState<string | null>(null);
  const running = useRef<string | null>(null);

  const track = (id: string | null) => {
    running.current = id;
    setRunningId(id);
  };

  // Poll only while this window is the visible tab.
  useEffect(() => {
    const sub = panelApi.onDidVisibilityChange((e) => setVisible(e.isVisible));
    return () => sub.dispose();
  }, [panelApi]);

  useEffect(() => {
    if (!inTauri()) return;
    studio.pollIntervalMs().then(setPollMs).catch(() => undefined);
    studio.session().then((s) => setUser(s?.user ?? "")).catch(() => undefined);
    api
      .ecus()
      .then((list) => {
        setEcus(list);
        setEcuId((old) => old || (list.find((e) => e.id === "ECM") ?? list[0])?.id || "");
      })
      .catch((e) => setError(describe(e)));
  }, []);

  const loadOps = useCallback(async () => {
    if (!ecuId) return;
    try {
      const list = await studio.operationList(ecuId);
      setOps(list);
      setOpId((old) => (list.some((o) => o.id === old) ? old : (list[0]?.id ?? "")));
    } catch (e) {
      if (!notifySessionLost(e)) setError(describe(e));
    }
  }, [ecuId]);

  useEffect(() => {
    if (inTauri() && visible) void loadOps();
  }, [loadOps, visible]);

  // One tick: the history and, while an execution started here is running, its status.
  const tick = useCallback(async () => {
    if (!ecuId || !opId) return;
    try {
      const res = await studio.operationHistory(ecuId, opId);
      if (res.status < 200 || res.status >= 300) throw new Error(`central answered HTTP ${res.status}`);
      const list = z.array(ExecutionSchema).parse(JSON.parse(res.body));
      setHistory([...list].sort((a, b) => b.created_at_unix_ms - a.created_at_unix_ms));
      if (running.current) {
        const state = await studio.operationStatus(ecuId, opId, running.current);
        if (state !== "running") {
          running.current = null;
          setRunningId(null);
        }
      }
      setMayChange(await studio.canChange());
      setUpdated(new Date().toLocaleTimeString());
      setError("");
    } catch (e) {
      if (!notifySessionLost(e)) setError(describe(e));
    }
  }, [ecuId, opId]);

  useEffect(() => {
    if (!inTauri() || !visible || !ecuId || !opId) return;
    void tick();
    const timer = window.setInterval(() => void tick(), pollMs);
    return () => window.clearInterval(timer);
  }, [tick, visible, pollMs, ecuId, opId]);

  async function startOperation() {
    setBusy(true);
    try {
      track(await studio.operationStart(ecuId, opId));
      setError("");
      await tick();
    } catch (e) {
      if (!notifySessionLost(e)) setError(describe(e));
    } finally {
      setConfirmStart(false);
      setBusy(false);
    }
  }

  async function stopExecution(id: string) {
    setBusy(true);
    try {
      await studio.operationStop(ecuId, opId, id);
      await tick();
    } catch (e) {
      if (!notifySessionLost(e)) setError(describe(e));
    } finally {
      setBusy(false);
    }
  }

  if (!inTauri()) {
    return <div className="h-full bg-[#080d18] p-4 text-sm text-slate-400">The Operations window is available in the desktop app.</div>;
  }

  const op = ops.find((o) => o.id === opId) ?? null;
  const caps = op?.capabilities ?? null;
  const startBlocked = busy || runningId !== null || !op?.supports_execution || caps?.execute === false;
  const canStop = mayChange && caps?.stop === true;
  const cols = "grid grid-cols-[110px_150px_120px_minmax(0,1fr)_64px] items-center gap-3";

  return (
    <div className="relative h-full bg-[#080d18] text-[13px] text-slate-100">
      <div className="flex h-full gap-4 overflow-auto p-5">
        <aside className="flex w-[290px] shrink-0 flex-col gap-3.5 self-start rounded-xl border border-slate-800 bg-[#0b1220] p-4">
          <label className="flex flex-col gap-1 text-[11px] text-slate-400">
            ECU
            <select
              value={ecuId}
              onChange={(e) => {
                setEcuId(e.target.value);
                setOps([]);
                setOpId("");
                setHistory([]);
                track(null);
              }}
              className="h-9 rounded-lg border border-slate-700 bg-slate-900 px-2.5 text-slate-100"
            >
              {ecus.map((e) => (
                <option key={e.id} value={e.id}>
                  {e.id} — {e.name}
                </option>
              ))}
            </select>
          </label>
          <div className="eyebrow">OPERATIONS · {ops.length}</div>
          {ops.map((o) => (
            <button
              key={o.id}
              onClick={() => {
                setOpId(o.id);
                setHistory([]);
                track(null);
              }}
              className={`rounded-lg border px-3 py-2.5 text-left ${o.id === opId ? "border-cyan-400 bg-[#12233a]" : "border-slate-800"}`}
            >
              <div className="font-semibold">{o.id}</div>
              <div className="mt-0.5 text-xs text-slate-300">{o.description}</div>
            </button>
          ))}
          <div className="text-xs text-slate-400">More operations appear here when an ECU declares them.</div>
        </aside>

        <section className="flex min-w-0 flex-1 flex-col gap-4">
          {error && <div className="rounded-lg border border-rose-500/40 bg-rose-950/40 px-3 py-2 text-rose-200">{error}</div>}
          {op ? (
            <div className="flex flex-col gap-3.5 rounded-xl border border-slate-800 bg-[#0b1220] p-5">
              <div className="flex items-start justify-between gap-4">
                <div>
                  <div className="eyebrow">OPERATION</div>
                  <h1 className="mb-0.5 mt-1 font-mono text-2xl font-bold">{op.id}</h1>
                  <div className="text-slate-300">{op.description}</div>
                </div>
                <button
                  onClick={() => {
                    void loadOps();
                    void tick();
                  }}
                  aria-label="Refresh"
                  className="flex h-9 items-center gap-1.5 rounded-lg border border-slate-700 bg-slate-900 px-3.5"
                >
                  <RefreshCw size={14} /> Refresh
                </button>
              </div>
              <div className="flex flex-wrap items-center gap-1.5">
                <span className="mr-1.5 text-xs text-slate-400">Declared by the operation</span>
                {caps ? (
                  CAPABILITIES.map((c) => (
                    <span
                      key={c}
                      className={`rounded-md border bg-slate-900 px-2 py-0.5 text-xs ${caps[c] ? "border-cyan-400 text-slate-100" : "border-dashed border-slate-600 text-slate-400"}`}
                    >
                      {c} · {caps[c] ? "yes" : "no"}
                    </span>
                  ))
                ) : (
                  <span className="text-xs text-slate-400">not published by this central</span>
                )}
              </div>
              {mayChange ? (
                <div className="flex items-center gap-2.5">
                  <button
                    disabled={startBlocked}
                    onClick={() => setConfirmStart(true)}
                    className="h-[38px] rounded-lg bg-cyan-500 px-[18px] font-semibold text-slate-950 disabled:opacity-40"
                  >
                    Start operation
                  </button>
                  <span className="flex items-center gap-1.5 rounded-full border border-slate-700 px-2.5 py-0.5 text-[11px] text-slate-400">
                    <Lock size={11} aria-hidden="true" /> Technician and up
                  </span>
                </div>
              ) : (
                <div className="flex max-w-[520px] items-center gap-2 self-start rounded-lg border border-dashed border-slate-600 px-3 py-2 text-xs text-slate-300">
                  <Lock size={14} aria-hidden="true" className="shrink-0" />
                  <span>Read-only for your role. Starting and stopping operations needs Technician or higher.</span>
                </div>
              )}
            </div>
          ) : (
            <div className="rounded-xl border border-slate-800 bg-[#0b1220] p-5 text-slate-400">No operation selected.</div>
          )}

          <div className="overflow-hidden rounded-xl border border-slate-800 bg-[#0b1220]">
            <div className="flex items-center justify-between border-b border-slate-800 px-4 py-3">
              <span className="eyebrow">EXECUTION HISTORY</span>
              <span className="text-xs text-slate-400">
                Read from central at {updated || "—"} · refreshes every {pollMs / 1000} s while this window is visible
              </span>
            </div>
            <div className={`${cols} border-b border-slate-800 px-4 py-2.5 text-[11px] tracking-[0.08em] text-slate-400`}>
              <span>ID</span>
              <span>STATE</span>
              <span>STARTED</span>
              <span>MESSAGE</span>
              <span />
            </div>
            {history.length === 0 && <div className="px-4 py-6 text-slate-400">No executions yet.</div>}
            {history.map((h) => (
              <div
                key={h.execution_id}
                className={`${cols} border-b border-slate-800 px-4 py-3 last:border-b-0 ${h.execution_id === runningId ? "bg-[#12233a]" : ""}`}
              >
                <span className="truncate font-mono" title={h.execution_id}>
                  {h.execution_id}
                </span>
                <StateCell status={h.status} />
                <span>{new Date(h.created_at_unix_ms).toLocaleTimeString()}</span>
                <span className="break-words text-slate-300">{h.message || "—"}</span>
                {canStop && h.status === "running" ? (
                  <button
                    disabled={busy}
                    onClick={() => void stopExecution(h.execution_id)}
                    className="h-[30px] rounded-md border border-rose-400 text-rose-300 disabled:opacity-40"
                  >
                    Stop
                  </button>
                ) : (
                  <span />
                )}
              </div>
            ))}
          </div>
        </section>
      </div>

      {confirmStart && op && (
        <div className="absolute inset-0 z-10 flex items-center justify-center bg-slate-950/80 p-4">
          <div
            role="dialog"
            aria-modal="true"
            aria-labelledby="start-title"
            className="flex w-full max-w-[500px] flex-col gap-4 rounded-2xl border border-slate-700 bg-slate-900 p-6 shadow-2xl"
          >
            <h2 id="start-title" className="text-xl font-bold">
              Start {op.id} on {ecuId}?
            </h2>
            <p className="text-slate-300">
              {op.description}. It runs on the ECU now and Studio cannot undo it. The start is written to the audit log under your
              user name ({user || "—"}).
            </p>
            <div className="flex justify-end gap-2.5">
              <button onClick={() => setConfirmStart(false)} className="h-10 rounded-lg border border-slate-700 px-4">
                Cancel
              </button>
              <button
                disabled={busy}
                onClick={() => void startOperation()}
                className="h-10 rounded-lg bg-cyan-500 px-4 font-semibold text-slate-950 disabled:opacity-40"
              >
                Start operation
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
