// Vision geometry: what a set of eyes sees around the head, from the eyes'
// azimuths and field widths alone (docs/morphology-contract.md). Pure and
// exact (interval arithmetic on the circle, no sampling).
//
// Angles are degrees, 0 = ahead, +90 = right, ±180 = behind. The derived
// numbers the contract names:
//   coverage        fraction of the full circle seen by at least one eye
//   front overlap   degrees within ±60° of straight ahead seen by two or
//                   more eyes (the binocular "depth" zone), less what a long
//                   snout hides: 40°·max(0, snout − 0.5) (contract v1)
//   rear blind spot width of the unseen arc straight behind (through 180°)
// plus the unseen arc straight ahead (through 0°), which two eyes pushed to
// the sides leave and a midline eye closes.

import { DEFAULT_MORPHOLOGY, eyeFieldWidth, eyePlacements, snoutOcclusion, type Morphology } from './morphology';

/** Half-width of the front zone where binocular overlap counts as depth vision. */
export const DEPTH_WINDOW = 60;

/** An arc running clockwise from `start` (−180 ≤ start < 180) for `width` degrees. */
export interface Arc {
  start: number;
  width: number;
}

export interface EyeField {
  /** Signed centre, degrees. */
  azimuth: number;
  /** Field width, degrees (capped at 360). */
  width: number;
}

export interface VisionStats {
  /** Seen by at least one eye, 0–1. */
  coverage: number;
  coveredDeg: number;
  /** Unseen degrees in total. */
  blindDeg: number;
  /** Depth vision: degrees within ±60° seen by two or more eyes, less snout occlusion. */
  overlapDeg: number;
  /** The same before snout occlusion. */
  rawOverlapDeg: number;
  /** Degrees of overlap the snout hides (≤ rawOverlapDeg). */
  occludedDeg: number;
  /** Width of the unseen arc through 180° (0 when straight back is seen). */
  rearBlindDeg: number;
  /** Width of the unseen arc through 0° (0 when straight ahead is seen). */
  frontBlindDeg: number;
  /** Arcs seen by at least one eye. */
  covered: Arc[];
  /** Arcs within ±60° seen by two or more eyes (before snout occlusion). */
  overlap: Arc[];
  /** Every unseen arc. */
  blind: Arc[];
  rearBlind: Arc | null;
  frontBlind: Arc | null;
}

const EPS = 1e-9;
const mod360 = (a: number): number => ((a % 360) + 360) % 360;
/** −180 ≤ a < 180. */
export const signedAngle = (a: number): number => mod360(a + 180) - 180;
const tidy = (x: number): number => Math.round(x * 1e6) / 1e6;

export function visionFields(m: Pick<Morphology, 'eye_count' | 'eye_azimuth' | 'eye_size'>): EyeField[] {
  const width = eyeFieldWidth(m.eye_size);
  return eyePlacements(m).map((e) => ({ azimuth: e.azimuth, width }));
}

interface Segment {
  from: number; // [0, 360)
  to: number;
  count: number;
  front: boolean;
}

/** The circle cut at every field edge (and at the depth window's edges), with how many eyes see each piece. */
function segments(fields: readonly EyeField[]): Segment[] {
  const intervals: [number, number][] = [];
  const cuts = new Set<number>([0, DEPTH_WINDOW, 360 - DEPTH_WINDOW, 360]);
  for (const f of fields) {
    const w = Math.min(360, Math.max(0, f.width));
    if (w <= EPS) continue;
    if (w >= 360 - EPS) {
      intervals.push([0, 360]);
      continue;
    }
    const s = mod360(f.azimuth - w / 2);
    const e = s + w;
    if (e > 360) {
      intervals.push([s, 360], [0, e - 360]);
      cuts.add(s).add(e - 360);
    } else {
      intervals.push([s, e]);
      cuts.add(s).add(e);
    }
  }
  const sorted = [...cuts].sort((a, b) => a - b);
  const out: Segment[] = [];
  for (let i = 0; i + 1 < sorted.length; i++) {
    const from = sorted[i];
    const to = sorted[i + 1];
    if (to - from <= EPS) continue;
    const mid = (from + to) / 2;
    const count = intervals.filter(([s, e]) => s <= mid && mid < e).length;
    out.push({ from, to, count, front: mid < DEPTH_WINDOW || mid > 360 - DEPTH_WINDOW });
  }
  return out;
}

/** Maximal circular runs of segments that satisfy `keep`, as arcs. */
function runs(segs: readonly Segment[], keep: (s: Segment) => boolean): Arc[] {
  const n = segs.length;
  if (segs.every(keep)) return [{ start: -180, width: 360 }];
  // Start just after a segment that fails, so no run is split at 0°.
  const first = segs.findIndex((s) => !keep(s));
  const out: Arc[] = [];
  let cur: { from: number; width: number } | null = null;
  for (let k = 1; k <= n; k++) {
    const s = segs[(first + k) % n];
    if (keep(s)) {
      if (cur) cur.width += s.to - s.from;
      else cur = { from: s.from, width: s.to - s.from };
    } else if (cur) {
      out.push({ start: tidy(signedAngle(cur.from)), width: tidy(cur.width) });
      cur = null;
    }
  }
  if (cur) out.push({ start: tidy(signedAngle(cur.from)), width: tidy(cur.width) });
  return out;
}

/** Whether `arc` contains the direction `a` (strictly inside, or on an edge of a full circle). */
export function arcContains(arc: Arc, a: number): boolean {
  if (arc.width >= 360) return true;
  const d = mod360(a - arc.start);
  return d > EPS && d < arc.width - EPS;
}

/** The vision stats of a set of eye fields; `occlusion` degrees of overlap are hidden (a long snout). */
export function analyseVision(fields: readonly EyeField[], occlusion = 0): VisionStats {
  const segs = segments(fields);
  const covered = runs(segs, (s) => s.count > 0);
  const blind = runs(segs, (s) => s.count === 0);
  const overlap = runs(segs, (s) => s.count >= 2 && s.front);
  const sum = (arcs: Arc[]) => arcs.reduce((t, a) => t + a.width, 0);
  const coveredDeg = tidy(sum(covered));
  const rearBlind = blind.find((a) => arcContains(a, 180)) ?? null;
  const frontBlind = blind.find((a) => arcContains(a, 0)) ?? null;
  const rawOverlapDeg = tidy(sum(overlap));
  const occludedDeg = tidy(Math.min(rawOverlapDeg, Math.max(0, occlusion)));
  return {
    coverage: tidy(coveredDeg / 360),
    coveredDeg,
    blindDeg: tidy(360 - coveredDeg),
    overlapDeg: tidy(rawOverlapDeg - occludedDeg),
    rawOverlapDeg,
    occludedDeg,
    rearBlindDeg: rearBlind?.width ?? 0,
    frontBlindDeg: frontBlind?.width ?? 0,
    covered,
    overlap,
    blind,
    rearBlind,
    frontBlind,
  };
}

/** The vision stats of a morphology (with snout occlusion when `snout_length` is given). */
export function visionOf(m: Pick<Morphology, 'eye_count' | 'eye_azimuth' | 'eye_size'> & { snout_length?: number }): VisionStats {
  return analyseVision(visionFields(m), m.snout_length === undefined ? 0 : snoutOcclusion(m.snout_length));
}

/** The default body's vision: the readouts compare against it. */
export const DEFAULT_VISION: VisionStats = visionOf(DEFAULT_MORPHOLOGY);

/** A plain-English line on what the eyes see. */
export function visionSummary(v: VisionStats): string {
  if (v.blindDeg <= 0) return v.overlapDeg > 0
    ? `Sees all the way round, with ${Math.round(v.overlapDeg)}° of depth vision in front.`
    : 'Sees all the way round, but nothing in front with two eyes at once.';
  const parts: string[] = [];
  if (v.rearBlindDeg > 0) parts.push(`blind ${Math.round(v.rearBlindDeg)}° behind`);
  if (v.frontBlindDeg > 0) parts.push(`blind ${Math.round(v.frontBlindDeg)}° straight ahead`);
  const side = v.blindDeg - v.rearBlindDeg - v.frontBlindDeg;
  if (side > 0.5) parts.push(`blind ${Math.round(side)}° at the sides`);
  parts.push(v.overlapDeg > 0 ? `${Math.round(v.overlapDeg)}° of depth vision in front` : 'no depth vision in front');
  const text = parts.join(', ');
  return `${text[0].toUpperCase()}${text.slice(1)}.`;
}
