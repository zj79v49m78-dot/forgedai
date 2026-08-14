/** Step 3 — review the plan Claude produced, per section, before applying. */
import { useMemo } from "react";
import {
  SECTION_LABELS,
  SECTION_ORDER,
  type PlanResult,
  type Section,
  type TweakMeta,
} from "../api";
import { EvidenceBadge, ImpactBadge, Notice, RiskBadge, Spinner } from "./shared";

export function Plan({
  result,
  catalog,
  building,
  error,
  enabled,
  onToggle,
  onBuild,
  onApply,
}: {
  result: PlanResult | null;
  catalog: TweakMeta[];
  building: boolean;
  error: string | null;
  enabled: Set<string>;
  onToggle: (id: string) => void;
  onBuild: () => void;
  onApply: () => void;
}) {
  const byId = useMemo(
    () => new Map(catalog.map((t) => [t.id, t])),
    [catalog],
  );

  const grouped = useMemo(() => {
    if (!result) return new Map<Section, { meta: TweakMeta; reason: string }[]>();
    const map = new Map<Section, { meta: TweakMeta; reason: string }[]>();
    for (const sel of result.plan.selected) {
      const meta = byId.get(sel.id);
      if (!meta) continue;
      const list = map.get(meta.section) ?? [];
      list.push({ meta, reason: sel.reason });
      map.set(meta.section, list);
    }
    return map;
  }, [result, byId]);

  if (building) {
    return <Spinner label="Claude is planning the optimisation for this hardware…" />;
  }

  if (error) {
    return (
      <div className="main-inner">
        <h1>Planning failed</h1>
        <Notice kind="danger" title="Could not build the plan">
          {error}
        </Notice>
        <div className="btn-row">
          <button className="btn btn-primary" onClick={onBuild}>
            Try again
          </button>
        </div>
      </div>
    );
  }

  if (!result) {
    return (
      <div className="main-inner">
        <h1>Build the plan</h1>
        <div className="btn-row">
          <button className="btn btn-primary" onClick={onBuild}>
            Build plan
          </button>
        </div>
      </div>
    );
  }

  const { plan, validation, offline } = result;
  const selectedCount = plan.selected.filter((s) => enabled.has(s.id)).length;
  const rebootCount = plan.selected.filter(
    (s) => enabled.has(s.id) && byId.get(s.id)?.requires_reboot,
  ).length;

  return (
    <div className="main-inner">
      <h1>The plan</h1>
      <p className="lede selectable">{plan.summary}</p>

      {offline && (
        <Notice kind="warn" title="Planned offline — the AI planner was not used">
          <p style={{ margin: "0 0 8px" }}>
            This is the deterministic fallback: every catalog entry that applies to your hardware,
            without AI ordering or hardware-specific reasoning. It is safe to apply and every change
            is still reversible — you are just missing the tailored explanations.
          </p>
          {result.fallback_reason && (
            <p className="selectable" style={{ margin: 0, color: "var(--text)" }}>
              <strong>Reason:</strong> {result.fallback_reason}
            </p>
          )}
        </Notice>
      )}

      {validation.unknown_ids.length > 0 && (
        <Notice kind="info" title="Some suggestions were discarded">
          The model returned {validation.unknown_ids.length} identifier
          {validation.unknown_ids.length === 1 ? "" : "s"} that {validation.unknown_ids.length === 1 ? "is" : "are"}{" "}
          not in the catalog, so {validation.unknown_ids.length === 1 ? "it was" : "they were"}{" "}
          dropped before anything ran. This is the safety check working as designed — the engine
          only executes vetted entries.
        </Notice>
      )}

      {plan.hardware_notes.length > 0 && (
        <>
          <h2>Things no software change can fix</h2>
          {plan.hardware_notes.map((note, i) => (
            <Notice key={i} kind="warn" title={note} />
          ))}
        </>
      )}

      <div className="stat-row" style={{ marginTop: 24 }}>
        <div className="stat">
          <div className="stat-number">{selectedCount}</div>
          <div className="stat-label">changes selected</div>
        </div>
        <div className="stat">
          <div className="stat-number">{grouped.size}</div>
          <div className="stat-label">areas covered</div>
        </div>
        <div className="stat">
          <div className="stat-number">{rebootCount}</div>
          <div className="stat-label">need a restart</div>
        </div>
      </div>

      {SECTION_ORDER.filter((s) => grouped.has(s)).map((section) => {
        const items = grouped.get(section)!;
        return (
          <div className="section-block" key={section}>
            <div className="section-head">
              <span className="section-title">{SECTION_LABELS[section]}</span>
              <span className="section-count">
                {items.filter((i) => enabled.has(i.meta.id)).length} of {items.length} selected
              </span>
            </div>

            {items.map(({ meta, reason }) => {
              const isNeutral = meta.evidence === "NoMeasuredBenefit";
              return (
                <label
                  className={`tweak${isNeutral ? " neutral" : ""}`}
                  key={meta.id}
                  htmlFor={`t-${meta.id}`}
                >
                  <input
                    id={`t-${meta.id}`}
                    className="tweak-check"
                    type="checkbox"
                    checked={enabled.has(meta.id)}
                    onChange={() => onToggle(meta.id)}
                  />
                  <div className="tweak-body">
                    <div className="tweak-head">
                      <span className="tweak-name">{meta.name}</span>
                      <ImpactBadge impact={meta.impact} />
                      <RiskBadge risk={meta.risk} />
                      <EvidenceBadge evidence={meta.evidence} />
                      {meta.requires_reboot && (
                        <span className="badge badge-minor">restart</span>
                      )}
                    </div>
                    <p className="tweak-reason selectable">{reason}</p>
                    {meta.tradeoff && (
                      <div className="tweak-tradeoff selectable">
                        <strong>Tradeoff:</strong> {meta.tradeoff}
                      </div>
                    )}
                  </div>
                </label>
              );
            })}
          </div>
        );
      })}

      {plan.rejected.length > 0 && (
        <>
          <h2>Deliberately not applied</h2>
          <p className="muted small">
            Catalog entries Claude ruled out for this specific machine.
          </p>
          {plan.rejected.map((r) => {
            const meta = byId.get(r.id);
            if (!meta) return null;
            return (
              <div className="tweak neutral" key={r.id}>
                <div className="tweak-body">
                  <div className="tweak-head">
                    <span className="tweak-name">{meta.name}</span>
                  </div>
                  <p className="tweak-reason selectable">{r.reason}</p>
                </div>
              </div>
            );
          })}
        </>
      )}

      <div className="btn-row">
        <button
          className="btn btn-primary"
          disabled={selectedCount === 0}
          onClick={onApply}
        >
          Apply {selectedCount} change{selectedCount === 1 ? "" : "s"}
        </button>
        <button className="btn btn-secondary" onClick={onBuild}>
          Re-plan
        </button>
        <span className="muted small">
          A restore point is taken first, and every change is individually reversible.
        </span>
      </div>
    </div>
  );
}
