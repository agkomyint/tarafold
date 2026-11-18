import { useEffect, useMemo, useRef, useState } from "react";
import { EvidenceBadge, GradeBadge } from "./components";
import { Ctx, useApp, type AppState } from "./ctx";
import type { Loaded, ProgressEvent } from "./report/load";
import type { Report } from "./report/schema";
import { Data } from "./views/Data";
import { Equations } from "./views/Equations";
import { Experiments } from "./views/Experiments";
import { Explore } from "./views/Explore";
import { Overview } from "./views/Overview";
import { Relationships } from "./views/Relationships";
import { Reliability } from "./views/Reliability";
import { Target } from "./views/Target";

const VIEWS = [
  { id: "overview", label: "Overview", el: Overview },
  { id: "data", label: "Data", el: Data },
  { id: "explore", label: "Explore", el: Explore },
  { id: "relationships", label: "Relationships", el: Relationships },
  { id: "equations", label: "Equations", el: Equations },
  { id: "target", label: "Target", el: Target },
  { id: "experiments", label: "Next experiments", el: Experiments },
  { id: "reliability", label: "Reliability", el: Reliability },
] as const;

type ViewId = (typeof VIEWS)[number]["id"];

export function App({ loaded }: { loaded: Loaded }) {
  const [report, setReport] = useState<Report>(loaded.report);
  const [targetIndex, setTargetIndex] = useState(0);
  const [view, setView] = useState<ViewId>("overview");
  const [shown, setShown] = useState<Record<number, number>>({});
  const t = report.targets[Math.min(targetIndex, report.targets.length - 1)];
  const tabs = useRef<(HTMLButtonElement | null)[]>([]);

  const state: AppState | null = useMemo(
    () =>
      t
        ? {
            report,
            data: loaded.data,
            api: loaded.api,
            target: t,
            targetIndex,
            shownEquation: shown[targetIndex] ?? t.discover.selected_id,
            setShownEquation: (id: number) => setShown((s) => ({ ...s, [targetIndex]: id })),
            setReport: (r: Report) => {
              setReport(r);
              setShown({});
            },
          }
        : null,
    [report, loaded, t, targetIndex, shown],
  );
  if (!state || !t) return <p>The report has no targets.</p>;
  const View = VIEWS.find((v) => v.id === view)!.el;
  const sel = t.discover.front.find((e) => e.id === t.discover.selected_id);

  const onTabKey = (e: React.KeyboardEvent, i: number) => {
    const n = VIEWS.length;
    const k = e.key === "ArrowRight" ? (i + 1) % n : e.key === "ArrowLeft" ? (i - 1 + n) % n : -1;
    if (k >= 0) {
      e.preventDefault();
      setView(VIEWS[k]!.id);
      tabs.current[k]?.focus();
    }
  };

  return (
    <Ctx.Provider value={state}>
      <header className="top">
        <div className="brand">
          <span className="logo" aria-hidden="true">
            ∮
          </span>{" "}
          Tarafold
        </div>
        <div className="summary">
          {report.targets.length > 1 ? (
            <label>
              Target{" "}
              <select value={targetIndex} onChange={(e) => setTargetIndex(Number(e.target.value))}>
                {report.targets.map((x, i) => (
                  <option key={x.target} value={i}>
                    {x.target}
                  </option>
                ))}
              </select>
            </label>
          ) : (
            <strong>{t.target}</strong>
          )}
          {sel && <span className="mono small eq-inline">= {sel.infix}</span>}
          <GradeBadge grade={t.validate.reliability.grade} />
          <EvidenceBadge evidence={t.validate.evidence} />
        </div>
        {loaded.api && <RunPanel />}
      </header>
      <nav className="tabs" role="tablist" aria-label="Report views">
        {VIEWS.map((v, i) => (
          <button
            key={v.id}
            ref={(el) => {
              tabs.current[i] = el;
            }}
            role="tab"
            id={`tab-${v.id}`}
            aria-selected={v.id === view}
            aria-controls="panel"
            tabIndex={v.id === view ? 0 : -1}
            onClick={() => setView(v.id)}
            onKeyDown={(e) => onTabKey(e, i)}
          >
            {v.label}
          </button>
        ))}
      </nav>
      <main id="panel" role="tabpanel" aria-labelledby={`tab-${view}`}>
        <View />
      </main>
      <footer className="muted small">
        Tarafold {report.tarafold_version} · report schema {report.schema_version} · run {report.run.id}. Equations describe this
        data; on observational data they are associations, not effects of intervening.
      </footer>
    </Ctx.Provider>
  );
}

/** Re-run the full pipeline on the server (new target or budget), with progress over SSE. */
function RunPanel() {
  const ctx = useApp();
  const [open, setOpen] = useState(false);
  const [targets, setTargets] = useState(ctx.report.targets.map((x) => x.target).join(","));
  const [maxEvals, setMaxEvals] = useState("");
  const [msgs, setMsgs] = useState<ProgressEvent[]>([]);
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState<string | null>(null);
  useEffect(() => {
    if (!busy || !ctx.api) return;
    return ctx.api.events((m) => setMsgs((x) => [...x.slice(-20), m]));
  }, [busy, ctx.api]);
  const go = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!ctx.api) return;
    setBusy(true);
    setErr(null);
    setMsgs([]);
    try {
      const r = await ctx.api.post<Report>("/api/run", {
        targets: targets.split(",").map((s) => s.trim()).filter(Boolean),
        max_evals: maxEvals ? Number(maxEvals) : undefined,
      });
      ctx.setReport(r);
      setOpen(false);
    } catch (x) {
      setErr(String(x instanceof Error ? x.message : x));
    } finally {
      setBusy(false);
    }
  };
  return (
    <div className="run-panel">
      <button type="button" onClick={() => setOpen((o) => !o)} aria-expanded={open}>
        Re-run…
      </button>
      {open && (
        <form className="card popover" onSubmit={go} aria-label="Re-run the pipeline">
          <label className="stack">
            Targets (comma-separated)
            <input value={targets} onChange={(e) => setTargets(e.target.value)} />
          </label>
          <label className="stack">
            Equation-search budget (evaluations, blank = default)
            <input type="number" min={1000} value={maxEvals} onChange={(e) => setMaxEvals(e.target.value)} />
          </label>
          <button type="submit" disabled={busy}>
            {busy ? "Running…" : "Run"}
          </button>
          {err && (
            <p className="error" role="alert">
              {err}
            </p>
          )}
          <ul className="small progress" aria-live="polite">
            {msgs.map((m, i) => (
              <li key={i}>{m.message}</li>
            ))}
          </ul>
        </form>
      )}
    </div>
  );
}
