/** Step 4 — what actually changed. */
import { useState } from "react";
import { api, errorMessage, type Report } from "../api";
import { EvidenceBadge, Notice, Spinner, Stat, severityKind } from "./shared";

export function ReportView({
  report,
  applying,
  error,
  onRetry,
  onViewBios,
}: {
  report: Report | null;
  applying: boolean;
  error: string | null;
  onRetry: () => void;
  onViewBios: () => void;
}) {
  const [exported, setExported] = useState<string | null>(null);
  const [exportError, setExportError] = useState<string | null>(null);

  if (applying) {
    return <Spinner label="Applying changes — taking a restore point, then writing each change…" />;
  }

  if (error) {
    return (
      <div className="main-inner">
        <h1>Apply failed</h1>
        <Notice kind="danger" title="The run could not complete">
          {error}
        </Notice>
        <div className="btn-row">
          <button className="btn btn-primary" onClick={onRetry}>
            Try again
          </button>
        </div>
      </div>
    );
  }

  if (!report) {
    return (
      <div className="main-inner">
        <h1>Nothing applied yet</h1>
        <p className="lede">Build and apply a plan first.</p>
      </div>
    );
  }

  async function save() {
    if (!report) return;
    setExportError(null);
    try {
      setExported(await api.exportReport(report));
    } catch (e) {
      setExportError(errorMessage(e));
    }
  }

  return (
    <div className="main-inner">
      <h1>Done</h1>
      <p className="lede selectable">{report.summary}</p>

      <div className="stat-row">
        <Stat value={report.effective_changes} label="performance changes" tone="ok" />
        {report.neutral_changes > 0 && (
          <Stat value={report.neutral_changes} label="applied, no expected gain" />
        )}
        <Stat value={report.skipped} label="skipped" />
        {report.failed > 0 && <Stat value={report.failed} label="failed" tone="danger" />}
      </div>

      {report.neutral_changes > 0 && (
        <Notice kind="info" title="About that second number">
          {report.neutral_changes} of the changes applied are entries that circulate widely in
          tweaking guides but have no measured effect. Forged applies them because they are
          harmless and people look for them, and counts them separately so the headline number
          means what it says.
        </Notice>
      )}

      {report.reboot_required && (
        <Notice kind="warn" title="Restart required">
          Some changes — driver-level and kernel settings especially — only take effect after a
          restart. Fast Startup has been disabled, so a normal shutdown is now a real shutdown.
        </Notice>
      )}

      {!report.restore_point_created && (
        <Notice kind="warn" title="No System Restore point was created">
          Forged's own rollback still works and is more precise than System Restore. This only
          removes the fallback for the case where Windows will not boot.
        </Notice>
      )}

      {report.failures.length > 0 && (
        <>
          <h2>Changes that failed</h2>
          {report.failures.map((f) => (
            <Notice key={f.id} kind="danger" title={f.name}>
              <span className="mono">{f.message}</span>
            </Notice>
          ))}
        </>
      )}

      {report.findings.length > 0 && (
        <>
          <h2>Worth more than anything above</h2>
          {report.findings.map((f, i) => (
            <Notice key={i} kind={severityKind(f.severity)} title={f.title}>
              {f.detail}
            </Notice>
          ))}
        </>
      )}

      {report.sections.map((section) => (
        <div className="section-block" key={section.section}>
          <div className="section-head">
            <span className="section-title">{section.label}</span>
            <span className="section-count">
              {section.effective_count} effective
              {section.neutral_count > 0 && ` · ${section.neutral_count} neutral`}
            </span>
          </div>
          {section.applied.map((change) => (
            <div
              className={`tweak${change.expected_to_help ? "" : " neutral"}`}
              key={change.id}
            >
              <div className="tweak-body">
                <div className="tweak-head">
                  <span className="tweak-name">
                    {change.expected_to_help ? "✓ " : "· "}
                    {change.name}
                  </span>
                  <EvidenceBadge evidence={change.evidence} />
                </div>
                <p className="tweak-reason selectable">{change.reason}</p>
                {change.tradeoff && (
                  <div className="tweak-tradeoff selectable">
                    <strong>Tradeoff:</strong> {change.tradeoff}
                  </div>
                )}
              </div>
            </div>
          ))}
        </div>
      ))}

      {report.game_settings.length > 0 && (
        <>
          <h2>Recommended in-game settings</h2>
          {report.game_settings.map((s, i) => (
            <div className="tweak" key={i}>
              <div className="tweak-body">
                <div className="tweak-head">
                  <span className="tweak-name">{s.setting}</span>
                  <span className="badge badge-moderate">{s.value}</span>
                </div>
                <p className="tweak-reason selectable">{s.reason}</p>
              </div>
            </div>
          ))}
        </>
      )}

      <div className="btn-row">
        <button className="btn btn-primary" onClick={onViewBios}>
          View your BIOS checklist
        </button>
        <button className="btn btn-secondary" onClick={save}>
          Save report
        </button>
        {exported && <span className="muted small mono">Saved to {exported}</span>}
        {exportError && (
          <span className="small" style={{ color: "var(--danger)" }}>
            {exportError}
          </span>
        )}
      </div>
    </div>
  );
}
