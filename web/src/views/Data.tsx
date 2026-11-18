import { useMemo, useState } from "react";
import { useApp } from "../ctx";
import { fmt } from "../format";
import { download, toCsv } from "../model";

const PAGE = 100;

/** The input table as Tarafold read it: numeric columns, sortable, paged, downloadable. */
export function Data() {
  const { report, data } = useApp();
  const [page, setPage] = useState(0);
  const [sort, setSort] = useState<{ col: number; desc: boolean } | null>(null);
  const targets = new Set(report.targets.map((t) => t.target));
  const controls = new Set(report.optimize?.controllables ?? report.run.config.optimize?.controllables ?? []);
  const n = data?.values[0]?.length ?? 0;
  const order = useMemo(() => {
    const idx = Array.from({ length: n }, (_, i) => i);
    if (!data || !sort) return idx;
    const col = data.values[sort.col] ?? [];
    return idx.sort((a, b) => {
      const x = col[a] ?? NaN;
      const y = col[b] ?? NaN;
      const d = (Number.isNaN(x) ? Infinity : x) - (Number.isNaN(y) ? Infinity : y);
      return sort.desc ? -d : d;
    });
  }, [data, sort, n]);

  if (!data) {
    return (
      <div className="view">
        <p className="muted">
          No data sample is embedded in this report. Rebuild it with <code>tarafold report {"<dir>"} --data {"<csv>"}</code>.
        </p>
      </div>
    );
  }
  const pages = Math.max(1, Math.ceil(n / PAGE));
  const p = Math.min(page, pages - 1);
  const rows = order.slice(p * PAGE, (p + 1) * PAGE);
  const role = (c: string) => (targets.has(c) ? "target" : controls.has(c) ? "controllable" : "feature");
  const csv = () =>
    download(
      "data-sample.csv",
      toCsv(
        data.columns,
        order.map((i) => data.values.map((col) => col[i] ?? null)),
      ),
    );
  const clickSort = (j: number) =>
    setSort((s) => (s && s.col === j ? (s.desc ? null : { col: j, desc: true }) : { col: j, desc: false }));

  return (
    <div className="view">
      <section className="card" aria-labelledby="data-h">
        <h2 id="data-h">Data</h2>
        <p className="small">
          {data.sampled
            ? `${n.toLocaleString()} evenly spaced rows of ${data.rows_total.toLocaleString()} (the sample embedded in this report).`
            : `All ${n.toLocaleString()} rows.`}{" "}
          Source <span className="mono">{report.dataset.source.path}</span>. Numeric columns only
          {report.dataset.source.excluded.length > 0 &&
            `; left out: ${report.dataset.source.excluded.map((e) => `${e.name} (${e.reason})`).join(", ")}`}
          . Click a column name to sort.
        </p>
        <div className="controls">
          <span className="legend-chip role-target">target</span>
          <span className="legend-chip role-controllable">controllable</span>
          <span className="legend-chip role-feature">other feature</span>
          <button type="button" onClick={csv}>
            Download as CSV
          </button>
        </div>
        <div className="table-wrap data-table">
          <table>
            <thead>
              <tr>
                <th scope="col">row</th>
                {data.columns.map((c, j) => (
                  <th
                    key={c}
                    scope="col"
                    className={`role-${role(c)}`}
                    aria-sort={sort?.col === j ? (sort.desc ? "descending" : "ascending") : "none"}
                  >
                    <button type="button" className="sort" onClick={() => clickSort(j)} title={role(c)}>
                      {c}
                      {sort?.col === j ? (sort.desc ? " ▼" : " ▲") : ""}
                    </button>
                  </th>
                ))}
              </tr>
            </thead>
            <tbody>
              {rows.map((i) => (
                <tr key={i}>
                  <td className="muted">{i + 1}</td>
                  {data.values.map((col, j) => (
                    <td key={j} className={`num role-${role(data.columns[j] ?? "")}`}>
                      {fmt(col[i] ?? null, 6)}
                    </td>
                  ))}
                </tr>
              ))}
            </tbody>
          </table>
        </div>
        <div className="controls">
          <button type="button" disabled={p === 0} onClick={() => setPage(p - 1)}>
            Previous
          </button>
          <span className="small" aria-live="polite">
            Page {p + 1} of {pages}
          </span>
          <button type="button" disabled={p >= pages - 1} onClick={() => setPage(p + 1)}>
            Next
          </button>
        </div>
      </section>
    </div>
  );
}
