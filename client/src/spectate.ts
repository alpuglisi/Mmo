// The spectator view's model (pure, unit-tested): reading the watch link,
// choosing the followed lineage, colours, smoothing the coarse head counts
// between two server answers, the population history and the captions.
// The view is `ui/spectate.ts`; the data is `GET /api/spectate`, which shows
// nothing private (server `game::spectate`).

import type { CatastropheView, ClockView, Spectate, SpectateLineage } from './protocol';

export interface WatchParams {
  /** The world id (`?watch=main`; empty: the server's default world). */
  world: string;
  /** `?lineage=<n>`: the lineage to follow, or none (the largest player lineage). */
  lineage: number | null;
  /** `?stream=1`: no controls, larger type (for streams and the capture script). */
  stream: boolean;
}

/** The watch parameters of a page's query string, or null when it isn't a watch link. */
export function parseWatch(search: string): WatchParams | null {
  const q = new URLSearchParams(search);
  if (!q.has('watch')) return null;
  const w = q.get('watch') ?? '';
  const l = q.get('lineage');
  const n = l === null || l === '' ? NaN : Number(l);
  return {
    world: w === '1' || w === 'true' ? '' : w,
    lineage: Number.isInteger(n) && n >= 0 ? n : null,
    stream: q.get('stream') === '1',
  };
}

/** The lineage to follow: the asked one if it exists, else the player lineage with the most members, else the largest. */
export function pickFollowed(s: Spectate, wanted: number | null): SpectateLineage | null {
  if (wanted !== null) {
    const l = s.lineages.find((x) => x.lineage === wanted);
    if (l) return l;
  }
  const alive = s.lineages.filter((l) => l.alive > 0);
  const players = alive.filter((l) => !l.wild);
  const pool = players.length ? players : alive;
  return pool.reduce<SpectateLineage | null>((best, l) => (!best || l.alive > best.alive ? l : best), null);
}

const HUES = [32, 200, 140, 280, 350, 60, 170, 230, 310, 100, 15, 260];

/** A lineage's colour: a fixed hue per index; the followed one bright, wild ones muted. */
export function lineageColor(l: SpectateLineage, followed: number | null): { fill: string; hue: number } {
  const hue = HUES[l.lineage % HUES.length];
  if (l.lineage === followed) return { fill: `hsl(${hue} 95% 62%)`, hue };
  return l.wild ? { fill: `hsl(${hue} 30% 62%)`, hue } : { fill: `hsl(${hue} 70% 58%)`, hue };
}

/** Head counts by `"lineage:cx:cy"`. */
export type Density = Map<string, number>;

export function densityOf(s: Spectate): Density {
  const d: Density = new Map();
  for (const l of s.lineages) for (const [x, y, n] of l.cells) d.set(`${l.lineage}:${x}:${y}`, n);
  return d;
}

/** Counts part way (`t` 0..1) from `a` to `b`: cells only in one fade in or out. */
export function lerpDensity(a: Density, b: Density, t: number): Density {
  const k = Math.min(1, Math.max(0, t));
  const out: Density = new Map();
  for (const [key, n] of a) out.set(key, n * (1 - k) + (b.get(key) ?? 0) * k);
  for (const [key, n] of b) if (!a.has(key)) out.set(key, n * k);
  return out;
}

/** The cell side in tiles of a lineage's counts: players' lineages are counted on coarser cells. */
export function cellOf(s: Spectate, l: SpectateLineage): number {
  return l.wild ? s.cell : s.player_cell ?? s.cell;
}

/** Disc radius in tiles for `n` animals in a cell of `cell` tiles: grows with the square root, at most half a cell past the edge. */
export function discRadius(n: number, cell: number): number {
  if (n <= 0) return 0;
  return Math.min(cell * 0.95, cell * 0.22 + Math.sqrt(n) * cell * 0.12);
}

/** Population over days, per lineage, for the chart (one point per new day). */
export class PopulationHistory {
  readonly days: number[] = [];
  readonly series = new Map<number, number[]>();
  constructor(readonly max = 240) {}

  push(s: Spectate): boolean {
    const day = s.clock.day;
    if (this.days.length && this.days[this.days.length - 1] === day) return false;
    this.days.push(day);
    const len = this.days.length;
    for (const l of s.lineages) {
      let v = this.series.get(l.lineage);
      if (!v) {
        v = new Array(len - 1).fill(0);
        this.series.set(l.lineage, v);
      }
      v.push(l.alive);
    }
    for (const v of this.series.values()) while (v.length < len) v.push(0);
    if (len > this.max) {
      this.days.shift();
      for (const v of this.series.values()) v.shift();
    }
    return true;
  }

  /** The largest value of the given lineages' series (at least 1). */
  peak(lineages: number[]): number {
    let m = 1;
    for (const l of lineages) for (const v of this.series.get(l) ?? []) m = Math.max(m, v);
    return m;
  }
}

/** The lineages worth listing: the followed one, then by head count, `n` in all (extinct ones left out). */
export function topLineages(s: Spectate, followed: number | null, n = 5): SpectateLineage[] {
  const alive = s.lineages.filter((l) => l.alive > 0).sort((a, b) => b.alive - a.alive || a.lineage - b.lineage);
  const f = alive.find((l) => l.lineage === followed);
  const rest = alive.filter((l) => l !== f);
  return (f ? [f, ...rest] : rest).slice(0, n);
}

/** "Year 3 · Spring · Day 241" (1-based days, as the game shows them). */
export function clockLine(c: ClockView): string {
  const season = c.season ? c.season[0].toUpperCase() + c.season.slice(1) : '';
  return [`Year ${c.year + 1}`, season, `Day ${c.day + 1}`].filter(Boolean).join(' · ');
}

const CATASTROPHE_NAMES: Record<string, string> = {
  severe_winter: 'Severe winter',
  megadrought: 'Megadrought',
  epidemic: 'Epidemic',
  flood_year: 'Flood year',
};

/** A catastrophe's banner text. */
export function catastropheLine(c: CatastropheView): string {
  const name = CATASTROPHE_NAMES[c.kind] ?? c.kind.replace(/_/g, ' ');
  return `${name} sweeps the land · ${Math.round(c.severity * 100)}% severity`;
}

/** The tint a catastrophe lays over the map (RGBA). */
export function catastropheTint(kind: string): [number, number, number, number] {
  switch (kind) {
    case 'severe_winter':
      return [215, 235, 255, 0.34];
    case 'megadrought':
      return [214, 150, 70, 0.3];
    case 'epidemic':
      return [150, 80, 200, 0.24];
    case 'flood_year':
      return [60, 120, 230, 0.3];
    default:
      return [255, 255, 255, 0.15];
  }
}

/** "312 alive · 18.7% of the genes" for a lineage. */
export function lineageFacts(l: SpectateLineage): string {
  const parts = [`${l.alive} alive`];
  // (No share before the first daily snapshot: it reads 0.)
  if (l.share !== undefined && l.share > 0) parts.push(`${(l.share * 100).toFixed(1)}% of the genes`);
  return parts.join(' · ');
}
