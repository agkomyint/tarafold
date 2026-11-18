import { useMemo, useState } from "react";
import { Scatter, Surface } from "../charts/charts";
import { Figure } from "../charts/common";
import { DomainBadge, Notice } from "../components";
import { useApp } from "../ctx";
import { compileTree } from "../expr/eval";
import { fmt, pct } from "../format";
import { column, domainMedians, domainRange, download, toCsv } from "../model";
import type { Report } from "../report/schema";
import { GoalBuilder, type GoalRequest } from "./Target";

const REASONS: Record<string, string> = {
  uncertainty: "the equations are least certain here",
  equation_discrimination: "competing equations disagree most here",
  goal_likelihood: "likely to meet the goals",
  improvement: "may improve on the best observed trade-off",
};

export function Experiments() {
  const { report, data, api, target: t, setReport } = useApp();
  const s = report.suggest;
  const [mode, setMode] = useState(s?.mode ?? "balanced");
  const [n, setN] = useState(s?.suggestions.length || 8);
  const [expand, setExpand] = useState("0");
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState<string | null>(null);
  const ctrls = s?.controllables ?? [];
  const [ax, setAx] = useState(ctrls[0] ?? "");
  const [ay, setAy] = useState(ctrls[1] ?? t.target);

  const run = async (g?: GoalRequest) => {
    if (!api) return;
    setBusy(true);
    setErr(null);
    try {
      const body = { ...(g ?? { require: [], prefer: [], control: [], bounds: {}, constrain: [], lexicographic: false }), n, mode, expand: Number(expand) / 100 };
      setReport(await api.post<Report>("/api/suggest", body));
    } catch (e) {
      setErr(String(e instanceof Error ? e.message : e));
    } finally {
      setBusy(false);
    }
  };

  const csv = () => {
    if (!s) return;
    const cols = data?.columns ?? Object.keys(s.suggestions[0]?.row ?? {}).concat(report.targets.map((x) => x.target));
    const rows = s.suggestions.map((x) => cols.map((c) => x.row[c] ?? null));
    download("suggestions.csv", toCsv(cols, rows));
  };

  const xs = column(data, ax);
  const ys = column(data, ay);
  const series = useMemo(() => {
    if (!s) return [];
    const sx = s.suggestions.map((x) => x.row[ax] ?? x.predictions[ax] ?? NaN);
    const sy = s.suggestions.map((x) => x.row[ay] ?? x.predictions[ay] ?? NaN);
    return [
      ...(xs && ys ? [{ x: xs, y: ys, color: "#8a96a3" }] : [{ x: [], y: [] }]),
      { x: sx, y: sy, color: "#D55E00", label: "suggestions" },
    ];
  }, [s, xs, ys, ax, ay]);

  return (
    <div className="view">
      {api ? (
        <section className="card" aria-labelledby="sg-h">
          <h2 id="sg-h">Suggest next experiments</h2>
          <div className="controls">
            <label>
              Mode{" "}
              <select value={mode} onChange={(e) => setMode(e.target.value)}>
                <option value="balanced">balanced</option>
                <option value="explore">explore (reduce uncertainty)</option>
                <option value="exploit">exploit (aim at the goals)</option>
              </select>
            </label>
            <label>
              Batch size <input type="number" min={1} max={100} value={n} onChange={(e) => setN(Number(e.target.value))} />
            </label>
            <label>
              Beyond observed range (%) <input type="number" min={0} max={100} value={expand} onChange={(e) => setExpand(e.target.value)} />
            </label>
            <button type="button" disabled={busy} onClick={() => void run()}>
              {busy ? "Running…" : "Suggest (no goals)"}
            </button>
          </div>
          <details>
            <summary>Aim suggestions at goals</summary>
            <GoalBuilder onSubmit={(g) => void run(g)} busy={busy} submitLabel="Suggest" />
          </details>
          {err && (
            <p className="error" role="alert">
              {err}
            </p>
          )}
        </section>
      ) : (
        <Notice>
          This is a static report. Use <code>tarafold suggest {"<report-dir>"} -n 8</code> or <code>tarafold serve</code> to make
          new suggestions.
        </Notice>
      )}
      {s ? (
        <>
          <section className="card" aria-labelledby="sl-h">
            <h2 id="sl-h">Suggestions</h2>
            <p className="small">
              Mode {s.mode}: {s.exploit} aimed at goals, {s.explore} to reduce uncertainty.
            </p>
            <Notice>{s.note}</Notice>
            <button type="button" onClick={csv}>
              Download suggestions.csv
            </button>
            <div className="table-wrap">
              <table>
                <thead>
                  <tr>
                    <th scope="col">#</th>
                    {s.controllables.map((c) => (
                      <th key={c} scope="col">
                        {c}
                      </th>
                    ))}
                    {Object.keys(s.suggestions[0]?.predictions ?? {}).map((k) => (
                      <th key={k} scope="col">
                        predicted {k} (90% range)
                      </th>
                    ))}
                    <th scope="col">why</th>
                    <th scope="col">P(all hard goals)</th>
                    <th scope="col">domain</th>
                  </tr>
                </thead>
                <tbody>
                  {s.suggestions.map((x) => (
                    <tr key={x.id}>
                      <td>{x.id}</td>
                      {s.controllables.map((c) => (
                        <td key={c}>{fmt(x.inputs[c])}</td>
                      ))}
                      {Object.entries(x.predictions).map(([k, v]) => (
                        <td key={k}>
                          {fmt(v)}{" "}
                          <span className="muted small">
                            [{fmt(x.intervals[k]?.[0])}, {fmt(x.intervals[k]?.[1])}]
                          </span>
                        </td>
                      ))}
                      <td className="small">
                        {x.mode}: {REASONS[x.reason] ?? x.reason}
                      </td>
                      <td>{pct(x.joint_probability)}</td>
                      <td>
                        <DomainBadge inDomain={x.in_domain} />
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          </section>
          {ctrls.length >= 1 && (
            <section className="card" aria-labelledby="ov-h">
              <h2 id="ov-h">Suggestions over existing data</h2>
              <div className="controls">
                <label>
                  x{" "}
                  <select value={ax} onChange={(e) => setAx(e.target.value)}>
                    {[...ctrls, ...report.targets.map((x) => x.target)].map((c) => (
                      <option key={c}>{c}</option>
                    ))}
                  </select>
                </label>
                <label>
                  y{" "}
                  <select value={ay} onChange={(e) => setAy(e.target.value)}>
                    {[...ctrls, ...report.targets.map((x) => x.target)].map((c) => (
                      <option key={c}>{c}</option>
                    ))}
                  </select>
                </label>
                <span className="small muted">grey: existing rows · orange diamonds: suggestions</span>
              </div>
              <Scatter series={series} xLabel={ax} yLabel={ay} width={600} height={360} />
            </section>
          )}
          <Disagreement />
        </>
      ) : (
        <p className="muted">No suggestions in this report. Add --suggest N to tarafold run.</p>
      )}
    </div>
  );
}

/** Where the competing equations of the current target disagree, over two chosen inputs. */
function Disagreement() {
  const { report, target: t } = useApp();
  const s = report.suggest!;
  const structs = s.structures[t.target] as [string, number][] | undefined;
  const alts = useMemo(
    () =>
      (structs ?? [])
        .map(([infix, w]) => ({ eq: t.discover.front.find((e) => e.infix === infix), w }))
        .filter((a) => a.eq !== undefined),
    [structs, t],
  );
  const vars = useMemo(() => [...new Set(alts.flatMap((a) => a.eq!.variables))], [alts]);
  const [vx, setVx] = useState(vars[0] ?? "");
  const [vy, setVy] = useState(vars[1] ?? "");
  const med = useMemo(() => domainMedians(t), [t]);
  const fns = useMemo(
    () => alts.map((a) => ({ f: compileTree(a.eq!.tree, vars), w: a.w })),
    [alts, vars],
  );
  const fn = useMemo(() => {
    const xi = vars.indexOf(vx);
    const yi = vars.indexOf(vy);
    const sw = fns.reduce((a, b) => a + b.w, 0) || 1;
    const base = vars.map((v) => med[v] ?? NaN);
    return (a: number, b: number) => {
      const x = Float64Array.from(base);
      x[xi] = a;
      x[yi] = b;
      const ys = fns.map((g) => g.f(x));
      const mean = ys.reduce((acc, y, k) => acc + fns[k]!.w * y, 0) / sw;
      return Math.sqrt(ys.reduce((acc, y, k) => acc + fns[k]!.w * (y - mean) ** 2, 0) / sw);
    };
  }, [fns, vars, med, vx, vy]);
  if (alts.length < 2 || vars.length < 2) return null;
  const xr = domainRange(t, vx) ?? [0, 1];
  const yr = domainRange(t, vy) ?? [0, 1];
  const pts = {
    x: s.suggestions.map((x) => x.row[vx] ?? NaN),
    y: s.suggestions.map((x) => x.row[vy] ?? NaN),
  };
  return (
    <Figure
      title={`Where competing equations for ${t.target} disagree`}
      description="weighted standard deviation across the equations used for suggestions; other inputs at medians; dots: suggestions"
      table={{ headers: ["equation", "weight"], rows: alts.map((a) => [a.eq!.infix, a.w]) }}
    >
      <div className="controls">
        <label>
          x{" "}
          <select value={vx} onChange={(e) => setVx(e.target.value)}>
            {vars.map((v) => (
              <option key={v}>{v}</option>
            ))}
          </select>
        </label>
        <label>
          y{" "}
          <select value={vy} onChange={(e) => setVy(e.target.value)}>
            {vars.map((v) => (
              <option key={v}>{v}</option>
            ))}
          </select>
        </label>
      </div>
      <Surface fn={fn} xDomain={xr} yDomain={yr} xLabel={vx} yLabel={vy} zLabel="disagreement" points={pts} />
    </Figure>
  );
}
