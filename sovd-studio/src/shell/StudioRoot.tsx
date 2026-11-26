// M2: application root - login gate, workspace, status bar.
import { useCallback, useEffect, useRef, useState } from "react";
import LoginScreen from "./LoginScreen";
import StatusBar from "./StatusBar";
import Workspace from "./Workspace";
import { studio, type SessionInfo } from "../services/studio";
import { inTauri, onSessionLost } from "../services/transport";

export default function StudioRoot() {
  const desktop = inTauri();
  const [session, setSession] = useState<SessionInfo | null>(null);
  const [checked, setChecked] = useState(!desktop);
  const [notice, setNotice] = useState("");
  const [connection, setConnection] = useState<"ok" | "error" | "unknown">("unknown");
  const reset = useRef<() => void>(() => undefined);

  useEffect(() => {
    if (!desktop) return;
    studio.session().then((s) => { setSession(s); setChecked(true); }).catch(() => setChecked(true));
    return onSessionLost((reason) => { setSession(null); setNotice(reason); });
  }, [desktop]);

  // Watch the session (idle/expiry are enforced in Rust; this keeps the UI in step).
  useEffect(() => {
    if (!desktop || !session) return;
    const t = window.setInterval(async () => {
      const s = await studio.session().catch(() => null);
      if (!s) { setSession(null); setNotice("Session ended (inactivity or expiry). Please sign in again."); }
    }, 15000);
    return () => window.clearInterval(t);
  }, [desktop, session]);

  // Connection indicator from the most recent trace entry.
  useEffect(() => {
    if (!desktop || !session) return;
    let last = 0;
    const t = window.setInterval(async () => {
      const fresh = await studio.traceSince(last).catch(() => []);
      if (fresh.length) {
        const e = fresh[fresh.length - 1];
        last = e.seq;
        setConnection(e.status !== null && e.status < 500 ? "ok" : "error");
      }
    }, 2000);
    return () => window.clearInterval(t);
  }, [desktop, session]);

  const logout = useCallback(async () => {
    await studio.logout().catch(() => undefined);
    setSession(null);
    setNotice("You have been signed out.");
  }, []);

  if (!checked) return <div className="h-screen bg-[#080d18]" />;
  if (desktop && !session) return <LoginScreen notice={notice} onLogin={(s) => { setNotice(""); setSession(s); }} />;

  return (
    <div className="flex h-screen flex-col bg-[#080d18] text-slate-100">
      <div className="min-h-0 flex-1">
        <Workspace key={session?.user ?? "browser"} session={session} onApi={(r) => { reset.current = r; }} />
      </div>
      <StatusBar session={session} connection={connection} onLogout={() => void logout()} onResetLayout={() => reset.current()} />
    </div>
  );
}
