import { Bars, Heatmap } from "../charts/charts";
import { Figure } from "../charts/common";
import { useApp } from "../ctx";
import { fmt } from "../format";

export function Relationships() {
  const { target: t } = useApp();
  const r = t.relate;
  const imp = [...r.features].sort((a, b) => (b.importance ?? 0) - (a.importance ?? 0));
  const inter = [...new Set(r.interactions.flatMap((i) => [i.a, i.b]))];
  const h2 = inter.map((a) =>
    inter.map((b) => {
      if (a === b) return null;
      const hit = r.interactions.find((i) => (i.a === a && i.b === b) || (i.a === b && i.b === a));
      return hit?.h2 ?? null;
    }),
  );
  return (
    <div className="view">
      <p className="muted">
        These measure association in this data, not the effect of changing a feature.
      </p>
      <div className="grid">
        <Figure
          title="Permutation importance"
          description="drop in validation R² of a gradient-boosted model when a feature is shuffled (± std)"
          table={{
            headers: ["feature", "importance", "std"],
            rows: imp.map((f) => [f.feature, f.importance ?? NaN, f.importance_std ?? NaN]),
          }}
        >
          <Bars
            label="Permutation importance"
            items={imp.map((f) => ({
              name: f.feature,
              value: f.importance ?? 0,
              err: f.importance_std ?? undefined,
              muted: !t.features.includes(f.feature),
            }))}
          />
        </Figure>
        {inter.length >= 2 && (
          <Figure
            title="Interactions"
            description="Friedman H² (0 = additive, 1 = pure interaction)"
            table={{ headers: ["a", "b", "H²"], rows: r.interactions.map((i) => [i.a, i.b, i.h2 ?? NaN]) }}
          >
            <Heatmap names={inter} values={h2} label="Interaction strength heatmap" diverging={false} />
          </Figure>
        )}
      </div>
      <section className="card" aria-labelledby="scr-h">
        <h2 id="scr-h">Screening</h2>
        <p className="small muted">Which features were passed to equation search, and why.</p>
        <div className="table-wrap">
          <table>
            <thead>
              <tr>
                <th scope="col">feature</th>
                <th scope="col">passed to search</th>
                <th scope="col">score</th>
                <th scope="col">reason</th>
                <th scope="col">VIF</th>
              </tr>
            </thead>
            <tbody>
              {t.screening.map((s) => (
                <tr key={s.feature}>
                  <td className="mono">{s.feature}</td>
                  <td>{s.kept ? "yes" : "no"}</td>
                  <td>{fmt(s.score, 3)}</td>
                  <td>{s.reason}</td>
                  <td>{fmt(r.features.find((f) => f.feature === s.feature)?.vif ?? NaN, 3)}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
        {r.collinear.length > 0 && (
          <>
            <h3>Collinear pairs</h3>
            <ul>
              {r.collinear.map((p) => (
                <li key={`${p.a}-${p.b}`}>
                  {p.a} ~ {p.b} (r = {fmt(p.r, 3)})
                </li>
              ))}
            </ul>
          </>
        )}
        {r.leakage.length > 0 && (
          <>
            <h3>Possible leakage</h3>
            <ul>
              {r.leakage.map((l) => (
                <li key={l}>
                  <span className="mono">{l}</span> has |r| &gt; 0.999 with the target.
                </li>
              ))}
            </ul>
          </>
        )}
      </section>
      <section className="card" aria-labelledby="bl-h">
        <h2 id="bl-h">Baseline models</h2>
        <table>
          <thead>
            <tr>
              <th scope="col">model</th>
              <th scope="col">R² validation</th>
              <th scope="col">R² test</th>
              <th scope="col">detail</th>
            </tr>
          </thead>
          <tbody>
            {r.baselines.map((b) => (
              <tr key={b.model}>
                <td>{b.model}</td>
                <td>{fmt(b.val_r2, 4)}</td>
                <td>{fmt(b.test_r2, 4)}</td>
                <td className="small">{b.detail}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </section>
    </div>
  );
}
