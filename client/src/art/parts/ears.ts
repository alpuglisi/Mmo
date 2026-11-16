// Ears (STYLE.md, "Ears"): four silhouettes on one construction. An ear
// stands on a base point, leans back from vertical by `tilt` and reaches
// `len` along its axis; the tip is always `base + axis · len`, so the
// builder's size handle can ride it.

import type { EarShape } from '../../morphology';
import type { Point } from '../geom';

export interface EarPart {
  /** Outer silhouette (a piece of the head's group). */
  points: Point[];
  /** Inner ear (a detail inside it). */
  inner: Point[];
  tip: Point;
}

/** How far back from vertical each shape leans (radians). */
const TILT: Record<EarShape, number> = { round: 0.35, pointed: 0.32, long: 0.62, fan: 0.95 };
/** Base width as a fraction of the head radius. */
const BASE: Record<EarShape, number> = { round: 0.62, pointed: 0.6, long: 0.42, fan: 0.75 };

/** The ear's axis direction (unit vector, pointing from base to tip). */
export function earAxis(shape: EarShape, extraTilt = 0): Point {
  const a = TILT[shape] + extraTilt;
  return [-Math.sin(a), -Math.cos(a)];
}

/** Length of an ear of this shape and size (0..1) on a head of radius hr. */
export function earLength(shape: EarShape, size: number, hr: number): number {
  const k = shape === 'long' ? 1.9 : shape === 'fan' ? 1.35 : shape === 'round' ? 0.85 : 1.15;
  return hr * k * (0.55 + 0.85 * size);
}

export function ear(shape: EarShape, base: Point, len: number, hr: number, extraTilt = 0): EarPart {
  const [ax, ay] = earAxis(shape, extraTilt);
  // Across the ear (perpendicular to the axis, toward the front).
  const [px, py] = [-ay, ax];
  const at = (along: number, across: number): Point => [base[0] + ax * len * along + px * across, base[1] + ay * len * along + py * across];
  const w = hr * BASE[shape];
  let pts: Point[];
  switch (shape) {
    case 'pointed':
      // Cat / fox: straight-ish sides, a soft point.
      pts = [at(-0.05, w * 0.55), at(0.45, w * 0.42), at(0.86, w * 0.12), at(1, 0), at(0.86, -w * 0.14), at(0.4, -w * 0.48), at(-0.05, -w * 0.5)];
      break;
    case 'long':
      // Hare / donkey: narrow base, widest past the middle, rounded tip.
      pts = [at(-0.05, w * 0.42), at(0.3, w * 0.62), at(0.68, w * 0.62), at(0.93, w * 0.3), at(1, 0), at(0.92, -w * 0.3), at(0.62, -w * 0.5), at(0.25, -w * 0.48), at(-0.05, -w * 0.4)];
      break;
    case 'fan':
      // A broad rounded fan lying back (fennec / elephant).
      pts = [at(-0.05, w * 0.4), at(0.35, w * 0.95), at(0.8, w * 0.85), at(1, 0.1 * w), at(0.85, -w * 0.7), at(0.45, -w * 0.95), at(0.05, -w * 0.55)];
      break;
    case 'round':
    default:
      // Bear / mouse: a rounded half disc.
      pts = [at(-0.05, w * 0.55), at(0.5, w * 0.62), at(0.88, w * 0.38), at(1, 0), at(0.88, -w * 0.4), at(0.5, -w * 0.64), at(-0.05, -w * 0.56)];
      break;
  }
  // Inner ear: the outline drawn in toward the axis and down toward the base.
  const c: Point = at(0.45, 0);
  const inner = pts.slice(1, -1).map(([x, y]): Point => [c[0] + (x - c[0]) * 0.62, c[1] + (y - c[1]) * 0.66 + len * 0.04]);
  return { points: pts, inner, tip: at(1, 0) };
}
