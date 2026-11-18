import type { ReactNode } from "react";
import { fmt } from "../format";

export interface Margin {
  top: number;
  right: number;
  bottom: number;
  left: number;
}

export const M: Margin = { top: 10, right: 24, bottom: 38, left: 56 };

/** Numeric axis drawn with plain SVG from a d3 scale's ticks. */
export function Axis({
  scale,
  orient,
  length,
  label,
  ticks = 5,
}: {
  scale: { (v: number): number; ticks(n?: number): number[] };
  orient: "bottom" | "left";
  length: number;
  label?: string;
  ticks?: number;
}) {
  const ts = scale.ticks(ticks);
  if (orient === "bottom") {
    return (
      <g className="axis" aria-hidden="true">
        <line x1={0} x2={length} y1={0} y2={0} />
        {ts.map((t) => (
          <g key={t} transform={`translate(${scale(t)},0)`}>
            <line y2={5} />
            <text y={16} textAnchor="middle">
              {fmt(t, 3)}
            </text>
          </g>
        ))}
        {label && (
          <text className="axis-label" x={length / 2} y={32} textAnchor="middle">
            {label}
          </text>
        )}
      </g>
    );
  }
  return (
    <g className="axis" aria-hidden="true">
      <line x1={0} x2={0} y1={0} y2={length} />
      {ts.map((t) => (
        <g key={t} transform={`translate(0,${scale(t)})`}>
          <line x2={-5} />
          <text x={-8} dy="0.32em" textAnchor="end">
            {fmt(t, 3)}
          </text>
        </g>
      ))}
      {label && (
        <text className="axis-label" transform={`translate(${-44},${length / 2}) rotate(-90)`} textAnchor="middle">
          {label}
        </text>
      )}
    </g>
  );
}

/** A chart with an accessible name and a data-table alternative (R10.7). */
export function Figure({
  title,
  description,
  table,
  children,
}: {
  title: string;
  description?: string;
  table?: { headers: string[]; rows: (string | number)[][] };
  children: ReactNode;
}) {
  return (
    <figure className="figure">
      <figcaption>
        <strong>{title}</strong>
        {description && <span className="muted"> — {description}</span>}
      </figcaption>
      {children}
      {table && table.rows.length > 0 && (
        <details>
          <summary>Data table</summary>
          <div className="table-wrap">
            <table>
              <thead>
                <tr>
                  {table.headers.map((h) => (
                    <th key={h} scope="col">
                      {h}
                    </th>
                  ))}
                </tr>
              </thead>
              <tbody>
                {table.rows.slice(0, 500).map((r, i) => (
                  <tr key={i}>
                    {r.map((c, j) => (
                      <td key={j}>{typeof c === "number" ? fmt(c) : c}</td>
                    ))}
                  </tr>
                ))}
              </tbody>
            </table>
            {table.rows.length > 500 && <p className="muted">First 500 of {table.rows.length} rows.</p>}
          </div>
        </details>
      )}
    </figure>
  );
}
