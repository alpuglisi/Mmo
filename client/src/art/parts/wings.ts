// Wings (STYLE.md, "Wings"): a folded-up wing raised over the back, either
// feathered (furred bodies grow plumage) or a membrane on long fingers
// (skin, scales, armor). Gliding flaps are a membrane between the legs.

import type { Point, Shape } from '../geom';
import { mix, type CreaturePalette } from '../palette';

export interface WingParams {
  root: Point;
  /** Wing span (creature units). */
  span: number;
  feathered: boolean;
  far: boolean;
  lod: 'portrait' | 'sprite';
}

export interface WingPart {
  pieces: Point[][];
  detail: Shape[];
}

export function wing(p: WingParams, pal: CreaturePalette): WingPart {
  const { root, span } = p;
  const o = (x: number, y: number): Point => [root[0] + x * span, root[1] + y * span];
  const part = p.far ? 'wing-far' : 'wing-near';
  const shade = p.far ? pal.far.shade : pal.body.shade;
  const pieces: Point[][] = [];
  const detail: Shape[] = [];
  if (p.feathered) {
    // Folded along the flank: rounded at the shoulder, coverts in front,
    // primaries tapering back past the rump to a point. `span` is its length.
    const h = span * 0.44;
    const up = 0.16; // tip raised a little above horizontal
    const at = (u: number, v: number): Point => [root[0] - u * span * Math.cos(up) + v * Math.sin(up) * 0, root[1] - u * span * Math.sin(up) + v * h];
    const top: Point[] = [at(-0.06, 0.05), at(0.04, -0.32), at(0.25, -0.42), at(0.55, -0.34), at(0.85, -0.14), at(1, 0)];
    const bottom: Point[] = [];
    // Primary tips along the lower edge, from the tip forward.
    const tipsU = [0.94, 0.84, 0.74, 0.64, 0.54];
    for (const u of tipsU) bottom.push(at(u, 0.12 + (0.94 - u) * 0.9), at(u - 0.05, 0.02 + (0.94 - u) * 0.9));
    bottom.push(at(0.38, 0.5), at(0.18, 0.56), at(0.02, 0.4));
    pieces.push([...top, ...bottom]);
    // Coverts: the paler front half, with a scalloped trailing edge.
    const cov: Point[] = [at(-0.06, 0.05), at(0.04, -0.32), at(0.25, -0.42), at(0.45, -0.38)];
    for (let i = 0; i <= 5; i++) {
      const v = -0.3 + (0.85 * i) / 5;
      cov.push(at(0.48 - 0.04 * (i % 2), v));
    }
    cov.push(at(0.2, 0.55), at(0.02, 0.4));
    detail.push({ part, kind: 'curve', closed: true, points: cov, fill: p.far ? pal.far.light : pal.body.light, alpha: 0.9 });
    if (p.lod === 'portrait') {
      for (let i = 0; i < tipsU.length; i++) {
        const u = tipsU[i];
        const v = 0.07 + (0.94 - u) * 0.9;
        detail.push({ part, kind: 'line', points: [at(0.45, v * 0.45 - 0.12), at(u - 0.02, v)], stroke: shade, width: 1.2, alpha: 0.9 });
      }
      for (const u of [0.14, 0.3]) detail.push({ part, kind: 'curve', closed: false, points: [at(u, -0.3), at(u + 0.05, 0.05), at(u, 0.45)], stroke: shade, width: 1, alpha: 0.6 });
    }
  } else {
    // Membrane: arm to the wrist, three long fingers, scalloped trailing edge.
    const wrist = o(-0.22, -0.78);
    const tips: Point[] = [o(-1.3, -0.95), o(-1.35, -0.5), o(-1.0, -0.08)];
    const back: Point = o(-0.5, 0.1);
    const sc = (a: Point, b: Point): Point => [(a[0] + b[0]) / 2 + span * 0.05, (a[1] + b[1]) / 2 + span * 0.1];
    pieces.push([o(0.06, 0.06), o(-0.02, -0.45), wrist, tips[0], sc(tips[0], tips[1]), tips[1], sc(tips[1], tips[2]), tips[2], sc(tips[2], back), back]);
    detail.push({ part, kind: 'curve', closed: true, points: [wrist, tips[0], sc(tips[0], tips[1]), tips[1], sc(tips[1], tips[2]), tips[2], sc(tips[2], back), back, o(0, 0)], fill: p.far ? mix(pal.membrane, pal.ink, 0.25) : pal.membrane, alpha: 0.9 });
    for (const t of tips) detail.push({ part, kind: 'line', points: [wrist, t], stroke: shade, width: 2, alpha: 0.9 });
    detail.push({ part, kind: 'line', points: [o(0.02, 0), o(-0.05, -0.45), wrist], stroke: shade, width: 3, alpha: 0.6 });
    detail.push({ part, kind: 'poly', points: [wrist, [wrist[0] + 4, wrist[1] - 6], [wrist[0] + 6, wrist[1] - 1]], fill: pal.bone, stroke: pal.ink, width: 0.8 });
  }
  return { pieces, detail };
}

/** Gliding flaps: a membrane hung between the front and hind legs. */
export function flaps(front: Point, hind: Point, depth: number, pal: CreaturePalette): { piece: Point[]; detail: Shape[] } {
  const yb = Math.max(front[1], hind[1]) + depth;
  const piece: Point[] = [front, hind, [hind[0] - 3, yb]];
  for (let i = 1; i <= 4; i++) {
    const x = hind[0] + ((front[0] - hind[0]) * i) / 4;
    piece.push([x - (front[0] - hind[0]) / 8, yb - depth * 0.3], [x + 2, yb]);
  }
  return { piece, detail: [{ part: 'body', kind: 'curve', closed: true, points: piece, fill: pal.membrane, alpha: 0.9 }] };
}
