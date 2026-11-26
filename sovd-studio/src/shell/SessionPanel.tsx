import { useEffect, useState } from "react";
import { studio, type LicenseStatus, type SessionInfo, type StudioInfo } from "../services/studio";

export default function SessionPanel({ session }: { session: SessionInfo | null }) {
  const [info, setInfo] = useState<StudioInfo | null>(null);
  const [license, setLicense] = useState<LicenseStatus | null>(null);
  useEffect(() => {
    studio.info().then(setInfo).catch(() => setInfo(null));
    studio.license().then(setLicense).catch(() => setLicense(null));
  }, []);

  const row = (k: string, v: string | number | null | undefined) => (
    <div className="flex justify-between gap-4 border-b border-slate-900 py-1.5">
      <span className="text-slate-500">{k}</span><span className="break-all text-right font-mono">{v ?? "—"}</span>
    </div>
  );
  return (
    <div className="h-full overflow-auto bg-[#080d18] p-4 text-xs text-slate-300">
      <div className="eyebrow mb-2">SESSION</div>
      {row("user", session?.user)}
      {row("role", session?.role)}
      {row("login expires", session ? new Date(session.expires_unix * 1000).toLocaleTimeString() : null)}
      {row("idle logout", session ? `${Math.round(session.idle_timeout_secs / 60)} min` : null)}
      <div className="eyebrow mb-2 mt-5">CONNECTION</div>
      {row("auth-service", info?.auth_url)}
      {row("central", info?.central_url)}
      {row("server identity", info?.tls_server_name)}
      {row("device certificate", info?.device_cert)}
      {row("config", info?.config_path)}
      <div className="eyebrow mb-2 mt-5">LICENSE</div>
      {row("licensed to", license?.licensed_to)}
      {row("type", license?.kind)}
      {license?.features.map(([f, on]) => row(f, on ? "enabled" : "disabled"))}
    </div>
  );
}
