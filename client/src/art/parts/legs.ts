// Legs and feet (STYLE.md, "Legs"). A leg is a tapering limb round a jointed
// centreline, topped by a muscle mass (shoulder or thigh) that sits inside the
// body outline: in the merged silhouette it reads as the leg growing out of
// the body, and its contour is drawn faintly across the flank. Feet stand on
// the ground (y = 0) and come in four kinds that share one footprint.

import { ellipsePts, limbOutline, type Point, type Shape } from '../geom';
import { mix, type CreaturePalette } from '../palette';

export type FootStyle = 'plain' | 'hoof' | 'claw' | 'webbed';

export interface LegParams {
  /** Hip / shoulder joint (inside the body). */
  top: Point;
  /** Leg length (top to ground). */
  len: number;
  /** Limb width at the knee. */
  w: number;
  front: boolean;
  /** Cold-blooded legs sprawl: the knee juts out at belly height. */
  sprawl: boolean;
  /** 0..1, bulks up the thigh / shoulder. */
  muscle: number;
  /** Belly line under the joint (sprawled knees sit there). */
  belly: number;
}

export interface LegPart {
  /** The limb (a piece), the muscle mass on top (a piece) and its contour for the flank. */
  limb: Point[];
  mass: Point[];
  /** The muscle's contour across the flank (an open arc on its leading or trailing side). */
  contour: Point[];
  joints: Point[];
  /** Where the foot goes (on the ground). */
  ankle: Point;
}

export function leg(p: LegParams): LegPart {
  const { top, len: L, w, front } = p;
  const g = -w * 0.35; // ankle height
  let joints: Point[];
  if (p.sprawl) {
    const dir = front ? 1 : -1;
    joints = [top, [top[0] + dir * L * 0.42, p.belly + L * 0.05], [top[0] + dir * L * 0.32, g]];
  } else if (front) {
    // Shoulder, elbow, wrist: almost straight, a slight forward wrist.
    joints = [top, [top[0] - L * 0.04, top[1] + (g - top[1]) * 0.5], [top[0] + L * 0.03, top[1] + (g - top[1]) * 0.85], [top[0] + L * 0.05, g]];
  } else {
    // Hip, knee (forward), hock (back), foot.
    joints = [top, [top[0] + L * 0.16, top[1] + (g - top[1]) * 0.42], [top[0] - L * 0.1, top[1] + (g - top[1]) * 0.72], [top[0] - L * 0.02, g]];
  }
  const n = joints.length;
  const widths = joints.map((_, i) => (i === 0 ? w * (1.45 + 0.35 * p.muscle) : i === n - 1 ? w * 0.62 : w * (1.05 - (0.35 * i) / n)));
  const limb = limbOutline(joints, widths);
  const mw = w * (1.3 + 0.45 * p.muscle) * (front ? 0.95 : 1.15);
  const mx = top[0] + (front ? 0 : -mw * 0.1);
  const my = top[1] + mw * 0.35;
  const rot = front ? 0.1 : -0.15;
  const mass = ellipsePts(mx, my, mw, mw * 1.25, 18, rot);
  // Hind legs: the front of the thigh, from the flank down to the knee; front legs: the back of the shoulder.
  const contour: Point[] = [];
  const [a0, a1] = front ? [Math.PI * 0.62, Math.PI * 1.25] : [-Math.PI * 0.35, Math.PI * 0.3];
  for (let i = 0; i <= 6; i++) {
    const t = a0 + ((a1 - a0) * i) / 6;
    const x = Math.cos(t) * mw, y = Math.sin(t) * mw * 1.25;
    contour.push([mx + x * Math.cos(rot) - y * Math.sin(rot), my + x * Math.sin(rot) + y * Math.cos(rot)]);
  }
  return { limb, mass, contour, joints, ankle: joints[n - 1] };
}

export interface FootPart {
  /** Silhouette pieces (in the leg's group). */
  pieces: Point[][];
  /** Colour and line details inside the silhouette. */
  detail: Shape[];
  /** Overlays with their own ink (claws). */
  over: Shape[];
}

/**
 * A foot at `ankle`, `w` wide (the leg width), facing +x. `claw` 0..1 is
 * claw length; `dig` makes them heavier.
 */
export function foot(style: FootStyle, ankle: Point, w: number, width: number, pal: CreaturePalette, opt: { far: boolean; claw: number; dig: boolean; climb: boolean; lod: 'portrait' | 'sprite' }): FootPart {
  const [x] = ankle;
  const fw = w * (0.95 + 0.9 * width);
  const fh = w * 0.62;
  const part = opt.far ? 'legs-far' : 'legs-near';
  const ink = pal.ink;
  const detail: Shape[] = [];
  const over: Shape[] = [];
  let pieces: Point[][];
  if (style === 'hoof') {
    const hoof: Point[] = [[x - fw * 0.42, -fh * 1.05], [x + fw * 0.3, -fh * 1.05], [x + fw * 0.62, 0], [x - fw * 0.48, 0]];
    pieces = [hoof];
    detail.push({ part, kind: 'poly', points: hoof, fill: opt.far ? mix(pal.hoof, ink, 0.3) : pal.hoof });
    detail.push({ part, kind: 'line', points: [[x - fw * 0.4, -fh * 1.0], [x + fw * 0.32, -fh * 1.0]], stroke: mix(pal.hoof, 0xffffff, 0.35), width: 1.2, alpha: 0.7 });
    if (opt.lod === 'portrait') detail.push({ part, kind: 'line', points: [[x + fw * 0.12, -fh * 0.95], [x + fw * 0.2, -0.5]], stroke: ink, width: 0.9, alpha: 0.7 });
  } else if (style === 'webbed') {
    // A paddle: three toes splayed forward with webbing between.
    const toes: Point[] = [[x + fw * 1.05, -1], [x + fw * 0.8, -fh * 0.25], [x + fw * 1.0, -fh * 0.45]];
    pieces = [[[x - fw * 0.35, -fh * 0.9], [x + fw * 0.35, -fh * 0.95], [x + fw * 1.1, -fh * 0.55], [x + fw * 1.2, -1], [x + fw * 0.2, 0], [x - fw * 0.45, -0.5]]];
    detail.push({ part, kind: 'poly', points: [[x + fw * 0.2, -fh * 0.6], [x + fw * 1.15, -fh * 0.5], [x + fw * 1.15, -0.8], [x + fw * 0.2, -0.5]], fill: pal.membrane, alpha: 0.85 });
    for (const t of toes) detail.push({ part, kind: 'line', points: [[x + fw * 0.15, -fh * 0.55], t], stroke: mix(pal.body.shade, ink, 0.4), width: 1, alpha: 0.8 });
  } else {
    // A rounded paw (plain or clawed).
    pieces = [ellipsePts(x + fw * 0.22, -fh * 0.55, fw * 0.66, fh * 0.6, 14)];
    if (opt.lod === 'portrait' && style === 'plain') {
      for (let i = 0; i < 2; i++) detail.push({ part, kind: 'line', points: [[x + fw * (0.42 + 0.22 * i), -fh * 1.05], [x + fw * (0.45 + 0.22 * i), -fh * 0.55]], stroke: ink, width: 0.8, alpha: 0.55 });
    }
    if (style === 'claw') {
      const cl = w * (0.35 + 0.9 * opt.claw) * (opt.dig ? 1.35 : 1);
      const hook = opt.climb ? 0.7 : 0.25;
      for (let i = 0; i < 3; i++) {
        const bx = x + fw * (0.5 + 0.17 * i);
        const by = -fh * (0.5 - 0.08 * i);
        over.push({
          part, tag: 'claw', kind: 'curve', closed: true,
          points: [[bx - cl * 0.12, by - cl * 0.12], [bx + cl * 0.6, by + cl * 0.05 - cl * hook * 0.1], [bx + cl * 0.95, Math.min(-0.3, by + cl * 0.6 * hook + cl * 0.2)], [bx + cl * 0.45, by + cl * 0.22], [bx - cl * 0.1, by + cl * 0.18]],
          fill: opt.far ? mix(pal.bone, ink, 0.35) : pal.bone, stroke: ink, width: 0.9,
        });
      }
    }
  }
  return { pieces, detail, over };
}
