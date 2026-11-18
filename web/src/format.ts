// Number formatting and small helpers shared by views.

/** Compact number: 4 significant digits, thousands separators, exponent for extremes. */
export function fmt(v: number | null | undefined, sig = 4): string {
  if (v === null || v === undefined || Number.isNaN(v)) return "—";
  if (!Number.isFinite(v)) return v > 0 ? "∞" : "−∞";
  if (v === 0) return "0";
  const a = Math.abs(v);
  if (a >= 1e9 || a < 1e-4) return v.toExponential(sig - 1);
  const digits = Math.max(0, sig - 1 - Math.floor(Math.log10(a)));
  return v.toLocaleString("en-US", { maximumFractionDigits: Math.min(digits, 10) });
}

export function pct(v: number | null | undefined, digits = 0): string {
  if (v === null || v === undefined || !Number.isFinite(v)) return "—";
  return `${(v * 100).toFixed(digits)}%`;
}

/** Finite numbers only. */
export function finite(xs: (number | null | undefined)[]): number[] {
  return xs.filter((x): x is number => typeof x === "number" && Number.isFinite(x));
}

export function median(xs: number[]): number {
  if (xs.length === 0) return NaN;
  const s = [...xs].sort((a, b) => a - b);
  const m = s.length >> 1;
  return s.length % 2 ? s[m]! : (s[m - 1]! + s[m]!) / 2;
}

/** Okabe–Ito palette (colour-blind safe). */
export const PALETTE = ["#0072B2", "#E69F00", "#009E73", "#CC79A7", "#56B4E9", "#D55E00", "#F0E442", "#000000"];

export const evidenceLabel: Record<string, string> = {
  association: "association",
  predictive: "predictive",
  functional_fit: "functional fit",
  interventional_fit: "interventional fit",
};
