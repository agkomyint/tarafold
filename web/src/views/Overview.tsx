import { Histogram } from "../charts/charts";
import { Figure } from "../charts/common";
import { Notice } from "../components";
import { useApp } from "../ctx";
import { finite, fmt } from "../format";

export function Overview() {
  const { report, data } = useApp();
  const d = report.dataset;
  return (
    <div className="view">
      <section className="card" aria-labelledby="ds-h">
        <h2 id="ds-h">Dataset</h2>
        <dl className="facts">
          <dt>Source</dt>
          <dd className="mono">{d.source.path}</dd>
          <dt>Rows</dt>
          <dd>
            {d.n_rows.toLocaleString()} used of {d.source.rows_read.toLocaleString()} read
            {d.source.rows_dropped_missing > 0 && ` (${d.source.rows_dropped_missing} dropped for missing values)`}
          </dd>
          <dt>Numeric columns</dt>
          <dd>{d.columns.length}</dd>
          <dt>Data source</dt>
          <dd>{d.data_source}</dd>
          <dt>Row weights</dt>
          <dd>{d.weighted ? "yes" : "no"}</dd>
          <dt>SHA-256</dt>
          <dd className="mono small">{d.source.sha256}</dd>
          <dt>Targets</dt>
          <dd>{report.targets.map((t) => t.target).join(", ")}</dd>
          <dt>Run</dt>
          <dd className="mono small">
            {report.run.id} · Tarafold {report.tarafold_version} · engine {report.run.engine} · {report.run.status}
          </dd>
          {data && (
            <>
              <dt>Data in this view</dt>
              <dd>
                {data.sampled
                  ? `${(data.values[0]?.length ?? 0).toLocaleString()} sampled rows of ${data.rows_total.toLocaleString()}`
                  : `all ${data.rows_total.toLocaleString()} rows`}
              </dd>
            </>
          )}
        </dl>
        {d.source.excluded.length > 0 && (
          <>
            <h3>Excluded columns</h3>
            <ul>
              {d.source.excluded.map((e) => (
                <li key={e.name}>
                  <span className="mono">{e.name}</span>: {e.reason}
                </li>
              ))}
            </ul>
          </>
        )}
        {report.notes.map((n, i) => (
          <Notice key={i}>{n}</Notice>
        ))}
        {!data && (
          <p className="muted small">
            No data sample is embedded, so data-based charts (scatter plots, residuals) are hidden.
          </p>
        )}
      </section>
      {report.warnings.length > 0 && (
        <section className="card" aria-labelledby="w-h">
          <h2 id="w-h">Warnings</h2>
          <ul>
            {report.warnings.map((w, i) => (
              <li key={i}>
                <span className="badge badge-medium">{w.code}</span> {w.target && <strong>{w.target}: </strong>}
                {w.message}
              </li>
            ))}
          </ul>
        </section>
      )}
      <section aria-labelledby="var-h">
        <h2 id="var-h">Variables</h2>
        <div className="grid">
          {d.profile.map((p) => {
            const flags = [
              p.integer_valued && "integer",
              p.bounded_unit && "in [0, 1]",
              p.all_positive && "positive",
              p.outliers > 0 && `${p.outliers} outliers`,
            ].filter(Boolean) as string[];
            return (
              <Figure
                key={p.name}
                title={p.name}
                description={flags.join(", ") || undefined}
                table={{
                  headers: ["bin from", "bin to", "count"],
                  rows: p.histogram.counts.map((c, i) => [
                    p.histogram.edges[i] ?? NaN,
                    p.histogram.edges[i + 1] ?? NaN,
                    c,
                  ]),
                }}
              >
                <Histogram edges={finite(p.histogram.edges)} counts={p.histogram.counts} label={p.name} />
                <p className="small muted">
                  min {fmt(p.min)} · median {fmt(p.quantiles.p50)} · max {fmt(p.max)} · std {fmt(p.std)} · {p.unique}{" "}
                  unique
                </p>
              </Figure>
            );
          })}
        </div>
      </section>
    </div>
  );
}
