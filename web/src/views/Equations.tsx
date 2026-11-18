import * as d3 from "d3";
import { useMemo, useState } from "react";
import { LineChart, Scatter, Surface } from "../charts/charts";
import { Axis, Figure, M } from "../charts/common";
import { EquationCard } from "../components";
import { useApp } from "../ctx";
import { compileTree } from "../expr/eval";
import { fmt, PALETTE } from "../format";
import { column, domainMedians, domainRange, ensembleTrees, predictRows, quantile } from "../model";
import type { EquationReport, TargetReport } from "../report/schema";

function loss(e: EquationReport) {
  return Math.max(1e-12, 1 - (e.metrics.val.r2 ?? -Infinity));
}

function Pareto({ t, shown, onPick }: { t: TargetReport; shown: number; onPick: (id: number) => void }) {
  const w = 420;
  const h = 260;
  const iw = w - M.left - M.right;
  const ih = h - M.top - M.bottom;
  const front = t.discover.front;
  const x = d3
    .scaleLinear()
    .domain([0, d3.max(front, (e) => e.complexity) ?? 1])
    .nice()
    .range([0, iw]);
  const ls = front.map(loss).filter(Number.isFinite);
  const y = d3
    .scaleLog()
    .domain([Math.max(1e-12, d3.min(ls) ?? 1e-6), Math.max(1, d3.max(ls) ?? 1)])
    .range([ih, 0])
    .clamp(true);
  return (
    <svg width={w} height={h} role="img" aria-label="Pareto front: validation loss against complexity">
      <g transform={`translate(${M.left},${M.top})`}>
        <path
          d={front.map((e, i) => `${i ? "L" : "M"}${x(e.complexity)},${y(loss(e))}`).join("")}
          fill="none"
          stroke="#9aa5b1"
        />
        {front.map((e) => {
          const sel = e.id === t.discover.selected_id;
          const on = e.id === shown;
          return (
            <circle
              key={e.id}
              cx={x(e.complexity)}
              cy={y(loss(e))}
              r={on ? 8 : 6}
              fill={sel ? PALETTE[5] : PALETTE[0]}
              stroke={on ? "#000" : "#fff"}
              strokeWidth={2}
              style={{ cursor: "pointer" }}
              onClick={() => onPick(e.id)}
            >
              <title>{`#${e.id}: ${e.infix} (1 − R² val = ${fmt(loss(e), 3)})`}</title>
            </circle>
          );
        })}
        <g transform={`translate(0,${ih})`}>
          <Axis scale={x} orient="bottom" length={iw} label="complexity" />
        </g>
        <g className="axis" aria-hidden="true">
          <line y2={ih} />
          {y.ticks(4).map((tk) => (
            <g key={tk} transform={`translate(0,${y(tk)})`}>
              <line x2={-5} />
              <text x={-8} dy="0.32em" textAnchor="end">
                {tk.toExponential(0)}
              </text>
            </g>
          ))}
          <text className="axis-label" transform={`translate(-44,${ih / 2}) rotate(-90)`} textAnchor="middle">
            1 − R² (validation)
          </text>
        </g>
      </g>
    </svg>
  );
}

export function Equations() {
  const { target: t, data, shownEquation, setShownEquation } = useApp();
  const eq = t.discover.front.find((e) => e.id === shownEquation) ?? t.discover.front[0];
  const vars = eq?.variables ?? [];
  const [sx, setSx] = useState(vars[0] ?? "");
  const [sy, setSy] = useState(vars[1] ?? vars[0] ?? "");
  const actual = useMemo(() => column(data, t.target), [data, t.target]);
  const pred = useMemo(() => (eq ? predictRows(eq, data) : null), [eq, data]);
  const residual = useMemo(
    () => (pred && actual ? pred.map((p, i) => actual[i]! - p) : null),
    [pred, actual],
  );
  const med = useMemo(() => domainMedians(t), [t]);
  const isSelected = eq?.id === t.discover.selected_id;
  const members = useMemo(() => (isSelected ? ensembleTrees(t) : []), [t, isSelected]);

  if (!eq) return <p>No equations on the front.</p>;
  const f = compileTree(eq.tree, vars);
  const base = vars.map((v) => med[v] ?? NaN);
  const memberFns = members.map((m) => compileTree(m, vars));
  const xSurf = vars.includes(sx) ? sx : vars[0]!;
  const ySurf = vars.includes(sy) && sy !== xSurf ? sy : (vars.find((v) => v !== xSurf) ?? xSurf);

  return (
    <div className="view">
      <div className="grid">
        <Figure
          title="Pareto front"
          description="click a point (or use the list) to inspect an equation; orange = selected"
          table={{
            headers: ["id", "complexity", "R² val", "R² test", "equation"],
            rows: t.discover.front.map((e) => [e.id, e.complexity, e.metrics.val.r2 ?? NaN, e.metrics.test.r2 ?? NaN, e.infix]),
          }}
        >
          <Pareto t={t} shown={eq.id} onPick={setShownEquation} />
          <div className="eq-list" role="listbox" aria-label="Equations on the front">
            {t.discover.front.map((e) => (
              <button
                key={e.id}
                role="option"
                aria-selected={e.id === eq.id}
                className={e.id === eq.id ? "active" : ""}
                onClick={() => setShownEquation(e.id)}
              >
                #{e.id} · c{e.complexity} · {e.infix.length > 48 ? `${e.infix.slice(0, 47)}…` : e.infix}
                {e.id === t.discover.selected_id ? " (selected)" : ""}
              </button>
            ))}
          </div>
        </Figure>
        <EquationCard t={t} eq={eq} />
      </div>
      {pred && actual && residual && (
        <div className="grid">
          <Figure title="Predicted vs actual" description="all rows in the data sample; dashed line = perfect fit">
            <Scatter series={[{ x: pred, y: actual }]} xLabel={`predicted ${t.target}`} yLabel={`actual ${t.target}`} diagonal />
          </Figure>
          <Figure title="Residuals" description="actual − predicted against predicted">
            <Scatter series={[{ x: pred, y: residual }]} xLabel={`predicted ${t.target}`} yLabel="residual" />
          </Figure>
        </div>
      )}
      <section aria-labelledby="pd-h">
        <h2 id="pd-h">Partial dependence</h2>
        <p className="small muted">
          Each curve varies one input with the others at their training medians. Shaded: observed range
          {isSelected && members.length > 0 ? "; band: 90% range of the bootstrap refits" : ""}.
        </p>
        <div className="grid">
          {vars.map((v, j) => {
            const r = domainRange(t, v) ?? [0, 1];
            const span = r[1] - r[0] || 1;
            const lo = r[0] - 0.25 * span;
            const hi = r[1] + 0.25 * span;
            const xs = d3.range(0, 81).map((k) => lo + ((hi - lo) * k) / 80);
            const ys: number[] = [];
            const blo: number[] = [];
            const bhi: number[] = [];
            const x = Float64Array.from(base);
            for (const xv of xs) {
              x[j] = xv;
              ys.push(f(x));
              if (memberFns.length > 1) {
                const vs = memberFns.map((g) => g(x)).filter(Number.isFinite).sort((a, b) => a - b);
                blo.push(quantile(vs, 0.05));
                bhi.push(quantile(vs, 0.95));
              }
            }
            return (
              <Figure
                key={v}
                title={v}
                table={{ headers: [v, t.target], rows: xs.filter((_, k) => k % 8 === 0).map((xv, k) => [xv, ys[k * 8]!]) }}
              >
                <LineChart
                  xs={xs}
                  ys={ys}
                  band={memberFns.length > 1 ? { lo: blo, hi: bhi } : undefined}
                  xLabel={v}
                  yLabel={t.target}
                  observed={r}
                />
              </Figure>
            );
          })}
        </div>
      </section>
      {vars.length >= 2 && (
        <section className="card" aria-labelledby="rs-h">
          <h2 id="rs-h">Response surface</h2>
          <div className="controls">
            <label>
              x{" "}
              <select value={xSurf} onChange={(e) => setSx(e.target.value)}>
                {vars.map((v) => (
                  <option key={v}>{v}</option>
                ))}
              </select>
            </label>
            <label>
              y{" "}
              <select value={ySurf} onChange={(e) => setSy(e.target.value)}>
                {vars.map((v) => (
                  <option key={v}>{v}</option>
                ))}
              </select>
            </label>
            <span className="small muted">Other inputs at their medians; dots are observed rows.</span>
          </div>
          <SurfaceFor t={t} eqVars={vars} f={f} base={base} xv={xSurf} yv={ySurf} />
        </section>
      )}
    </div>
  );
}

function SurfaceFor({
  t,
  eqVars,
  f,
  base,
  xv,
  yv,
}: {
  t: TargetReport;
  eqVars: string[];
  f: (x: ArrayLike<number>) => number;
  base: number[];
  xv: string;
  yv: string;
}) {
  const { data } = useApp();
  const xi = eqVars.indexOf(xv);
  const yi = eqVars.indexOf(yv);
  const fn = useMemo(() => {
    return (a: number, b: number) => {
      const x = Float64Array.from(base);
      x[xi] = a;
      x[yi] = b;
      return f(x);
    };
  }, [f, base, xi, yi]);
  const xr = domainRange(t, xv) ?? [0, 1];
  const yr = domainRange(t, yv) ?? [0, 1];
  const px = column(data, xv);
  const py = column(data, yv);
  const step = px ? Math.max(1, Math.floor(px.length / 1500)) : 1;
  const points =
    px && py ? { x: px.filter((_, i) => i % step === 0), y: py.filter((_, i) => i % step === 0) } : undefined;
  return <Surface fn={fn} xDomain={xr} yDomain={yr} xLabel={xv} yLabel={yv} zLabel={t.target} points={points} />;
}
