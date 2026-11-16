// Geometry and the shape vocabulary every part is authored in.
//
// Parts return plain data (points in creature units: ground at y = 0, +x is
// the way the creature faces, y grows downward as in SVG). The renderer
// (`render.ts`) turns that data into SVG; nothing here touches the DOM, so
// parts are unit-tested and can also feed a canvas or a sprite atlas.

import type { Color } from './palette';

export type Point = [number, number];

/** Which part of the body a shape belongs to (the renderer groups and animates by part). */
export type Part = 'shadow' | 'wing-far' | 'legs-far' | 'ears-far' | 'tail' | 'body' | 'legs-near' | 'head' | 'wing-near' | 'badge';

export interface Paint {
  fill?: Color;
  alpha?: number;
  stroke?: Color;
  width?: number;
  /** Stroke ends: round by default. */
  cap?: 'round' | 'butt';
}

/** A detail or overlay shape (markings, eyes, claws, teeth...). */
export type Shape = { part: Part; tag?: string } & Paint & (
  | { kind: 'ellipse'; x: number; y: number; rx: number; ry: number; rot?: number }
  /** Straight-edged polygon. */
  | { kind: 'poly'; points: Point[] }
  /** Smooth closed outline or open stroke through the points (Catmull-Rom). */
  | { kind: 'curve'; points: Point[]; closed: boolean }
  /** Straight polyline stroke. */
  | { kind: 'line'; points: Point[] }
);

/**
 * One piece of a silhouette: a closed outline. Pieces of a `Group` are
 * merged into one shape (one outline round all of them, no seams inside).
 */
export interface Piece {
  /** What it is (body, head, neck, leg-0, ear-1, foot-2, tail...): the tests and handles look pieces up by tag. */
  tag: string;
  points: Point[];
  /** Smooth (Catmull-Rom) or straight-edged. */
  smooth: boolean;
}

export const clamp01 = (v: number): number => Math.max(0, Math.min(1, v));
export const lerp = (a: number, b: number, t: number): number => a + (b - a) * t;
export const lerpPt = (a: Point, b: Point, t: number): Point => [lerp(a[0], b[0], t), lerp(a[1], b[1], t)];
export const add = (a: Point, b: Point): Point => [a[0] + b[0], a[1] + b[1]];

/** Outline of a limb or tail: a polygon around a centerline with a width at each joint. */
export function limbOutline(joints: Point[], widths: number[]): Point[] {
  const left: Point[] = [];
  const right: Point[] = [];
  for (let i = 0; i < joints.length; i++) {
    const a = joints[Math.max(0, i - 1)];
    const b = joints[Math.min(joints.length - 1, i + 1)];
    const tx = b[0] - a[0];
    const ty = b[1] - a[1];
    const len = Math.hypot(tx, ty) || 1;
    const nx = -ty / len;
    const ny = tx / len;
    const hw = widths[i] / 2;
    left.push([joints[i][0] + nx * hw, joints[i][1] + ny * hw]);
    right.push([joints[i][0] - nx * hw, joints[i][1] - ny * hw]);
  }
  return [...left, ...right.reverse()];
}

/** Points along a quadratic Bézier. */
export function bezier(p0: Point, c: Point, p1: Point, n: number): Point[] {
  const out: Point[] = [];
  for (let i = 0; i <= n; i++) {
    const t = i / n;
    const u = 1 - t;
    out.push([u * u * p0[0] + 2 * u * t * c[0] + t * t * p1[0], u * u * p0[1] + 2 * u * t * c[1] + t * t * p1[1]]);
  }
  return out;
}

/** Points along a cubic Bézier. */
export function bezier3(p0: Point, c1: Point, c2: Point, p1: Point, n: number): Point[] {
  const out: Point[] = [];
  for (let i = 0; i <= n; i++) {
    const t = i / n;
    const u = 1 - t;
    const a = u * u * u, b = 3 * u * u * t, c = 3 * u * t * t, d = t * t * t;
    out.push([a * p0[0] + b * c1[0] + c * c2[0] + d * p1[0], a * p0[1] + b * c1[1] + c * c2[1] + d * p1[1]]);
  }
  return out;
}

/** A small deterministic random sequence (markings stay put as sliders move). */
export function rng(seed: number): () => number {
  let s = seed >>> 0;
  return () => {
    s = (s * 1664525 + 1013904223) >>> 0;
    return s / 4294967296;
  };
}

/** An ellipse as a closed point ring (for pieces). */
export function ellipsePts(cx: number, cy: number, rx: number, ry: number, n = 16, rot = 0): Point[] {
  const out: Point[] = [];
  const c = Math.cos(rot), s = Math.sin(rot);
  for (let i = 0; i < n; i++) {
    const t = (i / n) * Math.PI * 2;
    const x = Math.cos(t) * rx, y = Math.sin(t) * ry;
    out.push([cx + x * c - y * s, cy + x * s + y * c]);
  }
  return out;
}

/** Signed area (positive = clockwise in screen coordinates). */
export function area(points: Point[]): number {
  let a = 0;
  for (let i = 0; i < points.length; i++) {
    const [x0, y0] = points[i];
    const [x1, y1] = points[(i + 1) % points.length];
    a += x0 * y1 - x1 * y0;
  }
  return a / 2;
}

export function bounds(points: Point[]): { minX: number; maxX: number; minY: number; maxY: number } {
  let minX = Infinity, maxX = -Infinity, minY = Infinity, maxY = -Infinity;
  for (const [x, y] of points) {
    minX = Math.min(minX, x); maxX = Math.max(maxX, x); minY = Math.min(minY, y); maxY = Math.max(maxY, y);
  }
  return { minX, maxX, minY, maxY };
}

/**
 * A closed outline re-drawn with soft locks or scallops along its edge: the
 * perimeter is split into `n` equal spans and each span bulges out along the
 * local normal by `reach(i)` (fur: a lock that rises and sweeps toward -x by
 * `lean`; feathers / plumes with `round`: a broad rounded scallop). Works for
 * any outline (torso, tail plume, ruff), unlike a radial push.
 */
export function scallopRing(outline: Point[], n: number, reach: (i: number) => number, lean: number, R: () => number, round = false): Point[] {
  // Perimeter sampler.
  const m = outline.length;
  const seg: number[] = [];
  let total = 0;
  for (let i = 0; i < m; i++) {
    const a = outline[i], b = outline[(i + 1) % m];
    const l = Math.hypot(b[0] - a[0], b[1] - a[1]);
    seg.push(l);
    total += l;
  }
  const at = (u: number): { p: Point; nrm: Point } => {
    let d = (((u % 1) + 1) % 1) * total;
    let i = 0;
    while (d > seg[i] && i < m - 1) d -= seg[i++];
    const a = outline[i], b = outline[(i + 1) % m];
    const t = seg[i] > 0 ? d / seg[i] : 0;
    const tx = b[0] - a[0], ty = b[1] - a[1];
    const l = Math.hypot(tx, ty) || 1;
    // Outward normal for a clockwise (screen) outline.
    const sgn = area(outline) > 0 ? 1 : -1;
    return { p: [a[0] + tx * t, a[1] + ty * t], nrm: [(ty / l) * sgn, (-tx / l) * sgn] };
  };
  const prof: [number, number, number][] = round
    ? [[0, 0, 0], [0.2, 0.7, 0.1], [0.5, 1, 0.2], [0.8, 0.7, 0.15]]
    : [[0, 0, 0], [0.3, 0.55, 0.15], [0.58, 0.95, 0.45], [0.78, 0.5, 0.35]];
  const pts: Point[] = [];
  for (let i = 0; i < n; i++) {
    const k = reach(i) * (0.8 + 0.4 * R());
    for (const [along, out, back] of prof) {
      const { p, nrm } = at((i + along) / n);
      pts.push([p[0] + nrm[0] * k * out - k * back * lean, p[1] + nrm[1] * k * out]);
    }
  }
  return pts;
}
