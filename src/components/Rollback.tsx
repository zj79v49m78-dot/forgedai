/** Undo. Available at every point in the flow, not just after a run. */
import { useCallback, useEffect, useState } from "react";
import { api, errorMessage, type JournalSummary, type RevertReport } from "../api";
import { Notice } from "./shared";

export function Rollback() {
  const [runs, setRuns] = useState<JournalSummary[]>([]);
  const [busy, setBusy] = useState<string | null>(null);
  const [result, setResult] = useState<RevertReport | null>(null);
  const [error, setError] = useState<string | null>(null);

  const refresh = useCallback(() => {
    api
      .runs()
      .then(setRuns)
      .catch((e) => setError(errorMessage(e)));
  }, []);

  useEffect(refresh, [refresh]);

  async function revert(runId: string) {
    setBusy(runId);
    setError(null);
    setResult(null);
    try {
      setResult(await api.rollback(runId));
      refresh();
    } catch (e) {
      setError(errorMessage(e));
    } finally {
      setBusy(null);
    }
  }

  return (
    <div className="main-inner">
      <h1>Rollback</h1>
      <p className="lede">
        Every change Forged makes is journalled with the exact value that was there before. Undo
        restores those values precisely — it does not guess at defaults, and it does not need the
        System Restore point.
      </p>

      {error && (
        <Notice kind="danger" title="Rollback problem">
          {error}
        </Notice>
      )}

      {result && (
        <Notice
          kind={result.failed.length > 0 ? "warn" : "ok"}
          title={`Restored ${result.restored} value${result.restored === 1 ? "" : "s"} from run ${result.run_id}`}
        >
          {result.failed.length > 0 ? (
            <>
              {result.failed.length} could not be restored:
              <ul className="clean">
                {result.failed.map((f, i) => (
                  <li key={i} className="mono small">
                    {f}
                  </li>
                ))}
              </ul>
            </>
          ) : (
            "Restart to make sure every restored setting takes effect."
          )}
        </Notice>
      )}

      {runs.length === 0 ? (
        <div className="card">
          <h3>No runs yet</h3>
          <p className="muted small" style={{ margin: 0 }}>
            Once you apply a plan it will appear here and stay available to undo.
          </p>
        </div>
      ) : (
        runs.map((run) => (
          <div className="run-row" key={run.run_id}>
            <div>
              <div style={{ fontWeight: 600 }}>Run {run.run_id}</div>
              <div className="run-meta">{run.description}</div>
            </div>
            <button
              className="btn btn-danger"
              disabled={run.reverted || run.applied === 0 || busy !== null}
              onClick={() => revert(run.run_id)}
            >
              {busy === run.run_id
                ? "Reverting…"
                : run.reverted
                  ? "Already reverted"
                  : "Revert this run"}
            </button>
          </div>
        ))
      )}
    </div>
  );
}
