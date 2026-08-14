/** Small presentational pieces shared across screens. */
import type { Evidence, Impact, Risk, Severity } from "../api";

export function ImpactBadge({ impact }: { impact: Impact }) {
  const cls = {
    Major: "badge-major",
    Moderate: "badge-moderate",
    Minor: "badge-minor",
  }[impact];
  return <span className={`badge ${cls}`}>{impact}</span>;
}

export function RiskBadge({ risk }: { risk: Risk }) {
  if (risk === "Low") return null; // low risk is the default; don't add noise
  const cls = risk === "High" ? "badge-high" : "badge-medium";
  return <span className={`badge ${cls}`}>{risk} risk</span>;
}

/**
 * Marks entries we do not expect to do anything. This is the visible half of the
 * catalog's honesty policy — these are applied because users look for them, and
 * labelled so the numbers above are not read as including them.
 */
export function EvidenceBadge({ evidence }: { evidence: Evidence }) {
  if (evidence !== "NoMeasuredBenefit") return null;
  return <span className="badge badge-neutral">no measured benefit</span>;
}

export function Notice({
  kind,
  title,
  children,
}: {
  kind: "danger" | "warn" | "info" | "ok";
  title: string;
  children?: React.ReactNode;
}) {
  const icon = { danger: "⚠", warn: "⚠", info: "ⓘ", ok: "✓" }[kind];
  return (
    <div className={`notice notice-${kind}`}>
      <div className="notice-icon">{icon}</div>
      <div className="notice-body">
        <div className="notice-title">{title}</div>
        {children && <div className="notice-text">{children}</div>}
      </div>
    </div>
  );
}

export function severityKind(severity: Severity): "danger" | "warn" | "info" {
  if (severity === "Critical") return "danger";
  if (severity === "High") return "warn";
  return "info";
}

export function Spinner({ label }: { label: string }) {
  return (
    <div className="center-state">
      <div className="spinner" />
      <div>{label}</div>
    </div>
  );
}

export function Stat({
  value,
  label,
  tone,
}: {
  value: number | string;
  label: string;
  tone?: "ok" | "warn" | "danger";
}) {
  const color =
    tone === "ok"
      ? "var(--ok)"
      : tone === "warn"
        ? "var(--warn)"
        : tone === "danger"
          ? "var(--danger)"
          : "var(--text)";
  return (
    <div className="stat">
      <div className="stat-number" style={{ color }}>
        {value}
      </div>
      <div className="stat-label">{label}</div>
    </div>
  );
}
