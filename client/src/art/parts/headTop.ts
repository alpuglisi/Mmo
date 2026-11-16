// The head seen from above, snout up (STYLE.md, "Parts": top view). The body
// builder's eye panel draws it under its field-of-view diagram, so it uses
// that panel's units: the cranium is centred on the origin, `-y` is ahead.
//
// Same house rules as the side view: one merged silhouette per group
// (cranium + snout + ears; neck + back + rump), painted from the creature's
// palette with one highlight rim and one shadow rim, eyes and nose as inked
// overlays. Ears reuse the side view's construction (`ears.ts`), lying out
// from the skull, foreshortened; the snout reaches `snoutTip` exactly (a
// linear function of snout length, chosen by the caller) so the panel's
// snout-shadow band starts at its tip.

import { lerp, rng, scallopRing, type Point, type Piece, type Shape } from '../geom';
import { GLINT, PUPIL, mix, type CreaturePalette } from '../palette';
import type { Group } from '../render';
import type { EarShape } from '../../morphology';
import { ear, earAxis, earLength } from './ears';
import type { Lod } from './cover';

export interface HeadTopEye {
  /** Where the eye sits (on the skull's outline; a midline eye on the forehead). */
  x: number;
  y: number;
  /** Which way it looks (degrees, 0 ahead, clockwise). */
  az: number;
  /** Iris radius. */
  r: number;
}

export interface HeadTopParams {
  /** Cranium radius (eyes sit on a circle of about this radius). */
  r: number;
  /** Distance from the cranium centre to the snout tip. */
  snoutTip: number;
  /** 0..1 carnivory: herbivore round broad muzzle → carnivore square one. */
  carn: number;
  /** Cold-blooded: a flatter, narrower wedge; small ears are just openings. */
  cold: boolean;
  ears: { count: number; shape: EarShape; size: number };
  eyes: HeadTopEye[];
  /** A furred body gets a soft locked edge. */
  furry: boolean;
  lod: Lod;
}

export interface HeadTopPart {
  /** Neck, back and rump trailing behind (draw it first). */
  body: Group;
  /** Cranium, snout and ears, with the muzzle and nose. */
  head: Group;
  /** One list of shapes per eye (tags `eye` / `glint`), in `eyes` order. */
  eyes: Shape[][];
  anchors: { snoutTip: Point; nose: Point; earTips: Point[] };
}

const piece = (tag: string, points: Point[]): Piece => ({ tag, points, smooth: true });
const mirror = (pts: Point[]): Point[] => pts.map(([x, y]): Point => [-x, y]);
const rad = (deg: number): number => (deg * Math.PI) / 180;
/** The panel's polar convention: azimuth in degrees, 0 up, clockwise. */
const polar = (az: number, d: number): Point => [d * Math.sin(rad(az)), -d * Math.cos(rad(az))];

/** Where each pair of ears stands on the skull (azimuth) and which way it lies (azimuth of its axis). */
const EAR_LIE: Record<EarShape, { at: number; dir: number; fore: number; width: number }> = {
  // Upright ears seen from above are foreshortened (`fore`) and narrowed (`width`).
  round: { at: 110, dir: 108, fore: 1, width: 0.8 },
  pointed: { at: 106, dir: 112, fore: 0.85, width: 0.75 },
  // Long ears lie back along the neck; fans spread out to the sides.
  long: { at: 128, dir: 158, fore: 0.9, width: 0.72 },
  fan: { at: 104, dir: 100, fore: 0.75, width: 0.8 },
};

export function headTop(p: HeadTopParams, pal: CreaturePalette): HeadTopPart {
  const { r, carn } = p;
  const L = p.snoutTip;
  const narrow = p.cold ? 0.84 : 1;
  const head: Group = { part: 'head', tones: pal.body, pieces: [], detail: [], over: [] };
  const body: Group = { part: 'body', tones: pal.body, pieces: [], detail: [], over: [] };

  // --- cranium: a broad egg, widest a little behind the eyes.
  const crown: Point[] = [];
  for (let i = 0; i < 24; i++) {
    const t = (i / 24) * Math.PI * 2;
    const sy = Math.cos(t);
    // Slightly narrower at the front (toward the snout), fuller at the back.
    const w = r * narrow * (1 - 0.06 * Math.max(0, -sy));
    crown.push([Math.sin(t) * w, -sy * r * 1.02 + 1.5]);
  }
  head.pieces.push(piece('cranium', crown));

  // --- snout: from the brow to the tip; carnivores square and broad-tipped, herbivores rounder.
  const wt = r * (0.27 + 0.12 * carn) * narrow * (p.cold ? 0.85 : 1);
  const w0 = r * 0.74 * narrow;
  const y0 = -r * 0.45;
  const at = (f: number) => lerp(y0, -L, f);
  const sq = 0.55 + 0.3 * carn;
  const half: Point[] = [
    [w0, y0],
    [lerp(w0, wt, 0.55), at(0.42)],
    [wt * 1.04, at(0.78)],
    [wt * (0.82 + 0.1 * carn), at(0.93)],
    [wt * sq, at(0.99)],
  ];
  head.pieces.push(piece('snout', [...half, [0, -L], ...mirror(half).reverse()]));

  // --- ears: pairs on the skull, lying out to the sides (the second pair behind and a little smaller).
  const earTips: Point[] = [];
  const earHole = p.cold && p.ears.size < 0.5 && p.ears.count > 0;
  const pairs = Math.floor(p.ears.count / 2);
  if (earHole) {
    for (let k = 0; k < pairs; k++) for (const sgn of [-1, 1]) {
      const [x, y] = polar(sgn * (112 + 26 * k), r * 0.72);
      head.detail.push({ part: 'head', tag: 'ear-hole', kind: 'ellipse', x, y, rx: r * 0.1, ry: r * 0.08, fill: pal.body.shade, stroke: pal.ink, width: 0.8 });
    }
  } else if (pairs > 0) {
    const lie = EAR_LIE[p.ears.shape];
    for (let k = 0; k < pairs; k++) {
      const shrink = k === 0 ? 1 : 0.85;
      const len = earLength(p.ears.shape, p.ears.size, r * 0.72) * lie.fore * shrink;
      const az = lie.at + 26 * k;
      const dir = lie.dir + 20 * k;
      // `ear()` builds an ear leaning back from vertical; aim its axis at azimuth -dir (the left ear), then mirror.
      const [ax, ay] = earAxis(p.ears.shape);
      const extra = rad(dir) - Math.atan2(-ax, -ay);
      const base = polar(-az, r * 0.84);
      const left = ear(p.ears.shape, base, len, r * 0.72 * lie.width * shrink, extra);
      head.pieces.push(piece(`ear-${k}-l`, left.points), piece(`ear-${k}-r`, mirror(left.points)));
      // From above an upright ear shows only a sliver of its inside, toward the front.
      const inner = left.inner.map(([x, y]): Point => [x + r * 0.03, y - r * 0.05]);
      const innerFill = mix(pal.inner, pal.body.shade, 0.25);
      head.detail.push(
        { part: 'head', tag: 'inner-ear', kind: 'curve', closed: true, points: inner, fill: innerFill, alpha: 0.9 },
        { part: 'head', tag: 'inner-ear', kind: 'curve', closed: true, points: mirror(inner), fill: innerFill, alpha: 0.9 },
      );
      earTips.push(left.tip, [-left.tip[0], left.tip[1]]);
    }
  }

  // --- face markings (clipped): a pale muzzle on non-predators, whisker pores.
  const nose: Point = [0, -L + Math.max(2.5, wt * 0.45)];
  if (carn < 0.67) head.detail.push({ part: 'head', tag: 'muzzle', kind: 'ellipse', x: 0, y: -L + wt * 1.25, rx: wt * 1.08, ry: wt * 1.55, fill: pal.belly, alpha: 0.85 });
  if (!p.cold && p.lod === 'portrait') {
    for (const sgn of [-1, 1]) for (let i = 0; i < 3; i++) {
      head.detail.push({ part: 'head', tag: 'whisker', kind: 'ellipse', x: sgn * wt * (0.45 + 0.18 * i), y: -L + wt * (1.35 + 0.32 * i), rx: 0.55, ry: 0.55, fill: pal.body.shade, alpha: 0.8 });
    }
  }
  // Brow ridges on predators (thin lines in the shade tone, like the side view's inner lines).
  if (carn >= 0.67) for (const sgn of [-1, 1]) {
    head.detail.push({ part: 'head', tag: 'brow-ridge', kind: 'curve', closed: false, points: [[sgn * r * 0.15, -r * 0.42], [sgn * r * 0.38, -r * 0.5], [sgn * r * 0.58, -r * 0.4]], stroke: pal.body.shade, width: 1.2 });
  }

  // --- nose: a pad at the tip (warm), nostrils on every snout.
  if (!p.cold) {
    const nw = wt * (0.62 + 0.12 * carn);
    head.over.push(
      { part: 'head', tag: 'nose', kind: 'curve', closed: true, points: [[-nw, nose[1] - nw * 0.25], [0, nose[1] - nw * 0.5], [nw, nose[1] - nw * 0.25], [nw * 0.55, nose[1] + nw * 0.5], [0, nose[1] + nw * 0.62], [-nw * 0.55, nose[1] + nw * 0.5]], fill: pal.nose, stroke: pal.ink, width: 0.9 },
      { part: 'head', tag: 'nose', kind: 'ellipse', x: -nw * 0.35, y: nose[1] - nw * 0.12, rx: nw * 0.28, ry: nw * 0.14, fill: GLINT, alpha: 0.35 },
    );
  } else {
    for (const sgn of [-1, 1]) head.over.push({ part: 'head', tag: 'nose', kind: 'ellipse', x: sgn * wt * 0.35, y: nose[1], rx: wt * 0.12, ry: wt * 0.18, fill: pal.ink });
  }

  // --- body: neck, shoulders, back and rump trailing behind (which way is front).
  const bw = p.cold ? 0.8 : 1;
  const side: Point[] = [
    [r * 0.55 * bw, r * 0.1],
    [r * 0.98 * bw, r * 0.95],
    [r * 0.8 * bw, r * 1.8],
    [r * 0.88 * bw, r * 2.38],
    [r * 0.5 * bw, r * 2.84],
  ];
  const back: Point[] = [...side, [0, r * 2.97], ...mirror(side).reverse()];
  body.pieces.push(piece('body', p.furry && p.lod === 'portrait' ? scallopRing(back, 24, () => 2.6, 0, rng(11), true) : back));
  // The spine and the shoulder blades: thin lines in the shade tone.
  body.detail.push(
    { part: 'body', tag: 'spine', kind: 'curve', closed: false, points: [[0, r * 1.05], [0, r * 1.8], [0, r * 2.55]], stroke: pal.body.shade, width: 1, alpha: 0.7 },
    ...[-1, 1].map((sgn): Shape => ({ part: 'body', tag: 'shoulder', kind: 'curve', closed: false, points: [[sgn * r * 0.2, r * 0.95], [sgn * r * 0.5, r * 1.05], [sgn * r * 0.62, r * 1.35]], stroke: pal.body.shade, width: 0.9, alpha: 0.6 })),
  );

  return { body, head, eyes: p.eyes.map((e) => eyeTop(e, p, pal)), anchors: { snoutTip: [0, -L], nose, earTips } };
}

/** One eye from above: an inked socket along the skull's outline, iris and pupil looking out along `az`, glint to the top left. */
function eyeTop(e: HeadTopEye, p: HeadTopParams, pal: CreaturePalette): Shape[] {
  const { r, az } = e;
  const rot = rad(az);
  const [ox, oy] = polar(az, 1);
  // The eyeball bulges a little out of the outline.
  const x = e.x + ox * r * 0.12;
  const y = e.y + oy * r * 0.12;
  const P = 'head' as const;
  const out: Shape[] = [
    { part: P, tag: 'eye', kind: 'ellipse', x, y, rx: r * 1.18 + 0.7, ry: r * 0.92 + 0.6, rot, fill: pal.ink },
    { part: P, tag: 'eye', kind: 'ellipse', x, y, rx: r, ry: r * 0.76, rot, fill: pal.iris },
  ];
  // The pupil sits toward the outer side (where the eye looks).
  const px = x + ox * r * 0.18;
  const py = y + oy * r * 0.18;
  if (p.cold) out.push({ part: P, tag: 'eye', kind: 'ellipse', x: px, y: py, rx: r * 0.86, ry: r * 0.2, rot, fill: PUPIL });
  else out.push({ part: P, tag: 'eye', kind: 'ellipse', x: px, y: py, rx: r * 0.62, ry: r * 0.5, rot, fill: PUPIL });
  if (p.lod === 'portrait') {
    // The lid: a band of ink along the inner (skull) side gives the look intent.
    const [tx, ty] = [Math.cos(rot), Math.sin(rot)];
    const lid = (u: number, v: number): Point => [x + tx * u * r - ox * v * r, y + ty * u * r - oy * v * r];
    out.push({ part: P, tag: 'eye', kind: 'curve', closed: false, points: [lid(-1.1, 0.15), lid(0, 0.98), lid(1.1, 0.15)], stroke: pal.ink, width: Math.max(0.9, r * 0.2) });
  }
  // Light from the top left, wherever the eye faces.
  out.push({ part: P, tag: 'glint', kind: 'ellipse', x: x - r * 0.3, y: y - r * 0.3, rx: r * 0.27, ry: r * 0.27, fill: GLINT, alpha: 0.95 });
  if (p.lod === 'portrait' && r > 2.5) out.push({ part: P, tag: 'eye', kind: 'ellipse', x: x + r * 0.3, y: y + r * 0.28, rx: r * 0.12, ry: r * 0.12, fill: GLINT, alpha: 0.6 });
  return out;
}
