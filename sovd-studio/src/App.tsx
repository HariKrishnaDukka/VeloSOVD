import DiagnosticConsole from "./components/DiagnosticConsole";
import SovdExplorer from "./components/SovdExplorer";
import { useCallback, useEffect, useMemo, useState } from "react";
import {
  Activity, AlertTriangle, CarFront, CheckCircle2, Cpu, Gauge,
  LayoutDashboard, RefreshCw, Server, ShieldCheck, Terminal, Trash2, Wifi,
} from "lucide-react";
import {
  Area, AreaChart, CartesianGrid, ResponsiveContainer, Tooltip, XAxis, YAxis,
} from "recharts";
import { api } from "./services/api";
import { inTauri } from "./services/transport";
import { studio } from "./services/studio";
import type { Ecu, Fault, DiagnosticValue } from "./types";

type Page = "overview" | "ecus" | "diagnostics" | "explorer";
const zone = (id: string) => ["ECM", "LIGHTING", "DOOR"].includes(id) ? "FRONT" : "REAR";

export default function App() {
  const [page, setPage] = useState<Page>("overview");
  const [ecus, setEcus] = useState<Ecu[]>([]);
  const [selected, setSelected] = useState("ECM");
  const [speed, setSpeed] = useState<DiagnosticValue | null>(null);
  const [faults, setFaults] = useState<Fault[]>([]);
  const [history, setHistory] = useState<{time:string;speed:number}[]>([]);
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");
  const [busy, setBusy] = useState(false);
  const [updated, setUpdated] = useState("");
  const [pollMs, setPollMs] = useState(5000);

  const ecu = useMemo(() => ecus.find(x => x.id === selected), [ecus, selected]);
  const online = ecus.filter(x => x.status.toLowerCase() === "online").length;

  const loadEcus = useCallback(async () => {
    try {
      const list = await api.ecus();
      setEcus(list);
      setSelected(old => list.some(x => x.id === old) ? old : list[0]?.id ?? "");
      setError("");
      setUpdated(new Date().toLocaleTimeString());
    } catch (e) {
      setError(e instanceof Error ? e.message : "Central API unavailable");
    }
  }, []);

  const loadDetails = useCallback(async (id: string) => {
    if (!id) return;
    try {
      const [v, f] = await Promise.all([
        api.speed(id).catch(() => null),
        api.faults(id),
      ]);
      setSpeed(v);
      setFaults(f);
      if (v) setHistory(old => [
        ...old.slice(-19),
        { time: new Date().toLocaleTimeString(), speed: v.value },
      ]);
    } catch (e) {
      setError(e instanceof Error ? e.message : "ECU request failed");
    }
  }, []);

  // One polling interval for everything that calls central; the value lives in studio-core.
  useEffect(() => {
    void studio.pollIntervalMs().then(setPollMs).catch(() => undefined);
  }, []);

  useEffect(() => {
    void loadEcus();
    const timer = window.setInterval(() => void loadEcus(), pollMs);
    return () => window.clearInterval(timer);
  }, [loadEcus, pollMs]);

  useEffect(() => {
    setHistory([]);
    void loadDetails(selected);
    const timer = window.setInterval(() => void loadDetails(selected), pollMs);
    return () => window.clearInterval(timer);
  }, [selected, loadDetails, pollMs]);

  async function clearFaults() {
    if (!ecu || !window.confirm(`Clear stored faults for ${ecu.id}?`)) return;
    setBusy(true);
    setNotice("");
    try {
      const result = await api.clearFaults(ecu.id);
      setNotice(result.message);
      await loadDetails(ecu.id);
    } catch (e) {
      setNotice(e instanceof Error ? e.message : "Clear operation failed");
    } finally {
      setBusy(false);
    }
  }

  return <div className="relative flex h-full bg-[#080d18] text-slate-100">
    <aside className="hidden w-64 shrink-0 overflow-auto border-r border-slate-800 bg-[#0b1220] p-5 md:flex md:flex-col">
      <div className="mb-10 flex items-center gap-3">
        <div className="rounded-xl bg-cyan-400/10 p-3 text-cyan-300"><CarFront size={25}/></div>
        <div><div className="font-bold tracking-wide">SOVD STUDIO</div><div className="text-xs text-slate-500">Vehicle Diagnostic Platform</div></div>
      </div>
      <div className="eyebrow mb-3">Workspace</div>
      <nav className="space-y-2">
        <Nav active={page==="overview"} icon={<LayoutDashboard size={17}/>} onClick={()=>setPage("overview")}>Overview</Nav>
        <Nav active={page==="ecus"} icon={<Cpu size={17}/>} onClick={()=>setPage("ecus")}>ECU Explorer</Nav>
        <Nav active={page==="diagnostics"} icon={<Activity size={17}/>} onClick={()=>setPage("diagnostics")}>Diagnostics</Nav>
        <Nav active={page==="explorer"} icon={<Terminal size={17}/>} onClick={()=>setPage("explorer")}>SOVD Explorer</Nav>
      </nav>
      <div className="panel mt-auto p-4 text-xs"><div className="flex items-center gap-2 text-emerald-300"><Wifi size={14}/> CENTRAL API</div><div className="mt-2 text-slate-500">{inTauri() ? "Studio core (mTLS + user login)" : "Host development proxy"}</div><div className="mt-1 text-cyan-300">{inTauri() ? "audited per user" : "127.0.0.1:18010"}</div></div>
    </aside>

    <main className="min-w-0 flex-1 overflow-auto">
      <header className="sticky top-0 z-10 flex min-h-20 items-center justify-between border-b border-slate-800 bg-[#080d18]/95 px-5 backdrop-blur md:px-8">
        <div><div className="text-xs text-slate-500">VIRTUAL VEHICLE / DIAGNOSTICS</div><h1 className="mt-1 text-lg font-semibold">{page==="overview"?"Vehicle Overview":page==="ecus"?"ECU Explorer":page==="explorer"?"SOVD Explorer":"Fault Diagnostics"}</h1></div>
        <div className="flex items-center gap-3"><span className="hidden rounded-full bg-emerald-500/10 px-3 py-2 text-xs text-emerald-300 sm:block"><span className="mr-2 inline-block h-2 w-2 rounded-full bg-emerald-400"/>API polling</span><button onClick={()=>void loadEcus()} className="rounded-lg border border-slate-700 p-2.5"><RefreshCw size={16}/></button></div>
      </header>

      <div className="p-5 pb-24 md:p-8">
        {error && <div className="mb-5 rounded-xl border border-rose-500/30 bg-rose-500/10 p-3 text-sm text-rose-300">{error}</div>}
        {notice && <div className="mb-5 rounded-xl border border-cyan-500/30 bg-cyan-500/10 p-3 text-sm text-cyan-200">{notice}</div>}

        {page==="overview" && <>
          <div className="mb-7 flex flex-wrap items-end justify-between gap-3"><div><div className="eyebrow">LIVE SYSTEM MONITOR</div><h2 className="mt-2 text-2xl font-bold">Virtual Vehicle <span className="text-cyan-300">SDV-DEMO-001</span></h2><p className="mt-1 text-sm text-slate-500">Central diagnostic gateway · Two zonal controllers</p></div><div className="text-xs text-slate-500">Updated {updated || "—"}</div></div>
          <div className="mb-7 grid gap-4 sm:grid-cols-2 xl:grid-cols-4">
            <Metric icon={<Cpu/>} label="Registered ECUs" value={ecus.length} detail="Across two zones"/>
            <Metric icon={<ShieldCheck/>} label="Online ECUs" value={`${online}/${ecus.length}`} detail="Central inventory"/>
            <Metric icon={<AlertTriangle/>} label="Selected ECU faults" value={faults.filter(x=>x.active).length} detail={selected}/>
            <Metric icon={<Gauge/>} label="Vehicle speed" value={speed?`${speed.value.toFixed(1)} ${speed.unit}`:"—"} detail={selected}/>
          </div>
          <div className="mb-7 grid gap-5 xl:grid-cols-[1.2fr_1fr]">
            <section className="panel p-5"><div className="eyebrow">VEHICLE NETWORK</div><h3 className="mb-5 mt-1 font-semibold">Zonal architecture</h3><div className="mx-auto max-w-lg"><div className="rounded-xl border border-cyan-500/30 bg-cyan-500/5 p-4 text-center"><Server className="mx-auto mb-2 text-cyan-300"/><b>Central SOVD Gateway</b><div className="mt-1 text-xs text-slate-500">HTTP API · 2 zones · 5 ECUs</div></div><div className="mx-auto h-7 w-px bg-slate-700"/><div className="grid grid-cols-2 gap-3"><ZoneCard name="FRONT ZONE" ids={["ECM","LIGHTING","DOOR"]} ecus={ecus} choose={id=>{setSelected(id);setPage("ecus")}}/><ZoneCard name="REAR ZONE" ids={["BCM","HVAC"]} ecus={ecus} choose={id=>{setSelected(id);setPage("ecus")}}/></div></div></section>
            <section className="panel p-5"><div className="eyebrow">ECU TELEMETRY</div><h3 className="mt-1 font-semibold">Vehicle speed</h3><div className="my-3 text-4xl font-bold">{speed?.value.toFixed(1)??"—"} <span className="text-sm font-normal text-slate-500">{speed?.unit??"km/h"}</span></div><div className="h-48"><ResponsiveContainer width="100%" height="100%"><AreaChart data={history}><defs><linearGradient id="fill" x1="0" y1="0" x2="0" y2="1"><stop offset="0%" stopColor="#22d3ee" stopOpacity={.3}/><stop offset="100%" stopColor="#22d3ee" stopOpacity={0}/></linearGradient></defs><CartesianGrid stroke="#263449" strokeDasharray="3 3"/><XAxis dataKey="time" hide/><YAxis tick={{fill:"#718096",fontSize:10}}/><Tooltip contentStyle={{background:"#101827",border:"1px solid #334155"}}/><Area dataKey="speed" type="monotone" stroke="#22d3ee" fill="url(#fill)"/></AreaChart></ResponsiveContainer></div><div className="text-xs text-slate-500">Selected ECU: {selected} · 3-second sampling</div></section>
          </div>
          <div className="mb-3 flex items-center justify-between"><h3 className="font-semibold">ECU inventory</h3><button onClick={()=>setPage("ecus")} className="text-xs text-cyan-300">Explore all →</button></div>
          <EcuGrid ecus={ecus} selected={selected} choose={id=>{setSelected(id);setPage("ecus")}}/>
        </>}

        {page==="ecus" && <>
          <div className="mb-5"><div className="eyebrow">VEHICLE COMPONENTS</div><h2 className="mt-1 text-2xl font-bold">ECU Explorer</h2><p className="mt-1 text-sm text-slate-500">Select a controller to inspect status and live data.</p></div>
          <EcuGrid ecus={ecus} selected={selected} choose={setSelected}/>
          {ecu && <section className="panel mt-6 p-5"><div className="eyebrow">CONTROLLER DETAILS</div><h3 className="mt-2 text-xl font-bold">{ecu.name}</h3><div className="mt-4 grid grid-cols-2 gap-3 md:grid-cols-4"><Detail label="ECU ID" value={ecu.id}/><Detail label="Zone" value={zone(ecu.id)}/><Detail label="Domain" value={ecu.domain}/><Detail label="Status" value={ecu.status}/></div><div className="mt-5 text-sm text-slate-400">Vehicle speed</div><div className="mt-1 text-3xl font-bold">{speed?`${speed.value.toFixed(1)} ${speed.unit}`:"Unavailable"}</div></section>}
        </>}

        {page==="explorer" && <SovdExplorer />}

        {page==="diagnostics" && <>
          <div className="mb-5"><div className="eyebrow">DIAGNOSTIC MANAGEMENT</div><h2 className="mt-1 text-2xl font-bold">Fault Diagnostics</h2><p className="mt-1 text-sm text-slate-500">Inspect diagnostic trouble codes reported by each ECU.</p></div>
          <section className="panel mb-5 p-4"><label className="mb-2 block text-xs text-slate-400">SELECT ECU</label><select value={selected} onChange={e=>setSelected(e.target.value)} className="w-full rounded-lg border border-slate-700 bg-[#0b1220] p-3 text-sm sm:max-w-sm">{ecus.map(x=><option key={x.id} value={x.id}>{x.id} — {x.name}</option>)}</select></section>
          <DiagnosticConsole ecuId={selected} />
          <section className="panel p-5"><div className="flex flex-wrap items-center justify-between gap-3"><div><div className="eyebrow">DIAGNOSTIC TROUBLE CODES</div><h3 className="mt-1 text-lg font-semibold">{selected} · {faults.length} faults</h3></div><button disabled={!faults.length||busy} onClick={()=>void clearFaults()} className="flex items-center gap-2 rounded-lg border border-rose-500/30 bg-rose-500/10 px-3 py-2 text-sm text-rose-300"><Trash2 size={15}/>{busy?"Clearing...":"Clear faults"}</button></div>
          <div className="mt-5 space-y-3">{faults.length===0?<div className="rounded-xl border border-emerald-500/20 bg-emerald-500/5 p-6 text-center"><CheckCircle2 className="mx-auto mb-2 text-emerald-400"/><div className="text-emerald-300">No faults reported</div></div>:faults.map(f=><div key={f.code} className="flex flex-wrap justify-between gap-3 rounded-xl border border-slate-700 bg-[#0b1220] p-4"><div><div className="font-mono text-sm font-semibold">{f.code}</div><div className="mt-1 text-sm text-slate-400">{f.description}</div></div><span className={`text-xs ${f.active?"text-amber-300":"text-slate-400"}`}>{f.active?"Active":"Stored"}</span></div>)}</div>
          <p className="mt-5 text-xs leading-5 text-slate-500">Demonstrator note: REST clear-faults clears the in-memory DTC store. It does not demonstrate an end-to-end UDS SID 0x14 request over DoIP or ISO-TP.</p></section>
        </>}
        <footer className="mt-10 border-t border-slate-800 pt-5 text-xs text-slate-600">SOVD Vehicle Diagnostic Studio · Development build</footer>
      </div>
    </main>
    <div className="absolute bottom-0 left-0 right-0 z-20 grid grid-cols-4 border-t border-slate-800 bg-[#0b1220] p-2 md:hidden"><Mobile active={page==="overview"} onClick={()=>setPage("overview")} icon={<LayoutDashboard size={18}/>} label="Overview"/><Mobile active={page==="ecus"} onClick={()=>setPage("ecus")} icon={<Cpu size={18}/>} label="ECUs"/><Mobile active={page==="diagnostics"} onClick={()=>setPage("diagnostics")} icon={<Activity size={18}/>} label="Diagnostics"/><Mobile active={page==="explorer"} onClick={()=>setPage("explorer")} icon={<Terminal size={18}/>} label="Explorer"/></div>
  </div>;
}

function Nav({active,icon,children,onClick}:{active:boolean;icon:React.ReactNode;children:React.ReactNode;onClick:()=>void}) {
  return <button onClick={onClick} className={`flex w-full items-center gap-3 rounded-lg px-3 py-3 text-sm ${active?"bg-cyan-400/10 text-cyan-200":"text-slate-400 hover:bg-slate-800"}`}>{icon}{children}</button>;
}
function Metric({icon,label,value,detail}:{icon:React.ReactNode;label:string;value:string|number;detail:string}) {
  return <div className="panel p-4"><div className="flex justify-between text-slate-400"><span className="text-xs">{label}</span><span className="text-cyan-300">{icon}</span></div><div className="mt-3 text-2xl font-bold">{value}</div><div className="mt-1 text-xs text-slate-500">{detail}</div></div>;
}
function EcuGrid({ecus,selected,choose}:{ecus:Ecu[];selected:string;choose:(id:string)=>void}) {
  return <div className="grid gap-3 sm:grid-cols-2 xl:grid-cols-3">{ecus.map(x=><button key={x.id} onClick={()=>choose(x.id)} className={`panel p-4 text-left hover:border-cyan-500/50 ${selected===x.id?"border-cyan-500/60 bg-cyan-500/5":""}`}><div className="flex justify-between"><span className="flex items-center gap-2 font-semibold"><Cpu size={17} className="text-cyan-300"/>{x.id}</span><span className={`text-xs ${x.status.toLowerCase()==="online"?"text-emerald-300":"text-rose-300"}`}>● {x.status}</span></div><div className="mt-3 text-sm text-slate-300">{x.name}</div><div className="mt-2 flex justify-between text-xs text-slate-500"><span>{zone(x.id)} ZONE</span><span>{x.domain}</span></div></button>)}</div>;
}
function ZoneCard({name,ids,ecus,choose}:{name:string;ids:string[];ecus:Ecu[];choose:(id:string)=>void}) {
  return <div className="rounded-xl border border-slate-700 bg-[#0b1220] p-3"><div className="mb-3 text-center text-xs font-bold text-cyan-300">{name}</div>{ids.map(id=>{const item=ecus.find(x=>x.id===id);return <button key={id} onClick={()=>choose(id)} className="mb-2 flex w-full justify-between rounded-lg bg-slate-800/70 px-2 py-2 text-xs"><span>{id}</span><span className="text-emerald-400">{item?.status??"—"}</span></button>})}</div>;
}
function Detail({label,value}:{label:string;value:string}) {
  return <div className="rounded-lg bg-[#0b1220] p-3"><div className="text-xs text-slate-500">{label}</div><div className="mt-1 text-sm">{value}</div></div>;
}
function Mobile({active,onClick,icon,label}:{active:boolean;onClick:()=>void;icon:React.ReactNode;label:string}) {
  return <button onClick={onClick} className={`flex flex-col items-center gap-1 py-2 text-xs ${active?"text-cyan-300":"text-slate-500"}`}>{icon}{label}</button>;
}
