/** Step 1 — elevation check and API key. */
import { useEffect, useState } from "react";
import { api, errorMessage, type Environment } from "../api";
import { Notice } from "./shared";

export function Setup({
  env,
  onRefresh,
  onContinue,
}: {
  env: Environment;
  onRefresh: () => void;
  onContinue: () => void;
}) {
  const [key, setKey] = useState("");
  const [masked, setMasked] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    api.maskedApiKey().then(setMasked).catch(() => setMasked(null));
  }, [env.has_api_key]);

  async function save() {
    setBusy(true);
    setError(null);
    try {
      await api.saveApiKey(key.trim());
      setKey("");
      onRefresh();
    } catch (e) {
      setError(errorMessage(e));
    } finally {
      setBusy(false);
    }
  }

  async function clear() {
    await api.clearApiKey().catch(() => {});
    setMasked(null);
    onRefresh();
  }

  return (
    <div className="main-inner">
      <h1>Set up Forged</h1>
      <p className="lede">
        Forged scans this machine, has Claude work out which of its {env.catalog_size} vetted
        optimisations apply to your specific hardware, applies them reversibly, and gives you a
        BIOS checklist for the changes software cannot make.
      </p>

      {!env.elevated && (
        <Notice kind="danger" title="Not running as Administrator">
          Forged writes to protected registry keys and reconfigures services, which a standard user
          token cannot do. Close Forged and relaunch it as Administrator — the installer registers
          an elevation prompt, so launching from the Start menu shortcut is enough.
        </Notice>
      )}

      {env.elevated && (
        <Notice kind="ok" title="Running with Administrator rights">
          Forged can apply every change in its catalog.
        </Notice>
      )}

      <h2>Anthropic API key</h2>
      <div className="card">
        {masked ? (
          <>
            <div className="card-row">
              <div>
                <h3>Key saved</h3>
                <div className="muted small mono">{masked}</div>
              </div>
              <button className="btn btn-danger" onClick={clear}>
                Remove
              </button>
            </div>
            <p className="muted small" style={{ marginBottom: 0, marginTop: 12 }}>
              Stored encrypted with DPAPI under your Windows account, in{" "}
              <span className="mono">%APPDATA%\Forged</span>. Copying that file to another machine
              or user account yields nothing usable.
            </p>
          </>
        ) : (
          <>
            <h3>Add your key</h3>
            <p className="muted small">
              Forged asks for your own key rather than shipping one, because a key compiled into a
              distributed binary is extractable in seconds — and whoever shipped it pays for every
              call. Get one at{" "}
              <span className="mono">console.anthropic.com</span>.
            </p>
            <input
              className="input"
              type="password"
              placeholder="sk-ant-..."
              value={key}
              onChange={(e) => setKey(e.target.value)}
              onKeyDown={(e) => e.key === "Enter" && key.trim() && save()}
              style={{ marginTop: 10 }}
            />
            {error && (
              <div className="small" style={{ color: "var(--danger)", marginTop: 8 }}>
                {error}
              </div>
            )}
            <div className="btn-row" style={{ marginTop: 14 }}>
              <button
                className="btn btn-primary"
                disabled={!key.trim() || busy}
                onClick={save}
              >
                {busy ? "Saving…" : "Save key"}
              </button>
            </div>
          </>
        )}
      </div>

      {!env.has_api_key && (
        <Notice kind="info" title="You can continue without a key">
          Forged falls back to a deterministic plan that selects every applicable catalog entry.
          It works, but it cannot weigh tradeoffs against your hardware, order changes by
          dependency, or explain them in terms of your actual components. The report will say
          plainly that it ran offline.
        </Notice>
      )}

      <div className="btn-row">
        <button className="btn btn-primary" disabled={!env.elevated} onClick={onContinue}>
          Continue to scan
        </button>
      </div>
    </div>
  );
}
