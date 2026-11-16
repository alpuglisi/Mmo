// Vision cones on the map: the geometry the world view draws around the
// player's own selected (or possessed) organism, from the server's compact
// eye descriptor (`VisionView`, own organisms only) and its quantized
// heading. Pure (no PixiJS) so it is unit-tested.
//
// Angles here are screen degrees: 0 = east (+x), increasing clockwise
// because screen y points down (south = 90°), which is also how the map's
// world coordinates run. An eye at head azimuth `a` (0 ahead, +90 right) on
// an animal facing `h` looks along `h + a`: facing east, its right is south.
//
// The style follows the founder builder (`ui/builder/builder.ts`): the area
// any eye sees is a light fill, the binocular depth zone in front is green
// and dotted, blind spots are red and hatched, each eye's field a dashed
// outline. Patterns as well as colours, so the zones read without colour.

import type { VisionView } from '../protocol';
import { eyePlacements } from '../morphology';
import { analyseVision, type Arc, type EyeField, type VisionStats } from '../vision';

/** Directions a server heading is quantized to (`vision::HEADINGS`). */
export const HEADINGS = 32;

const mod360 = (a: number): number => ((a % 360) + 360) % 360;
const RAD = Math.PI / 180;

/** Screen degrees of a quantized heading (0 east, 90 south). */
export function headingDeg(h: number): number {
  return mod360((h * 360) / HEADINGS);
}

/** Each eye's field (head azimuths), from the descriptor. */
export function visionFields(v: VisionView): EyeField[] {
  return eyePlacements({ eye_count: v.eyes, eye_azimuth: v.azimuth }).map((e) => ({ azimuth: e.azimuth, width: v.field }));
}

/** The arcs and numbers of a descriptor (the same rules as the sim and the design editor). */
export function visionStats(v: VisionView): VisionStats {
  return analyseVision(visionFields(v), v.occlusion ?? 0);
}

/** An arc in screen degrees: from `start`, clockwise, `width` degrees. */
export interface Wedge {
  start: number;
  width: number;
}

export type BlindKind = 'rear' | 'front' | 'side';

/** What the world view draws, rotated to the heading. */
export interface ConeLayout {
  /** Screen degrees the animal faces. */
  heading: number;
  covered: Wedge[];
  blind: (Wedge & { kind: BlindKind })[];
  /** The binocular depth zone (before snout occlusion). */
  overlap: Wedge[];
  /** Each eye's whole field. */
  eyes: Wedge[];
  /** The part of the depth zone a long snout hides, if any. */
  occluded: Wedge | null;
  stats: VisionStats;
}

const turn = (a: Arc, heading: number): Wedge => ({ start: mod360(heading + a.start), width: a.width });

/** The cones of `v` for an animal facing `heading` (screen degrees). */
export function coneLayout(v: VisionView, heading: number): ConeLayout {
  const stats = visionStats(v);
  const kind = (a: Arc): BlindKind => (a === stats.rearBlind ? 'rear' : a === stats.frontBlind ? 'front' : 'side');
  const occ = stats.occludedDeg;
  return {
    heading: mod360(heading),
    covered: stats.covered.map((a) => turn(a, heading)),
    blind: stats.blind.map((a) => ({ ...turn(a, heading), kind: kind(a) })),
    overlap: stats.overlap.map((a) => turn(a, heading)),
    eyes: visionFields(v).map((f) => turn({ start: f.azimuth - Math.min(360, f.width) / 2, width: Math.min(360, f.width) }, heading)),
    occluded: occ > 0 ? turn({ start: -occ / 2, width: occ }, heading) : null,
    stats,
  };
}

/** Unit vector of a screen angle. */
export function dir(deg: number): { x: number; y: number } {
  return { x: Math.cos(deg * RAD), y: Math.sin(deg * RAD) };
}

/** Whether the screen direction `deg` lies inside `w` (edges included). */
export function wedgeContains(w: Wedge, deg: number): boolean {
  if (w.width >= 360) return true;
  return mod360(deg - w.start) <= w.width + 1e-9;
}

/** A filled wedge as a flat polygon `[x0, y0, x1, y1, …]`: the center (unless a full circle), then the rim. */
export function wedgePolygon(cx: number, cy: number, r: number, w: Wedge, stepDeg = 6): number[] {
  const full = w.width >= 360;
  const width = Math.min(360, w.width);
  const n = Math.max(2, Math.ceil(width / stepDeg));
  const out: number[] = full ? [] : [cx, cy];
  for (let i = 0; i <= (full ? n - 1 : n); i++) {
    const d = dir(w.start + (width * i) / n);
    out.push(cx + r * d.x, cy + r * d.y);
  }
  return out;
}

/** Points along the rim of `w` at radius `r` (for arcs and dashes). */
export function arcPoints(cx: number, cy: number, r: number, w: Wedge, stepDeg = 4): [number, number][] {
  const width = Math.min(360, w.width);
  const n = Math.max(1, Math.ceil(width / stepDeg));
  const out: [number, number][] = [];
  for (let i = 0; i <= n; i++) {
    const d = dir(w.start + (width * i) / n);
    out.push([cx + r * d.x, cy + r * d.y]);
  }
  return out;
}

/** Split a wedge into convex pieces of at most 90°. */
function pieces(w: Wedge): Wedge[] {
  const width = Math.min(360, w.width);
  const n = Math.max(1, Math.ceil(width / 90));
  return Array.from({ length: n }, (_, i) => ({ start: w.start + (width * i) / n, width: width / n }));
}

const cross = (ax: number, ay: number, bx: number, by: number) => ax * by - ay * bx;

/**
 * Hatching: parallel lines `spacing` px apart at `angle` screen degrees (45°
 * like the builder's pattern), anchored at the wedge's center so the
 * pattern moves with the animal, clipped to the wedge of radius `r`.
 * Returns segments `[x1, y1, x2, y2]`.
 */
export function hatchSegments(cx: number, cy: number, r: number, w: Wedge, spacing = 6, angle = 45): [number, number, number, number][] {
  const out: [number, number, number, number][] = [];
  if (r <= 0 || w.width <= 0 || spacing <= 0) return out;
  const d = dir(angle);
  const nx = -d.y, ny = d.x;
  const k = Math.floor(r / spacing);
  for (const p of pieces(w)) {
    const a = dir(p.start);
    const b = dir(p.start + p.width);
    for (let i = -k; i <= k; i++) {
      const c = i * spacing;
      const half = Math.sqrt(r * r - c * c);
      let lo = -half, hi = half;
      // Point at t: (c·n + t·d); inside the piece: cross(a, p) ≥ 0 and cross(p, b) ≥ 0.
      const px = c * nx, py = c * ny;
      for (const [A, B] of [[cross(a.x, a.y, px, py), cross(a.x, a.y, d.x, d.y)], [cross(px, py, b.x, b.y), cross(d.x, d.y, b.x, b.y)]]) {
        if (Math.abs(B) < 1e-12) {
          if (A < 0) lo = Infinity;
        } else if (B > 0) lo = Math.max(lo, -A / B);
        else hi = Math.min(hi, -A / B);
      }
      if (hi - lo > 0.5) out.push([cx + px + lo * d.x, cy + py + lo * d.y, cx + px + hi * d.x, cy + py + hi * d.y]);
    }
  }
  return out;
}

/** Dots on a `spacing` px grid anchored at the center, inside the wedge (between `r0` and `r`). */
export function dotGrid(cx: number, cy: number, r: number, w: Wedge, spacing = 5, r0 = 0): [number, number][] {
  const out: [number, number][] = [];
  if (r <= 0 || w.width <= 0 || spacing <= 0) return out;
  const k = Math.floor(r / spacing);
  for (let i = -k; i <= k; i++) {
    for (let j = -k; j <= k; j++) {
      const x = i * spacing, y = j * spacing;
      const d2 = x * x + y * y;
      if (d2 > r * r || d2 < r0 * r0) continue;
      if (wedgeContains(w, Math.atan2(y, x) / RAD)) out.push([cx + x, cy + y]);
    }
  }
  return out;
}

/** Which way a side-view sprite faces for a heading (screen degrees): kept when nearly vertical. */
export function facingOf(heading: number, prev: 1 | -1): 1 | -1 {
  const c = Math.cos(heading * RAD);
  return c > 0.2 ? 1 : c < -0.2 ? -1 : prev;
}

/** Ease a drawn angle toward a target along the shorter way round; `k` in 0–1. */
export function easeAngle(from: number, to: number, k: number): number {
  const d = mod360(to - from + 180) - 180;
  return mod360(from + d * Math.min(1, Math.max(0, k)));
}

/** The panel's lines: the three numbers and whether the mechanic is on. */
export function visionLines(v: VisionView): { view: string; depth: string; blind: string; ahead: string | null; mechanic: string } {
  return {
    view: `${v.coverage}% (${Math.round((v.coverage * 360) / 100)}°)`,
    depth: `${v.binocular}°`,
    blind: `${v.blind}°`,
    ahead: v.front_gap ? `${v.front_gap}°` : null,
    mechanic: v.directional
      ? 'Directional vision is on in this world: animals in a blind spot are noticed later, and depth vision helps a hunter strike.'
      : 'Directional vision is off in this world: the cones show what the eyes cover, but detection does not depend on facing.',
  };
}
