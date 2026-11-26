import { useEffect, useRef, useState } from "react";
import { studio, type TraceEntry } from "../services/studio";
import { inTauri } from "../services/transport";

export default function TracePanel() {
  const [entries, setEntries] = useState<TraceEntry[]>([]);
  const [selected, setSelected] = useState<TraceEntry | null>(null);
  const [filter, setFilter] = useState("");
  const last = useRef(0);

  useEffect(() => {
    if (!inTauri()) return;
    const poll = async () => {
      const fresh = await studio.traceSince(last.current).catch(() => []);
      if (fresh.length) {
        last.current = fresh[fresh.length - 1].seq;
        setEntries((old) => [...old, ...fresh].slice(-500));
      }
    };
    void poll();
    const t = window.setInterval(() => void poll(), 1000);
    return () => window.clearInterval(t);
  }, []);

  if (!inTauri()) return <div className="p-4 text-sm text-slate-500">Trace is available in the desktop app.</div>;

  const shown = entries.filter((e) => !filter || `${e.method} ${e.path} ${e.status ?? ""}`.toLowerCase().includes(filter.toLowerCase()));
  const color = (s: number | null) => (s === null ? "text-rose-300" : s >= 400 ? "text-amber-300" : "text-emerald-300");

  return (
    <div className="flex h-full flex-col bg-[#080d18] text-xs text-slate-300">
      <div className="flex items-center gap-2 border-b border-slate-800 p-2">
        <input value={filter} onChange={(e) => setFilter(e.target.value)} placeholder="filter (method, path, status)"
          className="flex-1 rounded border border-slate-700 bg-slate-950 px-2 py-1" />
        <button onClick={() => { void studio.traceClear(); setEntries([]); setSelected(null); }}
          className="rounded border border-slate-700 px-2 py-1 hover:text-slate-100">clear</button>
      </div>
      <div className="grid min-h-0 flex-1 grid-rows-[1fr_auto]">
        <div className="overflow-auto">
          <table className="w-full font-mono">
            <tbody>
              {shown.map((e) => (
                <tr key={e.seq} onClick={() => setSelected(e)}
                  className={`cursor-pointer border-b border-slate-900 hover:bg-slate-800/50 ${selected?.seq === e.seq ? "bg-slate-800" : ""}`}>
                  <td className="px-2 py-1 text-slate-500">{new Date(e.ts_unix_ms).toLocaleTimeString()}</td>
                  <td className="px-2">{e.user ?? "—"}</td>
                  <td className="px-2">{e.method}</td>
                  <td className="px-2">{e.path}</td>
                  <td className={`px-2 ${color(e.status)}`}>{e.status ?? "ERR"}</td>
                  <td className="px-2 text-right text-slate-500">{e.elapsed_ms} ms</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
        {selected && (
          <div className="max-h-[45%] overflow-auto border-t border-slate-800 p-2 font-mono text-[11px]">
            {selected.error && <div className="mb-2 text-rose-300">error: {selected.error}</div>}
            <div className="text-slate-500">request</div>
            <pre className="whitespace-pre-wrap">{selected.request_excerpt ?? "(no body)"}</pre>
            <div className="mt-2 text-slate-500">response</div>
            <pre className="whitespace-pre-wrap">{selected.response_excerpt ?? "(none)"}</pre>
          </div>
        )}
      </div>
    </div>
  );
}
