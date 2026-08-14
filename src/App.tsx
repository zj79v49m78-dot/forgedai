import { useCallback, useEffect, useState } from "react";
import {
  api,
  errorMessage,
  type Environment,
  type HardwareProfile,
  type PlanResult,
  type Report,
  type TweakMeta,
} from "./api";
import { Bios } from "./components/Bios";
import { Plan } from "./components/Plan";
import { ReportView } from "./components/ReportView";
import { Rollback } from "./components/Rollback";
import { Scan } from "./components/Scan";
import { Setup } from "./components/Setup";

type Screen = "setup" | "scan" | "plan" | "report" | "bios" | "rollback";

const STEPS: { id: Screen; label: string; step: string }[] = [
  { id: "setup", label: "Setup", step: "1" },
  { id: "scan", label: "Scan", step: "2" },
  { id: "plan", label: "Plan", step: "3" },
  { id: "report", label: "Apply", step: "4" },
  { id: "bios", label: "BIOS", step: "5" },
];

export default function App() {
  const [screen, setScreen] = useState<Screen>("setup");
  const [env, setEnv] = useState<Environment | null>(null);

  const [profile, setProfile] = useState<HardwareProfile | null>(null);
  const [scanning, setScanning] = useState(false);
  const [scanError, setScanError] = useState<string | null>(null);

  const [catalog, setCatalog] = useState<TweakMeta[]>([]);
  const [planResult, setPlanResult] = useState<PlanResult | null>(null);
  const [planning, setPlanning] = useState(false);
  const [planError, setPlanError] = useState<string | null>(null);
  // Which of the planned tweaks are ticked. Defaults to all of them.
  const [enabled, setEnabled] = useState<Set<string>>(new Set());

  const [report, setReport] = useState<Report | null>(null);
  const [applying, setApplying] = useState(false);
  const [applyError, setApplyError] = useState<string | null>(null);

  const refreshEnv = useCallback(() => {
    api.environment().then(setEnv).catch(() => setEnv(null));
  }, []);

  useEffect(() => {
    refreshEnv();
    api.catalog().then(setCatalog).catch(() => setCatalog([]));
  }, [refreshEnv]);

  const runScan = useCallback(async () => {
    setScanning(true);
    setScanError(null);
    try {
      setProfile(await api.scan());
    } catch (e) {
      setScanError(errorMessage(e));
    } finally {
      setScanning(false);
    }
  }, []);

  const buildPlan = useCallback(async () => {
    setPlanning(true);
    setPlanError(null);
    try {
      const result = await api.buildPlan();
      setPlanResult(result);
      setEnabled(new Set(result.plan.selected.map((s) => s.id)));
    } catch (e) {
      setPlanError(errorMessage(e));
    } finally {
      setPlanning(false);
    }
  }, []);

  const applyPlan = useCallback(async () => {
    setScreen("report");
    setApplying(true);
    setApplyError(null);
    try {
      setReport(await api.apply(Array.from(enabled)));
      refreshEnv();
    } catch (e) {
      setApplyError(errorMessage(e));
    } finally {
      setApplying(false);
    }
  }, [enabled, refreshEnv]);

  function toggle(id: string) {
    setEnabled((prev) => {
      const next = new Set(prev);
      if (!next.delete(id)) next.add(id);
      return next;
    });
  }

  function goToPlan() {
    setScreen("plan");
    if (!planResult && !planning) buildPlan();
  }

  // Steps unlock in order; there is no useful state at step 3 without step 2.
  function isReachable(id: Screen): boolean {
    switch (id) {
      case "setup":
      case "scan":
        return true;
      case "plan":
        return profile !== null;
      case "report":
        return report !== null || applying;
      case "bios":
        return report !== null;
      default:
        return true;
    }
  }

  function isDone(id: Screen): boolean {
    switch (id) {
      case "setup":
        return env?.elevated ?? false;
      case "scan":
        return profile !== null;
      case "plan":
        return planResult !== null;
      case "report":
        return report !== null;
      case "bios":
        return false;
      default:
        return false;
    }
  }

  return (
    <div className="app">
      <nav className="sidebar">
        <div className="brand">
          <div className="brand-mark">F</div>
          <div>
            <div className="brand-name">Forged</div>
            <div className="brand-version">v{env?.version ?? "1.0.0"}</div>
          </div>
        </div>

        {STEPS.map((s) => (
          <button
            key={s.id}
            className={`nav-item${screen === s.id ? " active" : ""}${isDone(s.id) ? " done" : ""}`}
            disabled={!isReachable(s.id)}
            onClick={() => (s.id === "plan" ? goToPlan() : setScreen(s.id))}
          >
            <span className="nav-step">{isDone(s.id) ? "✓" : s.step}</span>
            {s.label}
          </button>
        ))}

        <div style={{ height: 12 }} />

        <button
          className={`nav-item${screen === "rollback" ? " active" : ""}`}
          onClick={() => setScreen("rollback")}
        >
          <span className="nav-step">↺</span>
          Rollback
        </button>

        <div className="sidebar-footer">
          {env?.catalog_size ?? 0} vetted tweaks
          <br />
          {env?.elevated ? "Administrator" : "Not elevated"}
          {env?.pending_rollback && (
            <>
              <br />
              <span style={{ color: "var(--warn)" }}>Previous run can be undone</span>
            </>
          )}
        </div>
      </nav>

      <main className="main">
        {screen === "setup" && env && (
          <Setup env={env} onRefresh={refreshEnv} onContinue={() => setScreen("scan")} />
        )}

        {screen === "scan" && (
          <Scan
            profile={profile}
            scanning={scanning}
            error={scanError}
            onScan={runScan}
            onContinue={goToPlan}
          />
        )}

        {screen === "plan" && (
          <Plan
            result={planResult}
            catalog={catalog}
            building={planning}
            error={planError}
            enabled={enabled}
            onToggle={toggle}
            onBuild={buildPlan}
            onApply={applyPlan}
          />
        )}

        {screen === "report" && (
          <ReportView
            report={report}
            applying={applying}
            error={applyError}
            onRetry={applyPlan}
            onViewBios={() => setScreen("bios")}
          />
        )}

        {screen === "bios" && <Bios report={report} />}
        {screen === "rollback" && <Rollback />}
      </main>
    </div>
  );
}
