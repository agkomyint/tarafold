import katex from "katex";
import { useMemo } from "react";
import { evidenceLabel, fmt } from "./format";
import type { EquationReport, TargetReport } from "./report/schema";

export function GradeBadge({ grade }: { grade: string | null }) {
  if (!grade) return <span className="badge badge-none">not graded</span>;
  return (
    <span className={`badge badge-${grade}`} title="Reliability grade">
      grade: {grade}
    </span>
  );
}

export function EvidenceBadge({ evidence }: { evidence: string | null }) {
  if (!evidence) return null;
  return (
    <span className="badge badge-evidence" title="Evidence level">
      evidence: {evidenceLabel[evidence] ?? evidence}
    </span>
  );
}

export function DomainBadge({ inDomain }: { inDomain: boolean }) {
  return inDomain ? (
    <span className="badge badge-high">in data domain</span>
  ) : (
    <span className="badge badge-low">outside data domain</span>
  );
}

/** LaTeX rendered as MathML (no fonts or CSS needed, works offline). */
export function Tex({ latex, display = false }: { latex: string; display?: boolean }) {
  const html = useMemo(() => {
    try {
      return katex.renderToString(latex, { output: "mathml", displayMode: display, throwOnError: false });
    } catch {
      return null;
    }
  }, [latex, display]);
  if (html === null) return <code>{latex}</code>;
  return <span className="tex" dangerouslySetInnerHTML={{ __html: html }} />;
}

export function EquationCard({ t, eq }: { t: TargetReport; eq: EquationReport }) {
  const selected = eq.id === t.discover.selected_id;
  const v = t.validate;
  const targetTex = `\\mathrm{${t.target.replace(/\\/g, "").replace(/_/g, "\\_").replace(/ /g, "\\ ")}}`;
  return (
    <section className="card equation-card" aria-label={`Equation ${eq.id} for ${t.target}`}>
      <div className="eq-head">
        <span className="muted">
          #{eq.id} · complexity {eq.complexity} · {eq.strategy.replace(/_/g, " ")}
          {selected && " · selected"}
        </span>
        <span>
          <GradeBadge grade={selected ? v.reliability.grade : null} />{" "}
          <EvidenceBadge evidence={selected ? v.evidence : null} />
        </span>
      </div>
      <div className="eq-tex">
        <Tex latex={`${targetTex} = ${eq.latex}`} display />
      </div>
      <p className="mono small">
        {t.target} = {eq.infix}
      </p>
      <table className="metrics">
        <thead>
          <tr>
            <th scope="col">split</th>
            <th scope="col">R²</th>
            <th scope="col">RMSE</th>
            <th scope="col">MAE</th>
            <th scope="col">rows</th>
          </tr>
        </thead>
        <tbody>
          {(["train", "val", "test"] as const).map((s) => (
            <tr key={s}>
              <th scope="row">{s}</th>
              <td>{fmt(eq.metrics[s].r2, 6)}</td>
              <td>{fmt(eq.metrics[s].rmse)}</td>
              <td>{fmt(eq.metrics[s].mae)}</td>
              <td>{eq.metrics[s].n}</td>
            </tr>
          ))}
        </tbody>
      </table>
      {selected && v.bootstrap.constants.length > 0 && (
        <details open>
          <summary>Constants with 90% bootstrap intervals ({v.bootstrap.resamples} resamples)</summary>
          <ul className="constants">
            {v.bootstrap.constants.map((c, i) => (
              <li key={i} className="mono">
                c{i + 1} = {fmt(c.value, 6)} [{fmt(c.lo, 4)}, {fmt(c.hi, 4)}]
              </li>
            ))}
          </ul>
        </details>
      )}
      {!selected && (
        <p className="muted small">
          Only the selected equation is graded. Use <code>tarafold run --select {eq.id}</code> to grade this one.
        </p>
      )}
    </section>
  );
}

export function Notice({ children }: { children: React.ReactNode }) {
  return (
    <p className="notice" role="note">
      {children}
    </p>
  );
}
