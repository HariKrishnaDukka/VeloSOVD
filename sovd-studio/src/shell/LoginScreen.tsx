import { useEffect, useState } from "react";
import { CarFront, KeyRound, ShieldCheck } from "lucide-react";
import { studio, type SessionInfo, type StudioInfo } from "../services/studio";
import { describe } from "../services/transport";

export default function LoginScreen({
  onLogin,
  notice,
}: {
  onLogin: (session: SessionInfo) => void;
  notice?: string;
}) {
  const [username, setUsername] = useState("");
  const [password, setPassword] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [info, setInfo] = useState<StudioInfo | null>(null);

  useEffect(() => {
    studio.info().then(setInfo).catch(() => setInfo(null));
  }, []);

  async function submit(event: React.FormEvent) {
    event.preventDefault();
    setBusy(true);
    setError("");
    try {
      const session = await studio.login(username, password);
      setPassword("");
      onLogin(session);
    } catch (cause) {
      setError(describe(cause));
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="flex h-screen items-center justify-center bg-[#080d18] text-slate-100">
      <form onSubmit={submit} className="panel w-[420px] p-8">
        <div className="mb-6 flex items-center gap-3">
          <div className="rounded-xl bg-cyan-400/10 p-3 text-cyan-300"><CarFront size={26} /></div>
          <div>
            <div className="font-bold tracking-wide">VELOSOVD STUDIO</div>
            <div className="text-xs text-slate-500">SOVD diagnostic tool · v{info?.version ?? "…"}</div>
          </div>
        </div>

        {notice && <div className="mb-4 rounded-lg border border-amber-500/30 bg-amber-500/10 p-3 text-sm text-amber-200">{notice}</div>}
        {info?.startup_error && (
          <div className="mb-4 rounded-lg border border-rose-500/30 bg-rose-500/10 p-3 text-xs text-rose-200">
            Studio is not configured: {info.startup_error}
          </div>
        )}

        <label className="block text-xs text-slate-400">
          USER NAME
          <input
            autoFocus
            value={username}
            onChange={(e) => setUsername(e.target.value)}
            autoComplete="username"
            className="mt-2 w-full rounded-lg border border-slate-700 bg-slate-950 px-3 py-2 text-sm"
          />
        </label>
        <label className="mt-4 block text-xs text-slate-400">
          PASSWORD
          <input
            type="password"
            value={password}
            onChange={(e) => setPassword(e.target.value)}
            autoComplete="current-password"
            className="mt-2 w-full rounded-lg border border-slate-700 bg-slate-950 px-3 py-2 text-sm"
          />
        </label>

        {error && <div className="mt-4 rounded-lg border border-rose-500/30 bg-rose-500/10 p-3 text-sm text-rose-300">{error}</div>}

        <button
          type="submit"
          disabled={busy || !username || !password || !!info?.startup_error}
          className="mt-6 flex w-full items-center justify-center gap-2 rounded-lg bg-cyan-500 px-4 py-2.5 text-sm font-semibold text-slate-950 disabled:opacity-40"
        >
          <KeyRound size={16} /> {busy ? "Signing in…" : "Sign in"}
        </button>

        <div className="mt-6 space-y-1 border-t border-slate-800 pt-4 text-[11px] text-slate-500">
          <div className="flex items-center gap-2"><ShieldCheck size={12} /> Device certificate + user login (TLS 1.3)</div>
          <div>Auth: {info?.auth_url ?? "—"} · verified as {info?.tls_server_name ?? "—"}</div>
          <div>Central: {info?.central_url ?? "—"}</div>
        </div>
      </form>
    </div>
  );
}
