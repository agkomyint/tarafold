import * as d3 from "d3";
import { useEffect, useMemo, useRef } from "react";
import { fmt, PALETTE } from "../format";
import { Axis, M } from "./common";

const W = 420;
const H = 260;

function extent(xs: number[]): [number, number] {
  let lo = Infinity;
  let hi = -Infinity;
  for (const x of xs) {
    if (Number.isFinite(x)) {
      if (x < lo) lo = x;
      if (x > hi) hi = x;
    }
  }
  if (!Number.isFinite(lo)) return [0, 1];
  if (lo === hi) return [lo - 1, hi + 1];
  return [lo, hi];
}

/** Histogram from precomputed edges and counts. */
export function Histogram({
  edges,
  counts,
  label,
  width = 260,
  height = 140,
}: {
  edges: number[];
  counts: number[];
  label: string;
  width?: number;
  height?: number;
}) {
  const m = { ...M, left: 40, bottom: 30 };
  const iw = width - m.left - m.right;
  const ih = height - m.top - m.bottom;
  const x = d3.scaleLinear().domain(extent(edges)).range([0, iw]);
  const y = d3
    .scaleLinear()
    .domain([0, d3.max(counts) ?? 1])
    .nice()
    .range([ih, 0]);
  return (
    <svg width={width} height={height} role="img" aria-label={`Histogram of ${label}`}>
      <g transform={`translate(${m.left},${m.top})`}>
        {counts.map((c, i) => {
          const x0 = x(edges[i] ?? 0);
          const x1 = x(edges[i + 1] ?? 0);
          return <rect key={i} x={x0} y={y(c)} width={Math.max(0, x1 - x0 - 1)} height={ih - y(c)} fill={PALETTE[0]} />;
        })}
        <g transform={`translate(0,${ih})`}>
          <Axis scale={x} orient="bottom" length={iw} ticks={4} />
        </g>
        <Axis scale={y} orient="left" length={ih} ticks={3} />
      </g>
    </svg>
  );
}

/** Horizontal bars with optional ± error. */
export function Bars({
  items,
  label,
}: {
  items: { name: string; value: number; err?: number; muted?: boolean }[];
  label: string;
}) {
  const rowH = 22;
  const m = { top: 4, right: 60, bottom: 30, left: 140 };
  const width = 520;
  const ih = items.length * rowH;
  const height = ih + m.top + m.bottom;
  const iw = width - m.left - m.right;
  const hi = d3.max(items, (d) => d.value + (d.err ?? 0)) ?? 1;
  const lo = Math.min(0, d3.min(items, (d) => d.value - (d.err ?? 0)) ?? 0);
  const x = d3.scaleLinear().domain([lo, hi > lo ? hi : lo + 1]).nice().range([0, iw]);
  return (
    <svg width={width} height={height} role="img" aria-label={label}>
      <g transform={`translate(${m.left},${m.top})`}>
        {items.map((d, i) => (
          <g key={d.name} transform={`translate(0,${i * rowH})`}>
            <text x={-8} y={rowH / 2} dy="0.32em" textAnchor="end" className="tick-label">
              {d.name.length > 20 ? `${d.name.slice(0, 19)}…` : d.name}
            </text>
            <rect
              x={x(Math.min(0, d.value))}
              y={4}
              width={Math.abs(x(d.value) - x(0))}
              height={rowH - 8}
              fill={d.muted ? "#9aa5b1" : PALETTE[0]}
            />
            {d.err !== undefined && d.err > 0 && (
              <line
                x1={x(d.value - d.err)}
                x2={x(d.value + d.err)}
                y1={rowH / 2}
                y2={rowH / 2}
                stroke="#222"
                strokeWidth={1.5}
              />
            )}
            <text x={x(Math.max(d.value, 0)) + 4} y={rowH / 2} dy="0.32em" className="tick-label">
              {fmt(d.value, 3)}
            </text>
          </g>
        ))}
        <g transform={`translate(0,${ih})`}>
          <Axis scale={x} orient="bottom" length={iw} ticks={5} />
        </g>
      </g>
    </svg>
  );
}

/** Square matrix heatmap with a diverging scale in [-1, 1] or sequential in [0, max]. */
export function Heatmap({
  names,
  values,
  label,
  diverging = true,
}: {
  names: string[];
  values: (number | null)[][];
  label: string;
  diverging?: boolean;
}) {
  const n = names.length;
  const cell = Math.max(18, Math.min(44, Math.floor(360 / Math.max(n, 1))));
  const m = { top: 8, right: 8, bottom: 8, left: 120 };
  const top = 110;
  const width = m.left + n * cell + m.right;
  const height = top + n * cell + m.bottom;
  const flat = values.flat().filter((v): v is number => v !== null && Number.isFinite(v));
  const max = diverging ? 1 : Math.max(1e-9, d3.max(flat) ?? 1);
  const color = diverging
    ? (v: number) => d3.interpolateRdBu((1 - v) / 2)
    : (v: number) => d3.interpolateBlues(0.1 + 0.9 * (v / max));
  return (
    <svg width={width} height={height} role="img" aria-label={label}>
      <g transform={`translate(${m.left},${top})`}>
        {names.map((nm, i) => (
          <text key={`r${nm}`} x={-6} y={i * cell + cell / 2} dy="0.32em" textAnchor="end" className="tick-label">
            {nm.length > 16 ? `${nm.slice(0, 15)}…` : nm}
          </text>
        ))}
        {names.map((nm, j) => (
          <text
            key={`c${nm}`}
            transform={`translate(${j * cell + cell / 2},-6) rotate(-55)`}
            className="tick-label"
          >
            {nm.length > 16 ? `${nm.slice(0, 15)}…` : nm}
          </text>
        ))}
        {values.map((row, i) =>
          row.map((v, j) => (
            <g key={`${i}-${j}`}>
              <rect
                x={j * cell}
                y={i * cell}
                width={cell - 1}
                height={cell - 1}
                fill={v === null || !Number.isFinite(v) ? "#eee" : color(v)}
              >
                <title>{`${names[i]} × ${names[j]}: ${fmt(v, 3)}`}</title>
              </rect>
              {cell >= 30 && v !== null && Number.isFinite(v) && (
                <text
                  x={j * cell + cell / 2}
                  y={i * cell + cell / 2}
                  dy="0.32em"
                  textAnchor="middle"
                  className="cell-label"
                  fill={Math.abs(diverging ? v : v / max) > 0.6 ? "#fff" : "#111"}
                >
                  {v.toFixed(2)}
                </text>
              )}
            </g>
          )),
        )}
      </g>
    </svg>
  );
}

/** Line chart with an optional band and observed-range shading. */
export function LineChart({
  xs,
  ys,
  band,
  xLabel,
  yLabel,
  observed,
  marker,
}: {
  xs: number[];
  ys: number[];
  band?: { lo: number[]; hi: number[] };
  xLabel: string;
  yLabel: string;
  observed?: [number, number];
  marker?: number;
}) {
  const iw = W - M.left - M.right;
  const ih = H - M.top - M.bottom;
  const x = d3.scaleLinear().domain(extent(xs)).range([0, iw]);
  const all = [...ys, ...(band ? [...band.lo, ...band.hi] : [])];
  const y = d3.scaleLinear().domain(extent(all)).nice().range([ih, 0]);
  const line = d3
    .line<number>()
    .defined((_, i) => Number.isFinite(ys[i]!))
    .x((_, i) => x(xs[i]!))
    .y((v) => y(v));
  const area =
    band &&
    d3
      .area<number>()
      .defined((_, i) => Number.isFinite(band.lo[i]!) && Number.isFinite(band.hi[i]!))
      .x((_, i) => x(xs[i]!))
      .y0((_, i) => y(band.lo[i]!))
      .y1((_, i) => y(band.hi[i]!));
  return (
    <svg width={W} height={H} role="img" aria-label={`${yLabel} against ${xLabel}`}>
      <g transform={`translate(${M.left},${M.top})`}>
        {observed && (
          <rect
            x={x(observed[0])}
            width={Math.max(0, x(observed[1]) - x(observed[0]))}
            y={0}
            height={ih}
            fill="#f1f5f9"
          >
            <title>Observed range</title>
          </rect>
        )}
        {area && <path d={area(ys) ?? ""} fill={PALETTE[4]} opacity={0.35} />}
        <path d={line(ys) ?? ""} fill="none" stroke={PALETTE[0]} strokeWidth={2} />
        {marker !== undefined && Number.isFinite(marker) && (
          <line x1={x(marker)} x2={x(marker)} y1={0} y2={ih} stroke={PALETTE[5]} strokeDasharray="4 3" />
        )}
        <g transform={`translate(0,${ih})`}>
          <Axis scale={x} orient="bottom" length={iw} label={xLabel} />
        </g>
        <Axis scale={y} orient="left" length={ih} label={yLabel} />
      </g>
    </svg>
  );
}

export interface ScatterSeries {
  x: ArrayLike<number>;
  y: ArrayLike<number>;
  color?: string;
  radius?: number;
  label?: string;
}

/**
 * Canvas scatter for many points (tested with 50k) with optional rectangular brushing. Selected
 * indices of series 0 are reported through `onBrush`; `highlight` marks selected points.
 */
export function Scatter({
  series,
  xLabel,
  yLabel,
  width = W,
  height = H,
  onBrush,
  highlight,
  diagonal,
  onClickPoint,
  compact,
}: {
  series: ScatterSeries[];
  xLabel: string;
  yLabel: string;
  width?: number;
  height?: number;
  onBrush?: (sel: Set<number> | null, final: boolean) => void;
  highlight?: Set<number> | null;
  diagonal?: boolean;
  onClickPoint?: (i: number) => void;
  compact?: boolean;
}) {
  const m = compact ? { top: 4, right: 4, bottom: 4, left: 4 } : M;
  const iw = width - m.left - m.right;
  const ih = height - m.top - m.bottom;
  const canvas = useRef<HTMLCanvasElement>(null);
  const brushG = useRef<SVGGElement>(null);
  const { x, y } = useMemo(() => {
    const xsAll: number[] = [];
    const ysAll: number[] = [];
    for (const s of series) {
      for (let i = 0; i < s.x.length; i++) {
        xsAll.push(s.x[i]!);
        ysAll.push(s.y[i]!);
      }
    }
    let ex = extent(xsAll);
    let ey = extent(ysAll);
    if (diagonal) {
      const lo = Math.min(ex[0], ey[0]);
      const hi = Math.max(ex[1], ey[1]);
      ex = [lo, hi];
      ey = [lo, hi];
    }
    return {
      x: d3.scaleLinear().domain(ex).nice().range([0, iw]),
      y: d3.scaleLinear().domain(ey).nice().range([ih, 0]),
    };
  }, [series, iw, ih, diagonal]);

  // Pixel positions per series, computed once per data or scale change.
  const pixels = useMemo(
    () =>
      series.map((s) => {
        const n = s.x.length;
        const px = new Float32Array(n);
        const py = new Float32Array(n);
        for (let i = 0; i < n; i++) {
          px[i] = x(s.x[i]!);
          py[i] = y(s.y[i]!);
        }
        return [px, py] as const;
      }),
    [series, x, y],
  );

  useEffect(() => {
    const c = canvas.current;
    if (!c) return;
    const dpr = window.devicePixelRatio || 1;
    c.width = iw * dpr;
    c.height = ih * dpr;
    const ctx = c.getContext("2d");
    if (!ctx) return;
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.clearRect(0, 0, iw, ih);
    if (diagonal) {
      ctx.strokeStyle = "#999";
      ctx.setLineDash([4, 3]);
      ctx.beginPath();
      const [lo, hi] = x.domain() as [number, number];
      ctx.moveTo(x(lo), y(lo));
      ctx.lineTo(x(hi), y(hi));
      ctx.stroke();
      ctx.setLineDash([]);
    }
    series.forEach((s, k) => {
      const n = s.x.length;
      const r = s.radius ?? (n > 5000 ? 1.2 : n > 500 ? 2 : 3);
      const color = s.color ?? PALETTE[k % PALETTE.length]!;
      ctx.globalAlpha = n > 5000 ? 0.35 : k === 0 ? 0.7 : 1;
      const [pxs, pys] = pixels[k]!;
      if (k > 0) {
        // Overlay series (suggestions, candidates): larger outlined diamonds.
        ctx.fillStyle = color;
        ctx.strokeStyle = "#111";
        for (let i = 0; i < n; i++) {
          const px = pxs[i]!;
          const py = pys[i]!;
          if (!Number.isFinite(px) || !Number.isFinite(py)) continue;
          ctx.beginPath();
          ctx.moveTo(px, py - 6);
          ctx.lineTo(px + 6, py);
          ctx.lineTo(px, py + 6);
          ctx.lineTo(px - 6, py);
          ctx.closePath();
          ctx.fill();
          ctx.stroke();
        }
        return;
      }
      // Two passes with one fill style each: unselected, then selected on top.
      for (const pass of highlight ? [0, 1] : [0]) {
        ctx.fillStyle = highlight ? (pass ? PALETTE[5]! : "#c5ccd3") : color;
        for (let i = 0; i < n; i++) {
          if (highlight && highlight.has(i) !== (pass === 1)) continue;
          const px = pxs[i]!;
          const py = pys[i]!;
          if (px !== px || py !== py) continue;
          ctx.fillRect(px - r, py - r, 2 * r, 2 * r);
        }
      }
    });
    ctx.globalAlpha = 1;  }, [series, pixels, x, y, iw, ih, highlight, diagonal]);

  useEffect(() => {
    const g = brushG.current;
    if (!g || !onBrush) return;
    const p0 = pixels[0];
    let frame = 0;
    let pending: [[number, number], [number, number]] | null = null;
    const emit = (final: boolean) => {
      frame = 0;
      const sel = pending;
      if (!sel || !p0) {
        onBrush(null, final);
        return;
      }
      const [[x0, y0], [x1, y1]] = sel;
      const [pxs, pys] = p0;
      const out = new Set<number>();
      for (let i = 0; i < pxs.length; i++) {
        const px = pxs[i]!;
        const py = pys[i]!;
        if (px >= x0 && px <= x1 && py >= y0 && py <= y1) out.add(i);
      }
      onBrush(out, final);
    };
    const brush = d3
      .brush()
      .extent([
        [0, 0],
        [iw, ih],
      ])
      .on("brush end", (ev: d3.D3BrushEvent<unknown>) => {
        pending = ev.selection as [[number, number], [number, number]] | null;
        if (ev.type === "end") {
          if (frame) cancelAnimationFrame(frame);
          emit(true);
        } else if (!frame) {
          // Coalesce brush moves to one update per animation frame.
          frame = requestAnimationFrame(() => emit(false));
        }
      });
    const sel = d3.select(g);
    sel.call(brush);
    return () => {
      if (frame) cancelAnimationFrame(frame);
      sel.on(".brush", null);
      sel.selectAll("*").remove();
    };
  }, [onBrush, pixels, iw, ih]);

  const click = (e: React.MouseEvent<SVGRectElement>) => {
    if (!onClickPoint || !series[0]) return;
    const rect = e.currentTarget.getBoundingClientRect();
    const mx = e.clientX - rect.left;
    const my = e.clientY - rect.top;
    let best = -1;
    let bd = 100;
    const s = series[0];
    for (let i = 0; i < s.x.length; i++) {
      const d = (x(s.x[i]!) - mx) ** 2 + (y(s.y[i]!) - my) ** 2;
      if (d < bd) {
        bd = d;
        best = i;
      }
    }
    if (best >= 0) onClickPoint(best);
  };

  return (
    <div className="scatter" style={{ width, height }} role="img" aria-label={`Scatter of ${yLabel} against ${xLabel}`}>
      <canvas
        ref={canvas}
        style={{ position: "absolute", left: m.left, top: m.top, width: iw, height: ih }}
        aria-hidden="true"
      />
      <svg width={width} height={height} style={{ position: "absolute", left: 0, top: 0 }}>
        <g transform={`translate(${m.left},${m.top})`}>
          {!compact && (
            <>
              <g transform={`translate(0,${ih})`}>
                <Axis scale={x} orient="bottom" length={iw} label={xLabel} />
              </g>
              <Axis scale={y} orient="left" length={ih} label={yLabel} />
            </>
          )}
          {onClickPoint && <rect width={iw} height={ih} fill="transparent" onClick={click} style={{ cursor: "pointer" }} />}
          {onBrush && <g ref={brushG} />}
        </g>
      </svg>
    </div>
  );
}

/** Heatmap of a function over a 2-D grid (response surface). */
export function Surface({
  fn,
  xDomain,
  yDomain,
  xLabel,
  yLabel,
  zLabel,
  points,
}: {
  fn: (x: number, y: number) => number;
  xDomain: [number, number];
  yDomain: [number, number];
  xLabel: string;
  yLabel: string;
  zLabel: string;
  points?: { x: number[]; y: number[] };
}) {
  const n = 60;
  const iw = W - M.left - M.right - 50;
  const ih = H - M.top - M.bottom;
  const canvas = useRef<HTMLCanvasElement>(null);
  const grid = useMemo(() => {
    const z: number[] = [];
    for (let j = 0; j < n; j++) {
      for (let i = 0; i < n; i++) {
        const xv = xDomain[0] + ((i + 0.5) / n) * (xDomain[1] - xDomain[0]);
        const yv = yDomain[1] - ((j + 0.5) / n) * (yDomain[1] - yDomain[0]);
        z.push(fn(xv, yv));
      }
    }
    return z;
  }, [fn, xDomain, yDomain]);
  const [zlo, zhi] = extent(grid);
  const x = d3.scaleLinear().domain(xDomain).range([0, iw]);
  const y = d3.scaleLinear().domain(yDomain).range([ih, 0]);
  const color = d3.scaleSequential(d3.interpolateViridis).domain([zlo, zhi]);
  useEffect(() => {
    const c = canvas.current;
    const ctx = c?.getContext("2d");
    if (!c || !ctx) return;
    c.width = n;
    c.height = n;
    const img = ctx.createImageData(n, n);
    grid.forEach((v, k) => {
      const rgb = d3.rgb(Number.isFinite(v) ? color(v) : "#ddd");
      img.data[4 * k] = rgb.r;
      img.data[4 * k + 1] = rgb.g;
      img.data[4 * k + 2] = rgb.b;
      img.data[4 * k + 3] = 255;
    });
    ctx.putImageData(img, 0, 0);
  }, [grid, color]);
  const legend = d3.range(0, 1.0001, 0.1).map((t) => zlo + t * (zhi - zlo));
  return (
    <div className="scatter" style={{ width: W, height: H }} role="img" aria-label={`${zLabel} over ${xLabel} and ${yLabel}`}>
      <canvas
        ref={canvas}
        style={{ position: "absolute", left: M.left, top: M.top, width: iw, height: ih, imageRendering: "pixelated" }}
        aria-hidden="true"
      />
      <svg width={W} height={H} style={{ position: "absolute", left: 0, top: 0 }}>
        <g transform={`translate(${M.left},${M.top})`}>
          {points &&
            points.x.map((px, i) => (
              <circle key={i} cx={x(px)} cy={y(points.y[i]!)} r={1.6} fill="#fff" stroke="#000" strokeWidth={0.5} />
            ))}
          <g transform={`translate(0,${ih})`}>
            <Axis scale={x} orient="bottom" length={iw} label={xLabel} />
          </g>
          <Axis scale={y} orient="left" length={ih} label={yLabel} />
          <g transform={`translate(${iw + 12},0)`}>
            {legend.map((v, i) => (
              <rect key={i} y={ih - ((i + 1) * ih) / legend.length} width={12} height={ih / legend.length + 0.5} fill={color(v)} />
            ))}
            <text x={16} y={8} className="tick-label">
              {fmt(zhi, 3)}
            </text>
            <text x={16} y={ih} className="tick-label">
              {fmt(zlo, 3)}
            </text>
          </g>
        </g>
      </svg>
    </div>
  );
}

/** Parallel coordinates with a brush per axis; `onFilter` gets the indices inside every brush. */
export function ParallelCoords({
  axes,
  rows,
  onFilter,
  highlight,
}: {
  axes: string[];
  rows: number[][];
  onFilter: (sel: Set<number> | null) => void;
  highlight?: Set<number> | null;
}) {
  const width = Math.max(420, axes.length * 120);
  const height = 280;
  const m = { top: 30, right: 40, bottom: 16, left: 64 };
  const iw = width - m.left - m.right;
  const ih = height - m.top - m.bottom;
  const gRef = useRef<SVGGElement>(null);
  const x = d3.scalePoint<string>().domain(axes).range([0, iw]);
  const ys = useMemo(
    () =>
      axes.map((_, j) =>
        d3
          .scaleLinear()
          .domain(extent(rows.map((r) => r[j]!)))
          .nice()
          .range([ih, 0]),
      ),
    [axes, rows, ih],
  );
  const brushes = useRef<Map<number, [number, number]>>(new Map());
  useEffect(() => {
    const g = gRef.current;
    if (!g) return;
    const sel = d3.select(g);
    sel.selectAll("*").remove();
    brushes.current = new Map();
    axes.forEach((a, j) => {
      const b = d3
        .brushY()
        .extent([
          [-10, 0],
          [10, ih],
        ])
        .on("brush end", (ev: d3.D3BrushEvent<unknown>) => {
          const s = ev.selection as [number, number] | null;
          if (s) brushes.current.set(j, [ys[j]!.invert(s[1]), ys[j]!.invert(s[0])]);
          else brushes.current.delete(j);
          if (brushes.current.size === 0) {
            onFilter(null);
            return;
          }
          const out = new Set<number>();
          rows.forEach((r, i) => {
            for (const [k, [lo, hi]] of brushes.current) {
              const v = r[k]!;
              if (!(v >= lo && v <= hi)) return;
            }
            out.add(i);
          });
          onFilter(out);
        });
      sel
        .append("g")
        .attr("transform", `translate(${x(a)},0)`)
        .call(b);
    });
  }, [axes, rows, ys, x, ih, onFilter]);
  return (
    <svg width={width} height={height} role="img" aria-label={`Parallel coordinates of ${axes.join(", ")}`}>
      <g transform={`translate(${m.left},${m.top})`}>
        {rows.map((r, i) => {
          const on = !highlight || highlight.has(i);
          const d = axes.map((a, j) => `${j ? "L" : "M"}${x(a)},${ys[j]!(r[j]!)}`).join("");
          return <path key={i} d={d} fill="none" stroke={on ? PALETTE[0] : "#d6dbe0"} strokeWidth={on ? 1.6 : 1} opacity={on ? 0.9 : 0.6} />;
        })}
        {axes.map((a, j) => (
          <g key={a} transform={`translate(${x(a)},0)`}>
            <Axis scale={ys[j]!} orient="left" length={ih} ticks={4} />
            <text y={-12} textAnchor="middle" className="axis-label">
              {a.length > 18 ? `${a.slice(0, 17)}…` : a}
            </text>
          </g>
        ))}
        <g ref={gRef} />
      </g>
    </svg>
  );
}
