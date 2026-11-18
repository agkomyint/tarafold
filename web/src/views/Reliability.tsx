import { Bars } from "../charts/charts";
import { Figure } from "../charts/common";
import { EvidenceBadge, GradeBadge, Notice } from "../components";
import { useApp } from "../ctx";
import { fmt, pct } from "../format";
import { selectedEquation } from "../model";

export function Reliability() {
  const { report, target: t } = useApp();
  const v = t.validate;
  const eq = selectedEquation(t);
  const st = v.stability;
  return (
    <div className="view">
      <section className="card" aria-labelledby="g-h">
        <h2 id="g-h">
          Selected equation for {t.target} <GradeBadge grade={v.reliability.grade} /> <EvidenceBadge evidence={v.evidence} />
        </h2>
        {eq && <p className="mono">{t.target} = {eq.infix}</p>}
        <p className="small">{v.evidence_basis}</p>
        <h3>Checklist</h3>
        <ul className="checklist">
          {v.reliability.checks.map((c) => (
            <li key={c.id} className={c.passed === false ? "fail" : c.passed ? "pass" : "na"}>
              <span aria-hidden="true">{c.passed === false ? "✗" : c.passed ? "✓" : "–"}</span>{" "}
              <span className="sr-only">{c.passed === false ? "failed:" : c.passed ? "passed:" : "not checked:"}</span>
              {c.condition}
            </li>
          ))}
        </ul>
      </section>
      <div className="grid">
        <section className="card" aria-labelledby="s-h">
          <h2 id="s-h">Stability and intervals</h2>
          <dl className="facts">
            <dt>Structural stability</dt>
            <dd>
              {st
                ? `${st.equivalent} of ${st.resamples} resampled rediscoveries found an equivalent equation (${pct(st.fraction)})`
                : "not computed"}
            </dd>
            <dt>90% interval</dt>
            <dd>
              ± {fmt(v.interval_halfwidth_90)} ({v.interval_method})
            </dd>
            <dt>Test coverage of the 90% interval</dt>
            <dd>{pct(v.test_coverage_90, 1)}</dd>
            <dt>Residual SD (validation)</dt>
            <dd>{fmt(v.residual_sd)}</dd>
            <dt>Residual trend</dt>
            <dd>
              correlation with prediction {fmt(v.residuals.residual_vs_prediction, 3)}; |residual| vs prediction{" "}
              {fmt(v.residuals.abs_residual_vs_prediction, 3)}
              {v.residuals.heteroscedastic && " — spread changes with the prediction (heteroscedastic)"}
            </dd>
          </dl>
        </section>
        <Figure
          title="Compared with baselines"
          description="test R²; the equation should match or beat flexible models"
          table={{
            headers: ["model", "R² test"],
            rows: [
              ["selected equation", eq?.metrics.test.r2 ?? NaN],
              ...t.relate.baselines.map((b) => [b.model, b.test_r2 ?? NaN] as [string, number]),
            ],
          }}
        >
          <Bars
            label="Test R² of the selected equation and baseline models"
            items={[
              { name: "selected equation", value: Math.max(0, eq?.metrics.test.r2 ?? 0) },
              ...t.relate.baselines.map((b) => ({ name: b.model, value: Math.max(0, b.test_r2 ?? 0), muted: true })),
            ]}
          />
        </Figure>
      </div>
      {v.residuals.largest.length > 0 && (
        <section className="card" aria-labelledby="lr-h">
          <h2 id="lr-h">Largest test residuals</h2>
          <table>
            <thead>
              <tr>
                <th scope="col">row</th>
                <th scope="col">actual</th>
                <th scope="col">predicted</th>
                <th scope="col">residual</th>
              </tr>
            </thead>
            <tbody>
              {v.residuals.largest.map((r) => (
                <tr key={r.row}>
                  <td>{r.row}</td>
                  <td>{fmt(r.actual)}</td>
                  <td>{fmt(r.predicted)}</td>
                  <td>{fmt((r.actual ?? NaN) - (r.predicted ?? NaN))}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </section>
      )}
      <section className="card" aria-labelledby="w2-h">
        <h2 id="w2-h">Warnings</h2>
        {report.warnings.filter((w) => !w.target || w.target === t.target).length === 0 ? (
          <p className="muted">None.</p>
        ) : (
          <ul>
            {report.warnings
              .filter((w) => !w.target || w.target === t.target)
              .map((w, i) => (
                <li key={i}>
                  <span className="badge badge-medium">{w.code}</span> {w.message}
                </li>
              ))}
          </ul>
        )}
        {report.notes.map((n, i) => (
          <Notice key={i}>{n}</Notice>
        ))}
      </section>
    </div>
  );
}
