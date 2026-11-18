import { useCallback, useMemo, useState } from "react";
import { Bars, ParallelCoords, Scatter } from "../charts/charts";
import { Figure } from "../charts/common";
import { DomainBadge, EvidenceBadge, Notice } from "../components";
import { useApp } from "../ctx";
import { compileTree } from "../expr/eval";
import { fmt, pct } from "../format";
import { domainMedians, domainRange, selectedEquation } from "../model";
import type { CandidateReport, Report } from "../report/schema";

/** Request body for `POST /api/optimize` and `/api/suggest` (mirrors the CLI goal flags). */
export interface GoalRequest {
  goal?: string;
  require: string[];
  prefer: string[];
  control: string[];
  bounds: Record<string, [number, number]>;
  constrain: string[];
  robust?: number;
  lexicographic: boolean;
}

export function GoalBuilder({
  onSubmit,
  busy,
  submitLabel,
}: {
  onSubmit: (g: GoalRequest) => void;
  busy: boolean;
  submitLabel: string;
}) {
  const { report, target: t } = useApp();
  const o = report.optimize;
  const prev = o?.goals ?? [];
  const [require, setRequire] = useState(prev.filter((g) => g.hard).map((g) => g.text).join("\n"));
  const [prefer, setPrefer] = useState(prev.filter((g) => !g.hard).map((g) => g.text).join("\n"));
  const [constrain, setConstrain] = useState((report.run.config.optimize?.constraints ?? []).join("\n"));
  const [robust, setRobust] = useState("");
  const [lex, setLex] = useState(false);
  const feats = t.features;
  const [control, setControl] = useState<string[]>(o?.controllables ?? selectedEquation(t)?.variables ?? []);
  const [bounds, setBounds] = useState<Record<string, [number, number]>>(() => {
    const b: Record<string, [number, number]> = {};
    for (const f of feats) {
      const ob = o?.bounds[f];
      const r = domainRange(t, f);
      if (ob && ob[0] !== null && ob[1] !== null) b[f] = [ob[0], ob[1]];
      else if (r) b[f] = r;
    }
    return b;
  });
  const lines = (s: string) =>
    s
      .split("\n")
      .map((x) => x.trim())
      .filter(Boolean);
  const submit = (e: React.FormEvent) => {
    e.preventDefault();
    const usedBounds = Object.fromEntries(Object.entries(bounds).filter(([k]) => control.includes(k)));
    onSubmit({
      require: lines(require),
      prefer: lines(prefer),
      constrain: lines(constrain),
      control,
      bounds: usedBounds,
      robust: robust ? Number(robust) : undefined,
      lexicographic: lex,
    });
  };
  return (
    <form className="card goal-builder" onSubmit={submit} aria-label="Goal builder">
      <h2>Goals</h2>
      <div className="grid">
        <label className="stack">
          Hard goals, one per line (<code>profit&gt;=5000</code>, <code>y=10</code>, <code>y=8..12</code>)
          <textarea rows={3} value={require} onChange={(e) => setRequire(e.target.value)} />
        </label>
        <label className="stack">
          Preferences, one per line (<code>max:revenue</code>, <code>min:cost:2</code>)
          <textarea rows={3} value={prefer} onChange={(e) => setPrefer(e.target.value)} />
        </label>
        <label className="stack">
          Linear constraints, one per line (<code>a + b &lt;= 100</code>)
          <textarea rows={3} value={constrain} onChange={(e) => setConstrain(e.target.value)} />
        </label>
      </div>
      <div className="controls">
        <label>
          Joint probability at least{" "}
          <input type="number" min={0} max={1} step={0.05} value={robust} onChange={(e) => setRobust(e.target.value)} placeholder="off" />
        </label>
        <label className="check">
          <input type="checkbox" checked={lex} onChange={(e) => setLex(e.target.checked)} /> rank preferences in order
          (lexicographic)
        </label>
      </div>
      <fieldset>
        <legend>Controllable inputs and bounds</legend>
        <div className="bounds">
          {feats.map((f) => {
            const r = domainRange(t, f) ?? [0, 1];
            const b = bounds[f] ?? r;
            const on = control.includes(f);
            return (
              <div key={f} className="bound-row">
                <label className="check">
                  <input
                    type="checkbox"
                    checked={on}
                    onChange={() => setControl((c) => (on ? c.filter((x) => x !== f) : [...c, f]))}
                  />{" "}
                  <span className="mono">{f}</span>
                </label>
                <label>
                  from{" "}
                  <input
                    type="number"
                    disabled={!on}
                    value={b[0]}
                    step="any"
                    aria-label={`${f} lower bound`}
                    onChange={(e) => setBounds((x) => ({ ...x, [f]: [Number(e.target.value), b[1]] }))}
                  />
                </label>
                <label>
                  to{" "}
                  <input
                    type="number"
                    disabled={!on}
                    value={b[1]}
                    step="any"
                    aria-label={`${f} upper bound`}
                    onChange={(e) => setBounds((x) => ({ ...x, [f]: [b[0], Number(e.target.value)] }))}
                  />
                </label>
                <span className="small muted">observed {fmt(r[0])} … {fmt(r[1])}</span>
              </div>
            );
          })}
        </div>
      </fieldset>
      <button type="submit" disabled={busy || (lines(require).length === 0 && lines(prefer).length === 0)}>
        {busy ? "Running…" : submitLabel}
      </button>
    </form>
  );
}

function WhatIf() {
  const { target: t } = useApp();
  const eq = selectedEquation(t);
  const med = useMemo(() => domainMedians(t), [t]);
  const vars = eq?.variables ?? [];
  const [vals, setVals] = useState<Record<string, number>>(() =>
    Object.fromEntries(vars.map((v) => [v, Number((med[v] ?? 0).toPrecision(4))])),
  );
  if (!eq) return null;
  const f = compileTree(eq.tree, vars);
  const y = f(vars.map((v) => vals[v] ?? NaN));
  const hw = t.validate.interval_halfwidth_90 ?? NaN;
  return (
    <section className="card" aria-labelledby="wi-h">
      <h2 id="wi-h">What-if</h2>
      <p className="small muted">
        Evaluates the selected equation in your browser. Values outside the observed range are extrapolations.
      </p>
      {vars.map((v) => {
        const r = domainRange(t, v) ?? [0, 1];
        const span = r[1] - r[0] || 1;
        const val = vals[v] ?? 0;
        const outside = val < r[0] || val > r[1];
        return (
          <div key={v} className="slider-row">
            <label htmlFor={`wi-${v}`} className="mono">
              {v}
            </label>
            <input
              id={`wi-${v}`}
              type="range"
              min={r[0] - 0.25 * span}
              max={r[1] + 0.25 * span}
              step={span / 200}
              value={val}
              onChange={(e) => setVals((x) => ({ ...x, [v]: Number(e.target.value) }))}
            />
            <input
              type="number"
              step="any"
              value={val}
              aria-label={`${v} value`}
              onChange={(e) => setVals((x) => ({ ...x, [v]: Number(e.target.value) }))}
            />
            {outside && <span className="badge badge-low">outside observed range</span>}
          </div>
        );
      })}
      <p className="whatif-out" aria-live="polite">
        {t.target} ≈ <strong>{fmt(y, 5)}</strong> <span className="muted">(90% interval ± {fmt(hw, 3)})</span>
      </p>
    </section>
  );
}

function CandidateTable({ report, chosen, onChoose, filter }: {
  report: Report;
  chosen: number;
  onChoose: (i: number) => void;
  filter: Set<number> | null;
}) {
  const o = report.optimize!;
  const goals = o.goals ?? [];
  const targets = [...new Set(goals.map((g) => g.target))];
  return (
    <div className="table-wrap">
      <table>
        <thead>
          <tr>
            <th scope="col">#</th>
            {o.controllables.map((c) => (
              <th key={c} scope="col">
                {c}
              </th>
            ))}
            {targets.map((tg) => (
              <th key={tg} scope="col">
                predicted {tg}
              </th>
            ))}
            {goals.map((g) => (
              <th key={g.text} scope="col" title={g.hard ? "hard goal" : "preference"}>
                {g.text}
              </th>
            ))}
            <th scope="col">joint probability</th>
            <th scope="col">domain</th>
            <th scope="col">marks</th>
          </tr>
        </thead>
        <tbody>
          {o.candidates.map((c, i) =>
            filter && !filter.has(i) ? null : (
              <tr
                key={c.id}
                className={i === chosen ? "chosen" : ""}
                tabIndex={0}
                onClick={() => onChoose(i)}
                onKeyDown={(e) => (e.key === "Enter" || e.key === " ") && onChoose(i)}
                aria-selected={i === chosen}
              >
                <td>{c.id}</td>
                {o.controllables.map((k) => (
                  <td key={k}>
                    {fmt(c.inputs[k])}
                    {c.change_pct[k] !== null && c.change_pct[k] !== undefined && (
                      <span className="muted small"> ({(c.change_pct[k]! >= 0 ? "+" : "") + c.change_pct[k]!.toFixed(0)}%)</span>
                    )}
                  </td>
                ))}
                {targets.map((tg) => (
                  <td key={tg}>{fmt(c.predictions?.[tg] ?? (tg === o.target ? c.predicted : null))}</td>
                ))}
                {goals.map((g, k) => (
                  <td key={g.text} aria-label={c.goals_met?.[k] ? "met" : "not met"}>
                    {g.kind.type === "maximize" || g.kind.type === "minimize" ? "—" : c.goals_met?.[k] ? "✓" : "✗"}
                  </td>
                ))}
                <td>{pct(c.joint_probability)}</td>
                <td>
                  <DomainBadge inDomain={c.in_domain} />
                </td>
                <td className="small">{(c.marks ?? []).join(", ")}</td>
              </tr>
            ),
          )}
        </tbody>
      </table>
    </div>
  );
}

function TradeOff({ report, onFilter, filter }: { report: Report; onFilter: (s: Set<number> | null) => void; filter: Set<number> | null }) {
  const o = report.optimize!;
  const targets = useMemo(() => [...new Set((o.goals ?? []).map((g) => g.target))], [o.goals]);
  const rows = useMemo(
    () => o.candidates.map((c) => targets.map((tg) => c.predictions?.[tg] ?? NaN)),
    [o.candidates, targets],
  );
  if (targets.length < 2 || o.candidates.length < 2) return null;
  if (targets.length === 2) {
    return (
      <Figure title="Trade-off" description="predicted values of the candidates">
        <Scatter
          series={[{ x: rows.map((r) => r[0]!), y: rows.map((r) => r[1]!), radius: 4 }]}
          xLabel={targets[0]!}
          yLabel={targets[1]!}
          onBrush={onFilter}
          highlight={filter}
        />
      </Figure>
    );
  }
  return (
    <Figure title="Trade-off" description="drag on an axis to filter candidates; the table follows">
      <ParallelCoords axes={targets} rows={rows} onFilter={onFilter} highlight={filter} />
    </Figure>
  );
}

export function Target() {
  const { report, api, target: t, setReport } = useApp();
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState<string | null>(null);
  const [chosen, setChosen] = useState(0);
  const [filter, setFilter] = useState<Set<number> | null>(null);
  const onFilter = useCallback((s: Set<number> | null) => setFilter(s), []);
  const o = report.optimize;
  const run = async (g: GoalRequest) => {
    if (!api) return;
    setBusy(true);
    setErr(null);
    try {
      setReport(await api.post<Report>("/api/optimize", g));
      setChosen(0);
      setFilter(null);
    } catch (e) {
      setErr(String(e instanceof Error ? e.message : e));
    } finally {
      setBusy(false);
    }
  };
  const c: CandidateReport | undefined = o?.candidates[chosen];
  return (
    <div className="view">
      {api ? (
        <GoalBuilder onSubmit={run} busy={busy} submitLabel="Find inputs" />
      ) : (
        <Notice>
          This is a static report. Run <code>tarafold serve {"<report-dir>"}</code> to change goals here, or use{" "}
          <code>tarafold optimize</code> on the command line.
        </Notice>
      )}
      {err && (
        <p className="error" role="alert">
          {err}
        </p>
      )}
      <WhatIf />
      {o ? (
        <>
          <section className="card" aria-labelledby="cand-h">
            <h2 id="cand-h">
              Candidates <EvidenceBadge evidence={o.evidence} />
            </h2>
            <p className="small">
              {o.reached
                ? "At least one candidate meets every hard goal under the fitted equations."
                : "No candidate meets every hard goal."}{" "}
              Method: {o.method}. {o.evaluations.toLocaleString()} evaluations.
            </p>
            <Notice>{o.disclaimer}</Notice>
            {o.conflict && (
              <div className="conflict" role="alert">
                <strong>These goals cannot all hold together:</strong> {o.conflict.goals.join(" · ")}
                <ul>
                  {o.conflict.best_achievable.map(([g, v]) => (
                    <li key={String(g)}>
                      {String(g)}: best achievable while the others hold ≈ {fmt(v as number)}
                    </li>
                  ))}
                </ul>
              </div>
            )}
            {o.binding.length > 0 && <p className="small">Binding at the closest point: {o.binding.join(", ")}</p>}
            {(o.redundant_objectives ?? []).map((r) => (
              <p key={`${r.a}${r.b}`} className="small">
                <strong>{r.a}</strong> and <strong>{r.b}</strong> move together across the trade-off set (ρ = {fmt(r.rho, 3)}); one of
                them may be redundant.
              </p>
            ))}
            {o.unused_controllables.length > 0 && (
              <p className="small muted">Not used by the equation: {o.unused_controllables.join(", ")}</p>
            )}
            {filter && (
              <p className="small" aria-live="polite">
                Showing {filter.size} of {o.candidates.length} candidates.{" "}
                <button type="button" className="link" onClick={() => setFilter(null)}>
                  Clear filter
                </button>
              </p>
            )}
            <CandidateTable report={report} chosen={chosen} onChoose={setChosen} filter={filter} />
          </section>
          <div className="grid">
            <TradeOff report={report} onFilter={onFilter} filter={filter} />
            {c && Object.keys(c.sensitivity).length > 0 && (
              <Figure
                title={`Sensitivity of candidate #${c.id}`}
                description={`elasticity: % change in ${o.target} per 1% change in the input, at this point`}
                table={{
                  headers: ["input", "∂ŷ/∂x", "elasticity"],
                  rows: Object.keys(c.sensitivity).map((k) => [k, c.sensitivity[k] ?? NaN, c.elasticity[k] ?? NaN]),
                }}
              >
                <Bars
                  label="Elasticity per input"
                  items={Object.keys(c.elasticity).map((k) => ({ name: k, value: c.elasticity[k] ?? 0 }))}
                />
              </Figure>
            )}
          </div>
        </>
      ) : (
        <p className="muted">No optimization in this report. Give goals with --goal, --require or --prefer.</p>
      )}
      <p className="small muted">Target: {t.target}</p>
    </div>
  );
}
