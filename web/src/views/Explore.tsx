import { useCallback, useMemo, useState } from "react";
import { Bars, Heatmap, Scatter } from "../charts/charts";
import { Figure } from "../charts/common";
import { useApp } from "../ctx";
import { fmt, median } from "../format";
import { column } from "../model";

export function Explore() {
  const { data, target: t } = useApp();
  const names = data?.columns ?? [];
  const ranked = [...t.relate.features].sort((a, b) => (b.importance ?? 0) - (a.importance ?? 0));
  const [vars, setVars] = useState<string[]>(() =>
    [t.target, ...ranked.map((f) => f.feature)].filter((n) => names.includes(n)).slice(0, 4),
  );
  const [xName, setX] = useState<string>(ranked[0]?.feature ?? names[0] ?? "");
  const [yName, setY] = useState<string>(t.target);
  const [sel, setSel] = useState<Set<number> | null>(null);
  const [live, setLive] = useState<Set<number> | null>(null);
  // The main scatter follows the brush every frame; the matrix and statistics update when the
  // brush is released.
  const onBrush = useCallback((s: Set<number> | null, final: boolean) => {
    const v = s && s.size > 0 ? s : null;
    setLive(v);
    if (final) setSel(v);
  }, []);
  const xs = useMemo(() => column(data, xName), [data, xName]);
  const ys = useMemo(() => column(data, yName), [data, yName]);
  const series = useMemo(() => (xs && ys ? [{ x: xs, y: ys }] : []), [xs, ys]);
  const cols = useMemo(() => Object.fromEntries(vars.map((v) => [v, column(data, v) ?? []])), [data, vars]);
  const fnames = t.relate.features.map((f) => f.feature);

  const toggle = (v: string) =>
    setVars((cur) => (cur.includes(v) ? cur.filter((c) => c !== v) : cur.length >= 6 ? cur : [...cur, v]));

  return (
    <div className="view">
      {data ? (
        <>
          <section className="card" aria-labelledby="sc-h">
            <h2 id="sc-h">Scatter with brushing</h2>
            <div className="controls">
              <label>
                x{" "}
                <select value={xName} onChange={(e) => setX(e.target.value)}>
                  {names.map((n) => (
                    <option key={n}>{n}</option>
                  ))}
                </select>
              </label>
              <label>
                y{" "}
                <select value={yName} onChange={(e) => setY(e.target.value)}>
                  {names.map((n) => (
                    <option key={n}>{n}</option>
                  ))}
                </select>
              </label>
              <span className="muted small">Drag on the chart to select rows; the matrix below highlights them.</span>
            </div>
            <Scatter series={series} xLabel={xName} yLabel={yName} width={640} height={380} onBrush={onBrush} highlight={live} />
            {sel && (
              <p className="small" aria-live="polite">
                {sel.size.toLocaleString()} rows selected.{" "}
                {vars.map((v) => {
                  const c = cols[v] ?? [];
                  const vals = [...sel].map((i) => c[i]!).filter(Number.isFinite);
                  return (
                    <span key={v} className="mono">
                      {v} median {fmt(median(vals))};{" "}
                    </span>
                  );
                })}
              </p>
            )}
          </section>
          <section className="card" aria-labelledby="sm-h">
            <h2 id="sm-h">Scatter matrix</h2>
            <fieldset className="controls">
              <legend className="small">Variables (up to 6)</legend>
              {names.map((n) => (
                <label key={n} className="check">
                  <input type="checkbox" checked={vars.includes(n)} onChange={() => toggle(n)} /> {n}
                </label>
              ))}
            </fieldset>
            <div className="matrix" style={{ gridTemplateColumns: `90px repeat(${vars.length}, 130px)` }}>
              <div />
              {vars.map((v) => (
                <div key={`h${v}`} className="matrix-label">
                  {v}
                </div>
              ))}
              {vars.map((r) => (
                <MatrixRow key={r} r={r} vars={vars} cols={cols} sel={sel} />
              ))}
            </div>
          </section>
        </>
      ) : (
        <p className="muted">No data sample is embedded in this report.</p>
      )}
      <div className="grid">
        <Figure
          title="Correlation between features"
          description="Pearson r"
          table={{
            headers: ["feature", ...fnames],
            rows: t.relate.feature_correlation.map((row, i) => [fnames[i] ?? "", ...row.map((v) => v ?? NaN)]),
          }}
        >
          <Heatmap names={fnames} values={t.relate.feature_correlation} label="Feature correlation heatmap" />
        </Figure>
        <Figure
          title={`Association with ${t.target}`}
          description="mutual information as R² equivalent; association only"
          table={{
            headers: ["feature", "pearson", "spearman", "MI r²", "dCor"],
            rows: t.relate.features.map((f) => [
              f.feature,
              f.pearson ?? NaN,
              f.spearman ?? NaN,
              f.mi_r2 ?? NaN,
              f.distance_correlation ?? NaN,
            ]),
          }}
        >
          <Bars
            label={`Mutual information with ${t.target}`}
            items={t.relate.features.map((f) => ({ name: f.feature, value: f.mi_r2 ?? 0 }))}
          />
        </Figure>
      </div>
    </div>
  );
}

function MatrixRow({
  r,
  vars,
  cols,
  sel,
}: {
  r: string;
  vars: string[];
  cols: Record<string, number[]>;
  sel: Set<number> | null;
}) {
  return (
    <>
      <div className="matrix-label">{r}</div>
      {vars.map((c) => (
        <MatrixCell key={c} x={cols[c] ?? []} y={cols[r] ?? []} xl={c} yl={r} sel={sel} />
      ))}
    </>
  );
}

function MatrixCell({ x, y, xl, yl, sel }: { x: number[]; y: number[]; xl: string; yl: string; sel: Set<number> | null }) {
  const series = useMemo(() => [{ x, y }], [x, y]);
  return <Scatter series={series} xLabel={xl} yLabel={yl} width={130} height={130} compact highlight={sel} />;
}
