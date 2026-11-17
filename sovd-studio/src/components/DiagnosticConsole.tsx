import { useState } from "react";
import { api } from "../services/api";

export default function DiagnosticConsole({ ecuId }: { ecuId: string }) {
  const [payload, setPayload] = useState("22 F1 90");
  const [response, setResponse] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  async function sendRequest() {
    const normalized = payload.replace(/\s+/g, "");
    if (!normalized || normalized.length % 2 !== 0 ||
        normalized.length > 8192 || !/^[0-9a-fA-F]+$/.test(normalized)) {
      setError("Enter an even-length hexadecimal UDS payload (1–4096 bytes).");
      setResponse(null);
      return;
    }

    setBusy(true);
    setError(null);
    setResponse(null);
    try {
      const result = await api.udsRequest(ecuId, normalized);
      setResponse(result.response_hex ?? "No response returned.");
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "Diagnostic request failed.");
    } finally {
      setBusy(false);
    }
  }

  return (
    <section className="panel mb-5 p-5">
      <div className="eyebrow">UDS SERVICE CONSOLE</div>
      <h3 className="mt-1 text-lg font-semibold">{ecuId} · Raw diagnostic request</h3>
      <p className="mt-1 text-sm text-slate-400">
        Sends a hexadecimal UDS payload through the central API and the ECU's shared dispatcher.
      </p>
      <label htmlFor="uds-payload" className="mt-4 block text-xs text-slate-400">
        REQUEST PAYLOAD (HEX)
      </label>
      <div className="mt-2 flex flex-col gap-2 sm:flex-row">
        <input
          id="uds-payload"
          value={payload}
          onChange={(event) => setPayload(event.target.value)}
          spellCheck={false}
          className="min-w-0 flex-1 rounded-lg border border-slate-700 bg-slate-950 px-3 py-2 font-mono text-sm text-slate-100 outline-none focus:border-cyan-500"
          placeholder="22 F1 90"
        />
        <button
          type="button"
          disabled={busy || !ecuId}
          onClick={() => void sendRequest()}
          className="rounded-lg border border-cyan-500/30 bg-cyan-500/10 px-4 py-2 text-sm font-medium text-cyan-300 disabled:opacity-50"
        >
          {busy ? "Sending..." : "Send UDS"}
        </button>
      </div>
      {response !== null && (
        <div className="mt-4 rounded-lg border border-emerald-500/20 bg-emerald-500/5 p-3">
          <div className="text-xs font-semibold text-emerald-300">RESPONSE (HEX)</div>
          <div className="mt-1 break-all font-mono text-sm text-slate-100">{response}</div>
        </div>
      )}
      {error !== null && (
        <div role="alert" className="mt-4 rounded-lg border border-rose-500/20 bg-rose-500/5 p-3 text-sm text-rose-300">
          {error}
        </div>
      )}
      <p className="mt-3 text-xs text-slate-500">
        Use only on the trusted development network. Diagnostic services may change ECU state.
      </p>
    </section>
  );
}
