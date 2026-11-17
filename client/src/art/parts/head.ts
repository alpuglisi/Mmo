// Head, neck, muzzle and the face furniture that sits on them: eyes, nose,
// mouth, teeth, fangs / tusks, horns, whiskers (STYLE.md, "Heads", "Eyes").
//
// One cranium for every species; diet bends the muzzle (herbivore: long,
// deep and round-nosed; carnivore: shorter, square, strong jaw and brow;
// omnivore between) and `snout` lengthens it continuously. The snout tip
// moves linearly with `snout`, so a drag handle can ride it.

import { lerp, limbOutline, type Point, type Shape } from '../geom';
import { GLINT, PUPIL, mix, type CreaturePalette } from '../palette';

export interface HeadParams {
  /** Cranium centre and radius. */
  hx: number;
  hy: number;
  hr: number;
  /** 0..1 snout length. */
  snout: number;
  /** 0..1 carnivory: muzzle shape. */
  carn: number;
  /** Cold-blooded: a flatter wedge head. */
  cold: boolean;
}

export interface HeadPart {
  params: HeadParams;
  points: Point[];
  anchors: {
    snoutTip: Point;
    /** Top of the nose (where the nostril and nose pad sit). */
    nose: Point;
    /** Mouth line: front, middle, corner. */
    mouth: [Point, Point, Point];
    /** Where fangs drop from (upper jaw, behind the nose) and tusks rise from (lower jaw). */
    fangRoot: Point;
    tuskRoot: Point;
    hornBase: Point;
    /** Behind the eye: cheek tuft, ear opening, poison gland. */
    cheek: Point;
    /** Throat and back of the skull (neck roots). */
    throat: Point;
    nape: Point;
    /** Muzzle centre and size (paler muzzle patch). */
    muzzle: { x: number; y: number; rx: number; ry: number };
  };
  /** Snout length in units and depth (the face furniture scales with them). */
  snL: number;
  snD: number;
}

export function head(p: HeadParams): HeadPart {
  const { hx, hy, hr, carn: c } = p;
  const flat = p.cold ? 0.78 : 1;
  // Snout: linear in `snout`; carnivores a little shorter and shallower.
  const snL = hr * (0.3 + 1.05 * p.snout) * (1 - 0.18 * c);
  const snD = hr * (0.92 - 0.26 * c) * (p.cold ? 0.8 : 1);
  const tipX = hx + hr * 0.6 + snL;
  // Herbivores' noses droop a little (a roman nose); carnivores' muzzles are level.
  const tipTop = hy - hr * 0.28 * flat + hr * 0.1 * (1 - c) * p.snout;
  const brow = 0.06 + 0.12 * c;
  const jaw = 1 + 0.12 * c;
  const y = (v: number) => hy + v * hr * flat;
  const points: Point[] = [
    [hx - hr * 0.55, y(0.78 * jaw)],
    [hx - hr * 0.98, y(0.12)],
    [hx - hr * 0.86, y(-0.58)],
    [hx - hr * 0.2, y(-1.0)],
    [hx + hr * 0.45, y(-0.86 - brow)],
    // Stop (between brow and bridge), then the bridge to the nose.
    [hx + hr * 0.72 + snL * 0.1, tipTop - snD * 0.1 - hr * 0.08],
    [tipX - snL * 0.45, tipTop - snD * 0.06],
    [tipX - snD * 0.18, tipTop],
    [tipX + snD * (0.08 - 0.06 * c), tipTop + snD * 0.32],
    [tipX - snD * 0.06, tipTop + snD * 0.7],
    [tipX - snL * 0.35 - snD * 0.1, tipTop + snD * (0.86 + 0.08 * c)],
    [hx + hr * 0.45, y(0.72 * jaw + 0.1 * c)],
    [hx - hr * 0.05, y(0.86 * jaw)],
  ];
  const mouthFront: Point = [tipX - snD * 0.12, tipTop + snD * 0.66];
  const corner: Point = [hx + hr * (0.32 - 0.25 * c), y(0.38 + 0.12 * c)];
  const mouthMid: Point = [lerp(mouthFront[0], corner[0], 0.5), lerp(mouthFront[1], corner[1], 0.5) + hr * 0.05];
  return {
    params: p,
    points,
    snL,
    snD,
    anchors: {
      snoutTip: [tipX + snD * (0.08 - 0.06 * c), tipTop + snD * 0.32],
      nose: [tipX - snD * 0.02, tipTop + snD * 0.2],
      mouth: [mouthFront, mouthMid, corner],
      fangRoot: [tipX - snD * 0.25 - snL * 0.18, tipTop + snD * 0.7],
      tuskRoot: [lerp(mouthFront[0], corner[0], 0.3), lerp(mouthFront[1], corner[1], 0.3) + hr * 0.04],
      hornBase: [hx + hr * 0.2, y(-0.86)],
      cheek: [hx - hr * 0.45, y(0.2)],
      throat: [hx - hr * 0.3, y(0.8)],
      nape: [hx - hr * 0.8, y(-0.45)],
      muzzle: { x: tipX - snL * 0.3 - snD * 0.1, y: tipTop + snD * 0.55, rx: snL * 0.55 + snD * 0.35, ry: snD * 0.45 },
    },
  };
}

/** The neck: a soft wedge from the body's neck roots to the back of the head. */
export function neck(top: Point, low: Point, h: HeadPart): Point[] {
  const { nape, throat } = h.anchors;
  const midTop: Point = [lerp(top[0], nape[0], 0.55), lerp(top[1], nape[1], 0.55) - h.params.hr * 0.08];
  const midLow: Point = [lerp(low[0], throat[0], 0.5) + h.params.hr * 0.1, lerp(low[1], throat[1], 0.5) + h.params.hr * 0.1];
  return [top, midTop, nape, [h.params.hx, h.params.hy], throat, midLow, low, [lerp(top[0], low[0], 0.5) - h.params.hr * 0.6, lerp(top[1], low[1], 0.5)]];
}

// ------------------------------------------------------------------ eyes

export interface EyeParams {
  /** 0..1 eye size. */
  size: number;
  cold: boolean;
  /** Portrait (with lids and double glint) or sprite (a dot and one glint). */
  lod: 'portrait' | 'sprite';
}

/**
 * Where an eye at azimuth `az` (degrees; 0 straight ahead, 180 straight
 * back) sits on the side view of the head, and how much it foreshortens.
 * Contract v1: forward eyes sit near the snout, side eyes higher and
 * further back; the midline eye (az 0) sits on the brow.
 */
export function eyeSpot(h: HeadPart, az: number, row: number, eyeR: number, far = false): { x: number; y: number; r: number; fore: number } {
  const { hx, hy, hr } = h.params;
  const flat = h.params.cold ? 0.78 : 1;
  const a = (Math.max(0, Math.min(180, az)) * Math.PI) / 180;
  const fore = 0.45 + 0.55 * Math.sin(a);
  const lift = hr * 0.2 * Math.max(0, Math.min(1, (az - 60) / 40));
  let x = az === 0 ? hx + hr * 0.5 : hx + hr * 0.02 + hr * 0.58 * Math.cos(a);
  let y = (az === 0 ? hy - hr * 0.68 * flat : hy - hr * 0.3 * flat - lift) - row * eyeR * 0.95;
  let r = eyeR;
  if (far) {
    // The far eye of a forward pair peeks past the bridge, smaller.
    x += eyeR * fore * 1.2 + hr * 0.05;
    y -= eyeR * 0.3;
    r = eyeR * 0.78;
  }
  return { x, y, r, fore };
}

/** One eye (shapes tagged `eye`; the glint is the white ellipse). */
export function eyeShapes(at: { x: number; y: number; r: number; fore: number }, e: EyeParams, pal: CreaturePalette, far = false): Shape[] {
  const { x, y, r, fore } = at;
  const rx = r * fore;
  const out: Shape[] = [];
  const P = 'head' as const;
  // Socket ring in ink, then the iris (cold: gold with a slit; warm: dark amber, big round pupil).
  out.push({ part: P, tag: 'eye', kind: 'ellipse', x, y, rx: rx * 1.18 + 0.7, ry: r * 1.18 + 0.4, fill: pal.ink });
  out.push({ part: P, tag: 'eye', kind: 'ellipse', x, y, rx, ry: r, fill: far ? mix(pal.iris, pal.ink, 0.3) : pal.iris });
  if (e.cold) out.push({ part: P, tag: 'eye', kind: 'ellipse', x: x + rx * 0.08, y, rx: rx * 0.24, ry: r * 0.86, fill: PUPIL });
  else out.push({ part: P, tag: 'eye', kind: 'ellipse', x: x + rx * 0.14, y: y + r * 0.04, rx: rx * 0.66, ry: r * 0.7, fill: PUPIL });
  if (e.lod === 'portrait' && !far) {
    // Upper lid: a band of body shade over the top of the eye (gives the look intent).
    out.push({ part: P, tag: 'eye', kind: 'curve', closed: false, points: [[x - rx * 1.1, y - r * 0.15], [x, y - r * 1.12], [x + rx * 1.15, y - r * 0.2]], stroke: pal.ink, width: Math.max(0.9, r * 0.22) });
  }
  out.push({ part: P, tag: 'glint', kind: 'ellipse', x: x + rx * 0.32, y: y - r * 0.38, rx: r * 0.28 * fore, ry: r * 0.28, fill: GLINT, alpha: 0.95 });
  if (e.lod === 'portrait' && r > 2.5) out.push({ part: P, tag: 'eye', kind: 'ellipse', x: x - rx * 0.28, y: y + r * 0.36, rx: r * 0.12 * fore, ry: r * 0.12, fill: GLINT, alpha: 0.6 });
  return out;
}

// ------------------------------------------------------------------ teeth, horns

/** A curved fang hanging from `root`, `len` long (bone, inked). */
export function fang(root: Point, len: number, w: number, pal: CreaturePalette): Shape {
  const [x, y] = root;
  return {
    part: 'head', tag: 'fang', kind: 'curve', closed: true,
    points: [[x - w, y - 1], [x - w * 0.55, y + len * 0.55], [x + w * 0.1, y + len], [x + w * 0.55, y + len * 0.5], [x + w * 0.7, y - 1]],
    fill: pal.bone, stroke: pal.ink, width: 1,
  };
}

/** A tusk curving up and forward out of the lower jaw. */
export function tusk(root: Point, len: number, w: number, pal: CreaturePalette): Shape {
  const [x, y] = root;
  return {
    part: 'head', tag: 'tusk', kind: 'curve', closed: true,
    points: [[x - w, y + 1], [x + len * 0.25, y - len * 0.15], [x + len * 0.6, y - len * 0.65], [x + len * 0.45, y - len * 0.6], [x + len * 0.1, y - len * 0.05], [x + w * 0.6, y + 1]],
    fill: pal.bone, stroke: pal.ink, width: 1,
  };
}

/** A horn sweeping back and up from `base`: a tapering, ringed crescent. */
export function horn(base: Point, len: number, w: number, pal: CreaturePalette): Shape[] {
  const [x, y] = base;
  const centre: Point[] = [];
  for (let i = 0; i <= 8; i++) {
    const t = i / 8;
    // Back, then curling up and a little forward at the tip.
    centre.push([x - len * (0.55 * t - 0.25 * t * t * t), y - len * (0.25 * t + 0.75 * t * t)]);
  }
  const widths = centre.map((_, i) => Math.max(1.2, w * 2 * (1 - i / 9)));
  const fill = mix(pal.bone, pal.hoof, 0.22);
  return [
    { part: 'head', tag: 'horn', kind: 'curve', closed: true, points: limbOutline(centre, widths), fill, stroke: pal.ink, width: 1.2 },
    ...[2, 4].map((i): Shape => {
      const [cx, cy] = centre[i];
      const h = widths[i] * 0.5;
      return { part: 'head', tag: 'horn', kind: 'line', points: [[cx - h * 0.8, cy - h * 0.6], [cx + h * 0.8, cy + h * 0.6]], stroke: mix(fill, pal.ink, 0.45), width: 0.9, alpha: 0.8 };
    }),
  ];
}

/** Straight spikes: a tapering cone raked back and up from `base` (Phase 3 `horn_form` straight). */
export function spikeHorn(base: Point, len: number, w: number, pal: CreaturePalette): Shape[] {
  const [x, y] = base;
  const centre: Point[] = [];
  for (let i = 0; i <= 6; i++) {
    const t = i / 6;
    centre.push([x - len * 0.42 * t, y - len * 0.9 * t]);
  }
  const widths = centre.map((_, i) => Math.max(0.9, w * 1.8 * (1 - i / 6.4)));
  const fill = mix(pal.bone, pal.hoof, 0.3);
  return [
    { part: 'head', tag: 'horn', kind: 'curve', closed: true, points: limbOutline(centre, widths), fill, stroke: pal.ink, width: 1.2 },
    ...[1, 3].map((i): Shape => {
      const [cx, cy] = centre[i];
      const h = widths[i] * 0.5;
      return { part: 'head', tag: 'horn', kind: 'line', points: [[cx - h, cy - h * 0.2], [cx + h, cy + h * 0.2]], stroke: mix(fill, pal.ink, 0.45), width: 0.9, alpha: 0.8 };
    }),
  ];
}

/** A branched rack (antlers): a beam sweeping back and up with tines forward (Phase 3 `horn_form` branched). */
export function antler(base: Point, len: number, w: number, pal: CreaturePalette, lod: 'portrait' | 'sprite' = 'portrait'): Shape[] {
  const [x, y] = base;
  const beam: Point[] = [];
  for (let i = 0; i <= 8; i++) {
    const t = i / 8;
    beam.push([x - len * (0.5 * t - 0.15 * t * t), y - len * (0.35 * t + 0.6 * t * t)]);
  }
  const fill = mix(pal.bone, pal.hoof, 0.38);
  const bw = Math.max(1.4, w * 1.3);
  const out: Shape[] = [
    { part: 'head', tag: 'horn', kind: 'curve', closed: true, points: limbOutline(beam, beam.map((_, i) => Math.max(0.9, bw * (1 - i / 11)))), fill, stroke: pal.ink, width: 1.2 },
  ];
  // Tines off the beam, forward and up; fewer at sprite size, so they read.
  const at = lod === 'sprite' ? [3, 6] : [2, 4, 6];
  for (const i of at) {
    const [bx, by] = beam[i];
    const tl = len * (0.28 + 0.06 * (i % 3));
    const tip: Point = [bx + tl * 0.55, by - tl * 0.85];
    const mid: Point = [bx + tl * 0.18, by - tl * 0.5];
    const tw = Math.max(0.8, bw * 0.75 * (1 - i / 12));
    out.push({ part: 'head', tag: 'horn', kind: 'curve', closed: true, points: limbOutline([[bx, by], mid, tip], [tw, tw * 0.75, 0.6]), fill, stroke: pal.ink, width: 1.1 });
  }
  return out;
}
