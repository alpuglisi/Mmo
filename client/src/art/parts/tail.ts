// Tails (STYLE.md, "Tails"): one tapering centreline from the rump, then a
// tip treatment per shape. The centreline's end moves linearly with length,
// so the builder's handle rides the tip. The root sits inside the body
// outline; the tail is its own group behind the body, so the body hides the
// join and the tail can sway on its own.

import type { TailShape } from '../../morphology';
import { bezier3, ellipsePts, limbOutline, rng, scallopRing, type Point, type Shape } from '../geom';
import { mix, type CreaturePalette } from '../palette';

export interface TailParams {
  root: Point;
  /** 0..1 length. */
  len: number;
  shape: TailShape;
  /** Width at the root. */
  base: number;
  /** Warm bodies carry the tail out and up; cold ones trail it. */
  warm: boolean;
  /** Furred (a tuft on a thin tail, a soft edge on a bushy one). */
  furry: number;
  /** Plumage: a thin or bushy tail becomes a fan of tail feathers. */
  feathered?: boolean;
  lod: 'portrait' | 'sprite';
}

export interface TailPart {
  pieces: Point[][];
  detail: Shape[];
  over: Shape[];
  /** The centreline's end (a drag handle rides it). */
  tip: Point;
  centre: Point[];
  widths: number[];
}

/** The centreline end for a tail of length `len` (linear in `len`). */
export function tailTip(root: Point, len: number, warm: boolean): Point {
  return warm ? [root[0] - (10 + 108 * len), root[1] - 4 + 42 * len] : [root[0] - (12 + 132 * len), root[1] + 6 + 44 * len];
}

export function tail(p: TailParams, pal: CreaturePalette): TailPart {
  const { root, len: L, shape, warm } = p;
  const tip = tailTip(root, L, warm);
  const ground = (pt: Point): Point => [pt[0], Math.min(-2, pt[1])];
  const dx = tip[0] - root[0];
  const dy = tip[1] - root[1];
  // Warm: up out of the rump, then droop; cold: along the ground.
  const c1: Point = warm ? [root[0] + dx * 0.35, root[1] - 14 - 22 * L] : [root[0] + dx * 0.4, root[1] + dy * 0.05];
  const c2: Point = warm ? [root[0] + dx * 0.75, root[1] + dy * 0.2 - 10 * L] : [root[0] + dx * 0.75, root[1] + dy * 0.9];
  let centre = bezier3(root, c1, c2, tip, 12).map(ground);
  if (shape === 'prehensile') {
    // Curl the tip into a coil, downward.
    const r0 = 5 + 13 * L;
    const [ax, ay] = centre[centre.length - 2];
    const [bx, by] = centre[centre.length - 1];
    const dir = Math.atan2(by - ay, bx - ax);
    const cx = bx + Math.cos(dir + Math.PI / 2) * r0;
    const cy = by + Math.sin(dir + Math.PI / 2) * r0;
    const a0 = Math.atan2(by - cy, bx - cx);
    for (let i = 1; i <= 9; i++) {
      const a = a0 - (i / 9) * Math.PI * 1.6;
      const r = r0 * (1 - 0.055 * i);
      centre.push(ground([cx + Math.cos(a) * r, cy + Math.sin(a) * r]));
    }
  }
  const n = centre.length;
  const thick = shape === 'club' ? 0.85 : shape === 'fin' ? 0.9 : 1;
  const tipW = shape === 'thin' ? 1.4 : 2.2;
  const widths = centre.map((_, i) => tipW + (p.base * thick - tipW) * (1 - i / (n - 1)) ** 0.85);
  const pieces: Point[][] = [];
  const detail: Shape[] = [];
  const over: Shape[] = [];
  const T = 'tail' as const;

  if (p.feathered && (shape === 'thin' || shape === 'bushy')) {
    // A fan of long rounded tail feathers spreading from the rump.
    const nF = 5;
    const fl = 16 + 52 * L;
    const fw = 5 + 3 * L;
    for (let i = 0; i < nF; i++) {
      const a = Math.PI + (warm ? -0.15 : 0.1) + (i - (nF - 1) / 2) * 0.17;
      const dirx = Math.cos(a), diry = Math.sin(a);
      const r0: Point = [root[0] - 4, root[1] + 2];
      const tipF: Point = [r0[0] + dirx * fl * (1 - 0.06 * Math.abs(i - 2)), r0[1] + diry * fl * (1 - 0.06 * Math.abs(i - 2))];
      const nx = -diry, ny = dirx;
      const feather: Point[] = ([
        [r0[0] + nx * fw * 0.3, r0[1] + ny * fw * 0.3],
        [r0[0] + dirx * fl * 0.6 + nx * fw, r0[1] + diry * fl * 0.6 + ny * fw],
        [tipF[0] + nx * fw * 0.6, tipF[1] + ny * fw * 0.6],
        [tipF[0] + dirx * fw * 0.5, tipF[1] + diry * fw * 0.5],
        [tipF[0] - nx * fw * 0.6, tipF[1] - ny * fw * 0.6],
        [r0[0] + dirx * fl * 0.6 - nx * fw, r0[1] + diry * fl * 0.6 - ny * fw],
        [r0[0] - nx * fw * 0.3, r0[1] - ny * fw * 0.3],
      ] as Point[]).map(ground);
      pieces.push(feather);
      if (p.lod === 'portrait') detail.push({ part: T, kind: 'line', points: ([r0, [tipF[0] - dirx * 2, tipF[1] - diry * 2]] as Point[]).map(ground), stroke: pal.body.shade, width: 1, alpha: 0.8 });
    }
    return { pieces, detail, over, tip, centre, widths };
  }
  if (shape === 'bushy') {
    // A plume: widest two thirds out, with a soft scalloped edge.
    const plume = 6 + 16 * L + 8 * p.furry;
    const w2 = centre.map((_, i) => {
      const t = i / (n - 1);
      return Math.max(widths[i], p.base * 0.7 + plume * 1.6 * Math.sqrt(Math.sin(Math.PI * (0.06 + 0.84 * t))));
    });
    const o = limbOutline(centre, w2);
    // Soft locks round the plume, sweeping toward the tip.
    const fluffed = scallopRing(o, 15, () => 2.5 + 2.5 * p.furry + 2 * L, 0.6, rng(41), true);
    pieces.push(fluffed);
    // A paler tip.
    const e = centre[n - 1];
    detail.push({ part: T, kind: 'ellipse', x: e[0] - 2, y: e[1], rx: plume * 0.95, ry: plume * 0.8, fill: pal.belly, alpha: 0.75 });
    if (p.lod === 'portrait') {
      for (let i = 3; i < n - 1; i += 2) {
        const [x, y] = centre[i];
        detail.push({ part: T, kind: 'curve', closed: false, points: [[x + 3, y - 3], [x - 1, y - 1], [x - 5, y + 1]], stroke: pal.body.shade, width: 1.1, alpha: 0.7 });
      }
    }
    return { pieces, detail, over, tip, centre, widths: w2 };
  }

  pieces.push(limbOutline(centre, widths));

  if (shape === 'thin' && p.furry > 0 && L > 0.15) {
    // A tuft at the tip.
    const [x, y] = centre[n - 1];
    const s = 5 + 9 * p.furry;
    const [px, py] = centre[n - 3];
    const rot = Math.atan2(y - py, x - px);
    // A brush: swells from the tail, then three soft locks to a point.
    const ca = Math.cos(rot), sa = Math.sin(rot);
    const at = (u: number, v: number): Point => [x + ca * u * s - sa * v * s, y + sa * u * s + ca * v * s];
    const tuft = [at(-0.6, 0.15), at(0, 0.5), at(0.9, 0.55), at(1.5, 0.3), at(1.25, 0.12), at(1.9, 0), at(1.25, -0.12), at(1.5, -0.3), at(0.9, -0.55), at(0, -0.5), at(-0.6, -0.15)];
    pieces.push(tuft);
    detail.push({ part: T, kind: 'curve', closed: true, points: tuft, fill: pal.body.shade, alpha: 0.55 });
  }
  if (shape === 'fin') {
    // Dorsal ridge along the top and a two-lobed caudal fin at the tip.
    const ridge = centre.slice(4).map(([x, y], i, a): Point => [x, y - widths[i + 4] / 2 - (3 + 7 * L) * Math.sin((Math.PI * (i + 1)) / (a.length + 1))]);
    const ridgePiece = [...ridge, ...centre.slice(4).reverse()];
    pieces.push(ridgePiece);
    const [ex, ey] = centre[n - 1];
    const [px, py] = centre[n - 3];
    const ang = Math.atan2(ey - py, ex - px);
    const fs = 9 + 16 * L;
    const rot = (u: number, v: number): Point => [ex + Math.cos(ang) * u - Math.sin(ang) * v, ey + Math.sin(ang) * u + Math.cos(ang) * v];
    const fin: Point[] = [rot(-fs * 0.2, 0), rot(fs * 0.6, -fs * 0.95), rot(fs * 0.85, -fs * 0.85), rot(fs * 0.45, 0), rot(fs * 0.85, fs * 0.8), rot(fs * 0.6, fs * 0.9)];
    pieces.push(fin.map(ground));
    detail.push({ part: T, kind: 'curve', closed: true, points: fin.map(ground), fill: pal.membrane, alpha: 0.9 });
    detail.push({ part: T, kind: 'curve', closed: true, points: ridgePiece, fill: pal.membrane, alpha: 0.6 });
    if (p.lod === 'portrait') {
      for (const v of [-0.7, -0.35, 0.35, 0.7]) detail.push({ part: T, kind: 'line', points: [rot(fs * 0.05, 0), rot(fs * 0.7, fs * v * 1.1)].map(ground), stroke: mix(pal.membrane, pal.ink, 0.45), width: 0.8, alpha: 0.7 });
    }
  }
  if (shape === 'club') {
    const [ex, ey] = centre[n - 1];
    const cr = 5 + p.base * 0.45 + 6 * L;
    const kc: Point = [ex - cr * 0.2, Math.min(-cr * 0.9, ey)];
    // A smooth bony knob, paler than the tail, with a pair of blunt spikes.
    const knob = ellipsePts(kc[0], kc[1], cr * 1.25, cr * 0.9, 18);
    pieces.push(knob);
    for (const sgn of [-1, 1]) pieces.push([[kc[0] - cr * 0.55, kc[1] + sgn * cr * 0.6], [kc[0] - cr * 0.15, kc[1] + sgn * cr * 1.45], [kc[0] + cr * 0.35, kc[1] + sgn * cr * 0.6]]);
    detail.push({ part: T, kind: 'ellipse', x: kc[0], y: kc[1], rx: cr * 1.25, ry: cr * 1.6, fill: mix(pal.body.base, pal.bone, 0.5) });
    if (p.lod === 'portrait') {
      over.push({ part: T, kind: 'curve', closed: false, points: [[kc[0] - cr * 0.9, kc[1] + cr * 0.1], [kc[0] - cr * 0.2, kc[1] - cr * 0.2], [kc[0] + cr * 0.6, kc[1] + cr * 0.15]], stroke: pal.ink, width: 0.9, alpha: 0.5 });
    }
  }
  // Rings along the tail (used for armor, warning bands): positions and widths for the caller.
  return { pieces, detail, over, tip, centre, widths };
}
