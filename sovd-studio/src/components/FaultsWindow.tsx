// M3: Faults window. Lists the faults of one ECU with the decoded DTC status bits and lets
// Technician and above clear them. Every rule (role, confirmation, paths, status decoding)
// lives in studio-core and is unit-tested there; this window only draws and calls Rust.
//
// In C terms: the display layer. It never builds a URL, never decides a permission.
import { useCallback, useEffect, useRef, useState } from "react";
import type { IDockviewPanelProps } from "dockview-react";
import { Lock, RefreshCw } from "lucide-react";
import { z } from "zod";
import { api } from "../services/api";
import { studio, type StatusBit } from "../services/studio";
import { describe, inTauri, notifySessionLost } from "../services/transport";
import { FaultSchema, type Ecu } from "../types";

// Central also sends the raw ISO 14229 status byte; the older FaultSchema ignores it.
const FaultRowSchema = FaultSchema.extend({ status: z.number().int().min(0).max(255) });
type FaultRow = z.infer<typeof FaultRowSchema>;
type Filter = "all" | "active" | "stored";

const hex = (n: number) => `0x${n.toString(16).toUpperCase().padStart(2, "0")}`;
const bin = (n: number) => n.toString(2).padStart(8, "0").replace(/(\d{4})(\d{4})/, "$1 $2");
const plural = (n: number, word: string) => `${n} ${word}${n === 1 ? "" : "s"}`;

export default function FaultsWindow({ api: panelApi }: IDockviewPanelProps) {
  const [ecus, setEcus] = useState<Ecu[]>([]);
  const [ecuId, setEcuId] = useState("");
  const [faults, setFaults] = useState<FaultRow[]>([]);
  const [bits, setBits] = useState<Record<number, StatusBit[]>>({});
  const [filter, setFilter] = useState<Filter>("all");
  const [selected, setSelected] = useState<string | null>(null);
  const [mayChange, setMayChange] = useState(false);
  const [user, setUser] = useState("");
  const [pollMs, setPollMs] = useState(5000);
  const [visible, setVisible] = useState(panelApi.isVisible);
  const [updated, setUpdated] = useState("");
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  const [dialog, setDialog] = useState<{ code: string | null } | null>(null);
  const [typed, setTyped] = useState("");
  const decoded = useRef<Record<number, StatusBit[]>>({});

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

  const refresh = useCallback(async () => {
    if (!ecuId) return;
    try {
      const res = await studio.faultList(ecuId);
      if (res.status < 200 || res.status >= 300) throw new Error(`central answered HTTP ${res.status}`);
      const list = z.array(FaultRowSchema).parse(JSON.parse(res.body));
      const missing = [...new Set(list.map((f) => f.status))].filter((s) => !(s in decoded.current));
      await Promise.all(
        missing.map(async (s) => {
          decoded.current[s] = await studio.decodeStatus(s);
        }),
      );
      setBits({ ...decoded.current });
      setFaults(list);
      setMayChange(await studio.canChange());
      setUpdated(new Date().toLocaleTimeString());
      setError("");
    } catch (e) {
      if (!notifySessionLost(e)) setError(describe(e));
    }
  }, [ecuId]);

  useEffect(() => {
    if (!inTauri() || !visible || !ecuId) return;
    void refresh();
    const timer = window.setInterval(() => void refresh(), pollMs);
    return () => window.clearInterval(timer);
  }, [refresh, visible, pollMs, ecuId]);

  async function confirmClear() {
    if (!dialog) return;
    setBusy(true);
    try {
      const res = await studio.clearFaults(ecuId, dialog.code, typed);
      if (res.status < 200 || res.status >= 300) throw new Error(`central answered HTTP ${res.status}`);
      setError("");
      await refresh(); // read again from central: no optimistic update
    } catch (e) {
      if (!notifySessionLost(e)) setError(describe(e));
    } finally {
      setDialog(null);
      setTyped("");
      setBusy(false);
    }
  }

  if (!inTauri()) {
    return <div className="h-full bg-[#080d18] p-4 text-sm text-slate-400">The Faults window is available in the desktop app.</div>;
  }

  const activeCount = faults.filter((f) => f.active).length;
  const shown =
    filter === "active" ? faults.filter((f) => f.active) : filter === "stored" ? faults.filter((f) => !f.active) : faults;
  const current = faults.find((f) => f.code === selected) ?? faults[0] ?? null;
  const setBitsOf = (f: FaultRow) => (bits[f.status] ?? []).filter((b) => b.set);
  const cols = "grid grid-cols-[72px_minmax(0,1fr)_84px_56px_minmax(0,1.3fr)_64px] items-center gap-3";
  const segment = (id: Filter, label: string) => (
    <button
      key={id}
      onClick={() => setFilter(id)}
      className={`h-[34px] px-3.5 text-sm ${filter === id ? "bg-cyan-900 font-semibold text-slate-100" : "text-slate-400"}`}
    >
      {label}
    </button>
  );

  return (
    <div className="relative h-full bg-[#080d18] text-[13px] text-slate-100">
      <div className="flex h-full gap-4 overflow-auto p-5">
        <section className="flex min-w-0 flex-1 flex-col gap-4">
          <div className="flex items-end justify-between gap-4">
            <div>
              <div className="eyebrow">FAULT COLLECTION</div>
              <h1 className="mb-0.5 mt-1 text-2xl font-bold">
                {ecuId || "—"} · {plural(faults.length, "fault")}
              </h1>
              <div className="text-xs text-slate-400">
                Read from central at {updated || "—"} · refreshes every {pollMs / 1000} s while this window is visible
              </div>
            </div>
            <div className="flex items-end gap-2.5">
              <label className="flex flex-col gap-1 text-[11px] text-slate-400">
                ECU
                <select
                  value={ecuId}
                  onChange={(e) => {
                    setEcuId(e.target.value);
                    setSelected(null);
                    setFaults([]);
                  }}
                  className="h-9 min-w-[230px] rounded-lg border border-slate-700 bg-slate-900 px-2.5 text-slate-100"
                >
                  {ecus.map((e) => (
                    <option key={e.id} value={e.id}>
                      {e.id} — {e.name}
                    </option>
                  ))}
                </select>
              </label>
              <button
                onClick={() => void refresh()}
                aria-label="Refresh"
                className="flex h-9 items-center gap-1.5 rounded-lg border border-slate-700 bg-slate-900 px-3.5"
              >
                <RefreshCw size={14} /> Refresh
              </button>
            </div>
          </div>

          {error && <div className="rounded-lg border border-rose-500/40 bg-rose-950/40 px-3 py-2 text-rose-200">{error}</div>}

          <div className="flex items-center justify-between gap-4">
            <div className="flex overflow-hidden rounded-lg border border-slate-700">
              {segment("all", `All · ${faults.length}`)}
              {segment("active", `Active · ${activeCount}`)}
              {segment("stored", `Stored · ${faults.length - activeCount}`)}
            </div>
            {mayChange ? (
              <div className="flex items-center gap-2.5">
                <span className="flex items-center gap-1.5 rounded-full border border-slate-700 px-2.5 py-0.5 text-[11px] text-slate-400">
                  <Lock size={11} aria-hidden="true" /> Technician and up
                </span>
                <button
                  disabled={faults.length === 0 || busy}
                  onClick={() => setDialog({ code: null })}
                  className="h-9 rounded-lg border border-rose-400 px-3.5 font-medium text-rose-300 disabled:opacity-40"
                >
                  Clear all faults
                </button>
              </div>
            ) : (
              <div className="flex max-w-[430px] items-center gap-2 rounded-lg border border-dashed border-slate-600 px-3 py-2 text-xs text-slate-300">
                <Lock size={14} aria-hidden="true" className="shrink-0" />
                <span>Read-only for your role. Clearing faults needs Technician or higher.</span>
              </div>
            )}
          </div>

          <div className="overflow-hidden rounded-xl border border-slate-800 bg-[#0b1220]">
            <div className={`${cols} border-b border-slate-800 px-4 py-2.5 text-[11px] tracking-[0.08em] text-slate-400`}>
              <span>CODE</span>
              <span>DESCRIPTION</span>
              <span>STATE</span>
              <span>STATUS</span>
              <span>DECODED BITS</span>
              <span />
            </div>
            {shown.length === 0 && <div className="px-4 py-6 text-slate-400">No faults match this filter.</div>}
            {shown.map((f) => (
              <div
                key={f.code}
                onClick={() => setSelected(f.code)}
                className={`${cols} cursor-pointer border-b border-slate-800 px-4 py-3 ${current?.code === f.code ? "bg-[#12233a]" : ""}`}
              >
                <span className="font-mono font-medium">{f.code}</span>
                <span>{f.description}</span>
                <span>{f.active ? <b className="text-amber-300">● Active</b> : <span className="text-slate-400">○ Stored</span>}</span>
                <span className="font-mono">{hex(f.status)}</span>
                <span className="flex flex-wrap gap-1">
                  {setBitsOf(f).map((b) => (
                    <span key={b.bit} className="rounded-md border border-slate-700 bg-slate-900 px-1.5 py-0.5 text-[11px] text-slate-300">
                      {b.name}
                    </span>
                  ))}
                </span>
                {mayChange ? (
                  <button
                    disabled={busy}
                    onClick={(e) => {
                      e.stopPropagation();
                      setDialog({ code: f.code });
                    }}
                    className="h-[30px] rounded-md border border-rose-400 text-rose-300 disabled:opacity-40"
                  >
                    Clear
                  </button>
                ) : (
                  <span />
                )}
              </div>
            ))}
          </div>
        </section>

        <aside className="flex w-[380px] shrink-0 flex-col gap-3.5 self-start rounded-xl border border-slate-800 bg-[#0b1220] p-[18px]">
          {current ? (
            <>
              <div>
                <div className="eyebrow">SELECTED FAULT</div>
                <div className="mt-1 font-mono text-[22px] font-semibold">{current.code}</div>
                <div className="mt-0.5">{current.description}</div>
              </div>
              <div className="flex justify-between text-xs text-slate-400">
                <span>Status byte</span>
                <span className="font-mono text-slate-100">
                  {hex(current.status)} · {bin(current.status)}
                </span>
              </div>
              {mayChange && (
                <button
                  disabled={busy}
                  onClick={() => setDialog({ code: current.code })}
                  className="h-9 rounded-lg border border-rose-400 font-medium text-rose-300 disabled:opacity-40"
                >
                  Clear this fault
                </button>
              )}
              <div className="overflow-hidden rounded-lg border border-slate-800">
                {(bits[current.status] ?? []).map((b) => (
                  <div key={b.bit} className="flex items-center gap-2.5 border-b border-slate-800 px-2.5 py-[7px] text-xs last:border-b-0">
                    <span className={`h-3 w-3 shrink-0 rounded-[3px] border-2 border-cyan-400 ${b.set ? "bg-cyan-400" : ""}`} />
                    <span className="w-[38px] shrink-0 whitespace-nowrap text-slate-400">bit {b.bit}</span>
                    <span className="min-w-0 flex-1 break-words text-[11px]">{b.name}</span>
                    <span className={`w-9 shrink-0 text-right font-semibold ${b.set ? "text-slate-100" : "text-slate-400"}`}>{b.set ? "set" : "clear"}</span>
                  </div>
                ))}
              </div>
              <div className="text-xs text-slate-400">
                {mayChange ? "Every clear" : "Reads"} {mayChange ? "is" : "are"} recorded in the audit log under your user name ({user || "—"}).
              </div>
            </>
          ) : (
            <div className="text-slate-400">No fault selected.</div>
          )}
        </aside>
      </div>

      {dialog && (
        <div className="absolute inset-0 z-10 flex items-center justify-center bg-slate-950/80 p-4">
          <div
            role="dialog"
            aria-modal="true"
            aria-labelledby="clear-title"
            className="flex w-full max-w-[500px] flex-col gap-4 rounded-2xl border border-slate-700 bg-slate-900 p-6 shadow-2xl"
          >
            <h2 id="clear-title" className="text-xl font-bold">
              {dialog.code ? `Clear fault ${dialog.code} on ${ecuId}?` : `Clear all faults on ${ecuId}?`}
            </h2>
            <p className="text-slate-300">
              {dialog.code
                ? `This deletes fault ${dialog.code} from ${ecuId}.`
                : `This deletes ${plural(faults.length, "fault")} from ${ecuId} (${activeCount} active, ${faults.length - activeCount} stored).`}{" "}
              Studio cannot undo it. The clear is written to the audit log under your user name ({user || "—"}).
            </p>
            <label className="flex flex-col gap-1.5 text-xs text-slate-400">
              Type the ECU id to confirm
              <input
                autoFocus
                value={typed}
                onChange={(e) => setTyped(e.target.value)}
                className={`h-10 rounded-lg border bg-[#080d18] px-3 font-mono text-sm text-slate-100 ${typed === ecuId ? "border-emerald-400" : "border-amber-400"}`}
              />
            </label>
            <div className="-mt-2 text-xs text-amber-300">Must match exactly: {ecuId}</div>
            <div className="mt-1 flex justify-end gap-2.5">
              <button
                onClick={() => {
                  setDialog(null);
                  setTyped("");
                }}
                className="h-10 rounded-lg border border-slate-700 px-4"
              >
                Cancel
              </button>
              <button
                disabled={typed !== ecuId || busy}
                onClick={() => void confirmClear()}
                className="h-10 rounded-lg border border-rose-800 bg-rose-900 px-4 font-semibold text-rose-100 disabled:bg-rose-950 disabled:text-slate-400"
              >
                {dialog.code ? "Clear fault" : `Clear ${plural(faults.length, "fault")}`}
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
