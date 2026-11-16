// Torso: one soft bean-shaped silhouette for every build (STYLE.md, "Bodies").
//
// The outline is a superellipse (rounder → boxier with `boxy`) made deeper at
// the chest than the rump, with a belly that sags with `sag` and withers that
// rise with `hump`. Everything else attaches to its anchors.

import { clamp01, type Point } from '../geom';

export interface BodyParams {
  /** Half length and half height (creature units). */
  rx: number;
  ry: number;
  /** Centre height (negative: above the ground). */
  cy: number;
  /** 0..1: deeper chest than rump (warm quadrupeds ~0.6, lizards ~0.15). */
  chest: number;
  /** 0..1: belly sag (fat). */
  sag: number;
  /** 0..1: withers / shoulder hump (muscle). */
  hump: number;
  /** 0..1: rounder (0) to boxier (1) superellipse (stocky builds). */
  boxy: number;
}

export interface BodyPart {
  params: BodyParams;
  /** Closed outline (32 points, clockwise from the chest). */
  points: Point[];
  /** Edge at angle t: 0 front, π/2 belly, -π/2 back. */
  edge: (t: number) => Point;
  center: Point;
  /** Inside the body scaled by k about its centre? */
  inside: (x: number, y: number, k?: number) => boolean;
  anchors: {
    /** Upper and lower roots of the neck. */
    neckTop: Point;
    neckLow: Point;
    /** Where the tail leaves the rump (tucked inside the outline). */
    tailRoot: Point;
    /** Top of the back over the shoulders and the hips. */
    withers: Point;
    rump: Point;
  };
}

export function body(p: BodyParams): BodyPart {
  const { rx, ry, cy } = p;
  const n = 2 + 0.9 * clamp01(p.boxy);
  const pw = (v: number) => Math.sign(v) * Math.abs(v) ** (2 / n);
  const edge = (t: number): Point => {
    const c = Math.cos(t);
    const s = Math.sin(t);
    let x = rx * pw(c);
    let y = ry * pw(s);
    // Chest deeper than the rump; belly sags toward the middle.
    y *= 1 + 0.16 * p.chest * c;
    if (s > 0) y *= 1 + 0.28 * p.sag * s * s;
    // Withers over the shoulders, a softer rise over the hips.
    if (s < 0) {
      y -= ry * 0.2 * p.hump * Math.exp(-((t + 1.05) ** 2) / 0.12);
      y -= ry * 0.06 * Math.exp(-((t + 2.25) ** 2) / 0.2);
    }
    // A slightly rounder, fuller rump.
    if (c < 0) x *= 1 + 0.04 * (1 - p.chest);
    return [x, cy + y];
  };
  const points: Point[] = [];
  for (let i = 0; i < 32; i++) points.push(edge((i / 32) * Math.PI * 2));
  return {
    params: p,
    points,
    edge,
    center: [0, cy],
    inside: (x, y, k = 0.85) => (x / (rx * k)) ** 2 + ((y - cy) / (ry * k)) ** 2 < 1,
    anchors: {
      neckTop: edge(-0.95),
      neckLow: edge(0.45),
      tailRoot: [-rx * 0.82, cy - ry * 0.25],
      withers: edge(-1.05),
      rump: edge(-2.3),
    },
  };
}
