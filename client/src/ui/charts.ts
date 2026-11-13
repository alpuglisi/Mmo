// Small hand-rolled SVG charts for the genetics dashboard: data preparation
// (pure, unit-tested) and the SVG builders that draw it. No chart library:
// four chart forms are all the dashboard needs and the bundle stays light.
//
// Colors: the categorical slots below are fixed per meaning (never cycled);
// text stays in text colors; every mark has a <title> for hover.

import type { AgeBand, DayVitals, GeneStat, LegacyPoint } from '../protocol';

/** Categorical slots (dark surface), in fixed order. */
export const SERIES = ['#3987e5', '#d95926', '#199e70', '#c98500', '#d55181', '#9085e9', '#8b949e'];

/** Death causes shown on their own; the rest fold into "other". */
export const CAUSES = ['eaten', 'starved', 'thirst', 'exposure', 'old_age'] as const;
export const CAUSE_COLORS: Record<string, string> = {
  eaten: SERIES[1], starved: SERIES[3], thirst: SERIES[0], exposure: SERIES[5], old_age: SERIES[2], other: SERIES[6],
};

export type Scale = (v: number) => number;

/** A linear map from `domain` to `range`; a zero-width domain maps to the range's middle. */
export function linearScale([d0, d1]: [number, number], [r0, r1]: [number, number]): Scale {
  const span = d1 - d0;
  if (span === 0 || !Number.isFinite(span)) return () => (r0 + r1) / 2;
  return (v) => r0 + ((v - d0) / span) * (r1 - r0);
}

/** The smallest 1, 2 or 5 × 10^k at or above `v` (1 for 0 or less). */
export function niceMax(v: number): number {
  if (!(v > 0)) return 1;
  const p = 10 ** Math.floor(Math.log10(v));
  for (const m of [1, 2, 5, 10]) if (m * p >= v - 1e-12) return m * p;
  return 10 * p;
}

/**
 * An SVG path through `values` at x = `sx(k)`; null or non-finite values
 * break the line (the next value starts a new segment).
 */
export function linePath(values: (number | null)[], sx: Scale, sy: Scale): string {
  let d = '';
  let pen = false;
  values.forEach((v, k) => {
    if (v === null || !Number.isFinite(v)) {
      pen = false;
      return;
    }
    d += `${pen ? 'L' : 'M'}${sx(k).toFixed(1)},${sy(v).toFixed(1)}`;
    pen = true;
  });
  return d;
}

export interface StackSegment {
  cause: string;
  y0: number;
  y1: number;
}

export interface VitalsDay {
  day: number;
  births: number;
  deaths: number;
  /** Deaths stacked by cause in `CAUSES` order then "other" (counts, bottom-up). */
  segments: StackSegment[];
}

/** Births and deaths per day, deaths stacked by cause. */
export function stackVitals(vitals: DayVitals[]): { days: VitalsDay[]; maxBirths: number; maxDeaths: number; causes: string[] } {
  const seen = new Set<string>();
  const days = vitals.map((v) => {
    const by: Record<string, number> = {};
    for (const [cause, n] of Object.entries(v.deaths)) {
      const key = (CAUSES as readonly string[]).includes(cause) ? cause : 'other';
      by[key] = (by[key] ?? 0) + n;
    }
    let y = 0;
    const segments: StackSegment[] = [];
    for (const cause of [...CAUSES, 'other']) {
      const n = by[cause] ?? 0;
      if (n > 0) {
        segments.push({ cause, y0: y, y1: y + n });
        seen.add(cause);
        y += n;
      }
    }
    return { day: v.day, births: v.births, deaths: y, segments };
  });
  return {
    days,
    maxBirths: Math.max(0, ...days.map((d) => d.births)),
    maxDeaths: Math.max(0, ...days.map((d) => d.deaths)),
    causes: [...CAUSES, 'other'].filter((c) => seen.has(c)),
  };
}

export interface PyramidRow {
  label: string;
  female: number;
  male: number;
}

/** Age bands youngest at the bottom (rows top-down: oldest first). */
export function pyramidRows(bands: AgeBand[]): { rows: PyramidRow[]; max: number } {
  const rows = [...bands].reverse().map((b) => ({ label: `${Math.round(b.from_days)}–${Math.round(b.to_days)} d`, female: b.female, male: b.male }));
  return { rows, max: Math.max(0, ...rows.map((r) => Math.max(r.female, r.male))) };
}

export interface DriftRow {
  gene: string;
  label: string;
  mean: number;
  sd: number;
  founding: number;
  drift: number;
  history: (number | null)[];
}

/** The `n` genes that drifted most from their founding mean (the server sorts them). */
export function driftRows(genes: GeneStat[], n: number): DriftRow[] {
  return [...genes]
    .map((g) => ({
      gene: g.gene,
      label: g.gene.replace(/_/g, ' '),
      mean: g.mean,
      sd: g.sd,
      founding: g.founding_mean,
      drift: g.mean - g.founding_mean,
      history: g.history,
    }))
    .sort((a, b) => Math.abs(b.drift) - Math.abs(a.drift) || a.gene.localeCompare(b.gene))
    .slice(0, n);
}

/** Legacy share series in percent, plus the y maximum. */
export function legacySeries(points: LegacyPoint[]): { days: number[]; share: number[]; weighted: number[]; max: number } {
  const share = points.map((p) => p.share * 100);
  const weighted = points.map((p) => p.weighted_share * 100);
  return { days: points.map((p) => p.day), share, weighted, max: niceMax(Math.max(0, ...share, ...weighted)) };
}

// ---------------------------------------------------------------- SVG

const NS = 'http://www.w3.org/2000/svg';
type Attrs = Record<string, string | number>;

export function svg(tag: string, attrs: Attrs = {}, ...children: (SVGElement | string | null)[]): SVGElement {
  const el = document.createElementNS(NS, tag);
  for (const [k, v] of Object.entries(attrs)) el.setAttribute(k, String(v));
  for (const c of children) {
    if (c === null) continue;
    el.append(typeof c === 'string' ? document.createTextNode(c) : c);
  }
  return el;
}

const title = (t: string) => svg('title', {}, t);
const AXIS = '#30363d';
const INK = '#8b949e';

function frame(w: number, h: number, label: string): SVGElement {
  return svg('svg', { viewBox: `0 0 ${w} ${h}`, width: '100%', role: 'img', 'aria-label': label, class: 'chart' });
}

/** Line chart of legacy share (%) and weighted share per day. */
export function legacyChart(points: LegacyPoint[]): SVGElement {
  const W = 300, H = 120, L = 30, R = 8, T = 8, Bm = 18;
  const root = frame(W, H, 'Legacy share per day');
  const s = legacySeries(points);
  const sx = linearScale([0, Math.max(1, points.length - 1)], [L, W - R]);
  const sy = linearScale([0, s.max], [H - Bm, T]);
  root.append(
    svg('line', { x1: L, x2: W - R, y1: H - Bm, y2: H - Bm, stroke: AXIS }),
    svg('line', { x1: L, x2: W - R, y1: T, y2: T, stroke: AXIS, 'stroke-dasharray': '2 3' }),
    svg('text', { x: L - 4, y: T + 4, 'text-anchor': 'end', fill: INK, 'font-size': 9 }, `${s.max}%`),
    svg('text', { x: L - 4, y: H - Bm + 3, 'text-anchor': 'end', fill: INK, 'font-size': 9 }, '0'),
  );
  if (points.length > 0) {
    root.append(
      svg('text', { x: L, y: H - 4, fill: INK, 'font-size': 9 }, `day ${s.days[0] + 1}`),
      svg('text', { x: W - R, y: H - 4, 'text-anchor': 'end', fill: INK, 'font-size': 9 }, `day ${s.days[s.days.length - 1] + 1}`),
    );
  }
  const series: [number[], string, string][] = [[s.weighted, SERIES[2], 'weighted'], [s.share, SERIES[0], 'share']];
  for (const [vals, color, name] of series) {
    root.append(svg('path', { d: linePath(vals, sx, sy), fill: 'none', stroke: color, 'stroke-width': 2, 'stroke-linejoin': 'round', class: `series-${name}` }));
  }
  // Hover: one column per day.
  points.forEach((p, k) => {
    const x = sx(k);
    const w = (W - L - R) / Math.max(1, points.length);
    root.append(svg('rect', { x: x - w / 2, y: T, width: w, height: H - Bm - T, fill: 'transparent' },
      title(`Day ${p.day + 1}: share ${(p.share * 100).toFixed(2)}%, weighted ${(p.weighted_share * 100).toFixed(2)}%, ${p.alive} alive`)));
  });
  return root;
}

/** Population pyramid: females left, males right, oldest band on top. */
export function pyramidChart(bands: AgeBand[]): SVGElement {
  const { rows, max } = pyramidRows(bands);
  const W = 300, rowH = 16, T = 4, mid = W / 2, half = W / 2 - 46;
  const H = T * 2 + Math.max(1, rows.length) * rowH;
  const root = frame(W, H, 'Population by age and sex');
  const sx = linearScale([0, Math.max(1, max)], [0, half]);
  rows.forEach((r, k) => {
    const y = T + k * rowH;
    const fw = sx(r.female), mw = sx(r.male);
    root.append(
      svg('rect', { x: mid - 1 - fw, y: y + 2, width: fw, height: rowH - 4, rx: 2, fill: SERIES[0] }, title(`${r.label}: ${r.female} female`)),
      svg('rect', { x: mid + 1, y: y + 2, width: mw, height: rowH - 4, rx: 2, fill: SERIES[1] }, title(`${r.label}: ${r.male} male`)),
      svg('text', { x: mid - 4 - fw, y: y + rowH - 4, 'text-anchor': 'end', fill: INK, 'font-size': 9 }, String(r.female)),
      svg('text', { x: mid + 4 + mw, y: y + rowH - 4, fill: INK, 'font-size': 9 }, String(r.male)),
      svg('text', { x: 2, y: y + rowH - 4, fill: INK, 'font-size': 9 }, r.label),
    );
  });
  return root;
}

/** Births above the axis, deaths stacked by cause below, per day. */
export function vitalsChart(vitals: DayVitals[]): SVGElement {
  const st = stackVitals(vitals);
  const W = 300, H = 130, L = 24, R = 4, T = 6, B = 6;
  const root = frame(W, H, 'Births and deaths per day');
  const top = niceMax(st.maxBirths), bottom = niceMax(st.maxDeaths);
  const zero = T + ((H - T - B) * top) / (top + bottom);
  const up = linearScale([0, top], [zero, T]);
  const down = linearScale([0, bottom], [zero, H - B]);
  const n = Math.max(1, st.days.length);
  const slot = (W - L - R) / n;
  const bw = Math.max(1, slot - 2);
  root.append(
    svg('text', { x: L - 3, y: T + 7, 'text-anchor': 'end', fill: INK, 'font-size': 9 }, `+${top}`),
    svg('text', { x: W - R, y: T + 7, 'text-anchor': 'end', fill: INK, 'font-size': 9 }, 'born ↑'),
    svg('text', { x: L - 3, y: H - B, 'text-anchor': 'end', fill: INK, 'font-size': 9 }, `−${bottom}`),
    svg('text', { x: W - R, y: H - B, 'text-anchor': 'end', fill: INK, 'font-size': 9 }, 'died ↓'),
  );
  st.days.forEach((d, k) => {
    const x = L + k * slot + 1;
    const today = k === st.days.length - 1;
    if (d.births > 0) {
      root.append(svg('rect', { x, y: up(d.births), width: bw, height: zero - up(d.births), rx: Math.min(2, bw / 2), fill: SERIES[2], opacity: today ? 0.6 : 1 },
        title(`Day ${d.day + 1}${today ? ' (so far)' : ''}: ${d.births} born`)));
    }
    for (const s of d.segments) {
      const y0 = down(s.y0) + (s.y0 > 0 ? 1 : 0);
      const h = Math.max(0.5, down(s.y1) - y0);
      root.append(svg('rect', { x, y: y0, width: bw, height: h, fill: CAUSE_COLORS[s.cause], opacity: today ? 0.6 : 1 },
        title(`Day ${d.day + 1}${today ? ' (so far)' : ''}: ${s.y1 - s.y0} died (${s.cause.replace('_', ' ')})`)));
    }
  });
  root.append(svg('line', { x1: L, x2: W - R, y1: zero, y2: zero, stroke: AXIS }));
  return root;
}

/** A sparkline of a gene's daily mean with its founding value as a dashed baseline (y 0..1). */
export function geneSparkline(row: DriftRow): SVGElement {
  const W = 120, H = 28;
  const root = frame(W, H, `${row.label} over time`);
  const vals = row.history.length > 0 ? [...row.history, row.mean] : [row.founding, row.mean];
  const finite = vals.filter((v): v is number => v !== null && Number.isFinite(v));
  const lo = Math.max(0, Math.min(...finite, row.founding) - 0.02), hi = Math.min(1, Math.max(...finite, row.founding) + 0.02);
  const sx = linearScale([0, Math.max(1, vals.length - 1)], [2, W - 2]);
  const sy = linearScale([lo, hi], [H - 3, 3]);
  root.append(
    svg('line', { x1: 2, x2: W - 2, y1: sy(row.founding), y2: sy(row.founding), stroke: INK, 'stroke-dasharray': '2 2', 'stroke-width': 1 }),
    svg('path', { d: linePath(vals, sx, sy), fill: 'none', stroke: SERIES[0], 'stroke-width': 2, 'stroke-linejoin': 'round' }),
    svg('circle', { cx: sx(vals.length - 1), cy: sy(row.mean), r: 2.5, fill: SERIES[0] }),
    title(`${row.label}: founded at ${row.founding.toFixed(3)}, now ${row.mean.toFixed(3)} ± ${row.sd.toFixed(3)}`),
  );
  return root;
}

/** Mean ± sd of a gene on a 0..1 track. */
export function spreadBar(mean: number, sd: number, founding: number): SVGElement {
  const W = 100, H = 12;
  const root = frame(W, H, `mean ${mean.toFixed(2)} ± ${sd.toFixed(2)}`);
  const sx = linearScale([0, 1], [3, W - 3]);
  root.append(
    svg('line', { x1: sx(0), x2: sx(1), y1: H / 2, y2: H / 2, stroke: AXIS, 'stroke-width': 2, 'stroke-linecap': 'round' }),
    svg('line', { x1: sx(Math.max(0, mean - sd)), x2: sx(Math.min(1, mean + sd)), y1: H / 2, y2: H / 2, stroke: SERIES[0], 'stroke-width': 4, 'stroke-linecap': 'round', opacity: 0.55 }),
    svg('line', { x1: sx(founding), x2: sx(founding), y1: 1, y2: H - 1, stroke: INK, 'stroke-width': 1 }),
    svg('circle', { cx: sx(mean), cy: H / 2, r: 3, fill: SERIES[0] }),
  );
  return root;
}
