// Helpers that connect the report, the data sample, and the evaluator.
import { compileTree } from "./expr/eval";
import type { DataSample } from "./report/load";
import type { EquationReport, Report, TargetReport, Tree } from "./report/schema";

export function column(data: DataSample | null, name: string): number[] | null {
  if (!data) return null;
  const i = data.columns.indexOf(name);
  if (i < 0) return null;
  return data.values[i]!.map((v) => (v === null ? NaN : v));
}

/** Replace a tree's constants, in left-to-right (postfix) order, with `values`. */
export function withConstants(tree: Tree, values: (number | null)[]): Tree {
  let k = 0;
  const go = (t: Tree): Tree => {
    if ("const" in t) {
      const v = values[k++];
      return { const: v ?? t.const };
    }
    if ("var" in t) return t;
    return { op: t.op, args: t.args.map(go) };
  };
  return go(tree);
}

/** Function of a full feature vector (in `names` order) for an equation. */
export function equationFn(tree: Tree, names: string[]): (x: ArrayLike<number>) => number {
  return compileTree(tree, names);
}

/** Predictions of an equation on every data-sample row; null without data or a missing column. */
export function predictRows(eq: EquationReport, data: DataSample | null): number[] | null {
  if (!data) return null;
  const cols = eq.variables.map((v) => column(data, v));
  if (cols.some((c) => c === null)) return null;
  const f = compileTree(eq.tree, eq.variables);
  const n = data.values[0]?.length ?? 0;
  const x = new Float64Array(eq.variables.length);
  const out = new Array<number>(n);
  for (let i = 0; i < n; i++) {
    for (let j = 0; j < cols.length; j++) x[j] = cols[j]![i]!;
    out[i] = f(x);
  }
  return out;
}

/** Medians of the target's features from the training domain. */
export function domainMedians(t: TargetReport): Record<string, number> {
  const d = t.validate.domain;
  return Object.fromEntries(d.names.map((n, i) => [n, d.median[i] ?? NaN]));
}

export function domainRange(t: TargetReport, name: string): [number, number] | null {
  const d = t.validate.domain;
  const i = d.names.indexOf(name);
  if (i < 0) return null;
  return [d.min[i] ?? NaN, d.max[i] ?? NaN];
}

/** Ensemble members of the selected equation as trees (bootstrap constant refits). */
export function ensembleTrees(t: TargetReport): Tree[] {
  const sel = t.discover.front.find((e) => e.id === t.discover.selected_id);
  if (!sel) return [];
  return t.validate.bootstrap.ensemble.map((c) => withConstants(sel.tree, c));
}

export function selectedEquation(t: TargetReport): EquationReport | undefined {
  return t.discover.front.find((e) => e.id === t.discover.selected_id);
}

export function quantile(sorted: number[], q: number): number {
  if (sorted.length === 0) return NaN;
  const p = (sorted.length - 1) * q;
  const lo = Math.floor(p);
  const hi = Math.ceil(p);
  return sorted[lo]! + (sorted[hi]! - sorted[lo]!) * (p - lo);
}

/** CSV text with formula-injection guarding for text cells (R14.3). */
export function toCsv(headers: string[], rows: (number | string | null)[][]): string {
  const cell = (v: number | string | null) => {
    if (v === null || v === undefined) return "";
    if (typeof v === "number") return Number.isFinite(v) ? String(v) : "";
    let s = v;
    if (/^[=+\-@]/.test(s)) s = `'${s}`;
    return /[",\n\r]/.test(s) ? `"${s.replace(/"/g, '""')}"` : s;
  };
  return [headers.map(cell).join(","), ...rows.map((r) => r.map(cell).join(","))].join("\n") + "\n";
}

export function download(name: string, text: string, type = "text/csv") {
  const url = URL.createObjectURL(new Blob([text], { type }));
  const a = document.createElement("a");
  a.href = url;
  a.download = name;
  a.click();
  setTimeout(() => URL.revokeObjectURL(url), 1000);
}

export function isInterventional(r: Report): boolean {
  return r.dataset.data_source !== "observational";
}
