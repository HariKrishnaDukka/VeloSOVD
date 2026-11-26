import { useEffect, useState } from "react";
import { LayoutTemplate, LogOut, ShieldCheck, User } from "lucide-react";
import type { SessionInfo } from "../services/studio";

function remaining(expires: number): string {
  const s = Math.max(0, expires - Math.floor(Date.now() / 1000));
  return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")}`;
}

export default function StatusBar({
  session,
  connection,
  onLogout,
  onResetLayout,
}: {
  session: SessionInfo | null;
  connection: "ok" | "error" | "unknown";
  onLogout: () => void;
  onResetLayout: () => void;
}) {
  const [, tick] = useState(0);
  useEffect(() => {
    const t = window.setInterval(() => tick((n) => n + 1), 1000);
    return () => window.clearInterval(t);
  }, []);

  const dot = connection === "ok" ? "bg-emerald-400" : connection === "error" ? "bg-rose-400" : "bg-slate-500";
  return (
    <footer className="flex h-8 shrink-0 items-center gap-4 border-t border-slate-800 bg-[#0b1220] px-3 text-[11px] text-slate-400">
      <span className="flex items-center gap-1.5"><span className={`h-2 w-2 rounded-full ${dot}`} /> central {connection}</span>
      {session ? (
        <>
          <span className="flex items-center gap-1.5"><User size={12} /> {session.user}</span>
          <span className="flex items-center gap-1.5"><ShieldCheck size={12} /> {session.role}</span>
          <span>login expires in {remaining(session.expires_unix)}</span>
          <span>idle logout {Math.round(session.idle_timeout_secs / 60)} min</span>
        </>
      ) : (
        <span>browser dev mode · no login</span>
      )}
      <span className="ml-auto" />
      <button onClick={onResetLayout} className="flex items-center gap-1 hover:text-slate-200"><LayoutTemplate size={12} /> reset layout</button>
      {session && <button onClick={onLogout} className="flex items-center gap-1 hover:text-rose-300"><LogOut size={12} /> logout</button>}
    </footer>
  );
}
