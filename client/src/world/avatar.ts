// The founder avatar: a detailed side-view portrait of a design, for the
// creation screen. Two pure steps, so both are unit-tested:
//
//   DesignSpec ──morphologyOf──▶ AvatarModel ──avatarDrawing──▶ AvatarDrawing (shapes)
//
// and a renderer (`avatarSvg`) that turns the shapes into an SVG string.
//
// `AvatarModel` is the one place slider values become body geometry and
// visible features. It extends the body plan (`Morphology` from
// `../morphology`, docs/morphology-contract.md: eye count/placement, ear
// count/shape, leg count, tail shape, snout, sizes) with the rest of the
// visible body, grouped by region (body, legs, tail, wings, cover, head,
// coloring, extras). The design editor passes the design's morphology (set
// by the body builder); without one, the traits give the default body plan.
//
// The look follows the in-world sprites (`creature.ts`): the same palette per
// coloring class, the same coarse classes (`lookOf` mirrors the server's
// `appearance.rs`), the same outline color and cues (round warm bodies with
// ears, long low cold-blooded ones with a tapering tail, carnivores' long
// snout, warning bands). Unlike the sprite it is continuous: secondary
// coverings and sub-threshold patterns show faintly, so mixed designs read.

import type { Appearance, BirthSpec, DesignSpec } from '../protocol';
import { adultMass, flight, geneValue } from '../bodyMath';
import {
  clampMorphology, FANG_MIN, hasFangs, morphologyFromTraits, pairAzimuth, TAIL_PRESENT_MIN, type Morphology,
} from '../morphology';
import { describeLook, palette, type Palette, type Point } from './creature';
import { cssColor } from './colors';

// ------------------------------------------------------------------ parameters

export type Cover = Appearance['cover'];
export type Coloring = Appearance['coloring'];
export type FootKind = 'paw' | 'hoof' | 'claw';
export type WingStage = 'none' | 'flaps' | 'wings';

/** Gene value thresholds the drawing uses (and the tests pin). */
export const THRESHOLDS = {
  /** Wings above this are gliding flaps (stats.rs `can_glide`). */
  flaps: 0.25,
  /** Wings above this are full wings (stats.rs `can_fly`). */
  wings: 0.5,
  /** Fangs below this are none (contract `fang_size`, traitText `fangs.lo`). */
  fangs: FANG_MIN,
  /** A tail from here (contract `tail_present`). */
  tail: TAIL_PRESENT_MIN,
  /** A covering or color gene shows on the body from here (appearance.rs `SHOWS`). */
  shows: 0.5,
  /** A non-dominant covering shows faintly from here. */
  secondary: 0.15,
  /** Feet: hooves or claws from here. */
  foot: 0.33,
  /** Climbing / burrowing claws from here (traitText `lo`). */
  digClimb: 0.4,
  /** Webbed feet from here (or a strong swimmer). */
  webbed: 0.5,
  /** Seasonal strategies switch on here. */
  strategy: 0.5,
} as const;

export interface CoverLayer {
  /** Gene value 0..1. */
  amount: number;
  role: 'dominant' | 'secondary' | 'none';
}

/**
 * Everything the avatar draws, derived from a design: the body plan
 * (`Morphology`, docs/morphology-contract.md; flat, snake_case) plus the rest
 * of the visible body. All amounts are 0..1.
 */
export interface AvatarModel extends Morphology {
  /** The coarse class the world sprite draws (as the server would classify it). */
  look: Appearance;
  warm: boolean;
  /** Body size gene, 0..1. */
  size: number;
  /** Adult mass in kg. */
  mass: number;
  body: { muscle: number; fat: number };
  feet: {
    kind: FootKind;
    /** Foot width gene. */
    width: number;
    webbed: boolean;
    /** Claw length 0..1 (claws, climbing and burrowing all show as claws). */
    claws: number;
    climbing: boolean;
    digging: boolean;
  };
  wings: { stage: WingStage; amount: number };
  cover: { fur: CoverLayer; scales: CoverLayer; armor: CoverLayer; dominant: Cover };
  /** Carnivory 0..1: snout shape and teeth (its length is `snout_length`). */
  carnivory: number;
  nose: number;
  /** Horn length 0..1 (large herbivores, as on the world sprite). */
  horns: number;
  coloring: { kind: Coloring; camo: number; display: number; toxins: number; palette: Palette };
  extras: { birth: BirthSpec; seasonalCoat: boolean; swim: number };
}

const clamp01 = (v: number) => Math.max(0, Math.min(1, v));

/** The server's coarse appearance class for a founder of this design (appearance.rs `Appearance::of`). */
export function lookOf(d: DesignSpec): Appearance {
  const v = (g: string) => geneValue(d, g);
  const mass = adultMass(d);
  const size = mass < 1 ? 0 : mass < 8 ? 1 : mass < 40 ? 2 : 3;
  const c = v('carnivory');
  const diet = c < 0.34 ? 'herbivore' : c < 0.67 ? 'omnivore' : 'carnivore';
  const f = flight(d);
  const swim = clamp01(v('swimming') + 0.3 * v('foot_width'));
  const locomotion = f.fly ? 'fly' : f.glide ? 'glide' : swim >= 0.6 ? 'swim' : 'walk';
  return { size, diet, warm: d.thermo === 'warm', locomotion, cover: dominantCover(v('fur'), v('scales'), v('armor_plates')), coloring: coloringOf(d) };
}

/** The covering that shows (appearance.rs): armor, then fur, then scales, from 0.5. */
export function dominantCover(fur: number, scales: number, armor: number): Cover {
  const S = THRESHOLDS.shows;
  if (armor >= S && armor >= fur && armor >= scales) return 'armor';
  if (fur >= S && fur >= scales) return 'fur';
  if (scales >= S) return 'scales';
  return 'skin';
}

function coloringOf(d: DesignSpec): Coloring {
  const S = THRESHOLDS.shows;
  if (geneValue(d, 'coloration') >= S) return geneValue(d, 'toxins') >= S ? 'warning' : 'bright';
  if (geneValue(d, 'camouflage') >= S) return 'camo';
  return 'drab';
}

function layer(amount: number, dominant: boolean): CoverLayer {
  return { amount, role: dominant ? 'dominant' : amount >= THRESHOLDS.secondary ? 'secondary' : 'none' };
}

export function wingStage(w: number): WingStage {
  return w > THRESHOLDS.wings ? 'wings' : w > THRESHOLDS.flaps ? 'flaps' : 'none';
}

export { hasFangs };

/**
 * What the avatar draws for a design. The body plan comes from the design's
 * traits (`morphologyFromTraits`: the shared genes set eye, ear, leg, tail
 * and fang sizes; the rest take the contract defaults); `features` overrides
 * any of it (the editor passes the design's whole morphology).
 */
export function morphologyOf(d: DesignSpec, features: Partial<Morphology> = {}): AvatarModel {
  const v = (g: string) => clamp01(geneValue(d, g));
  const look = lookOf(d);
  const [fur, scales, armor] = [v('fur'), v('scales'), v('armor_plates')];
  const hooves = v('hooves');
  const claws = v('claws');
  const climbing = v('climbing') >= THRESHOLDS.digClimb;
  const digging = v('burrowing') >= THRESHOLDS.digClimb;
  const clawLen = Math.max(claws, climbing ? v('climbing') * 0.8 : 0, digging ? v('burrowing') : 0);
  const kind: FootKind = hooves >= THRESHOLDS.foot && hooves >= claws ? 'hoof' : clawLen >= THRESHOLDS.foot ? 'claw' : 'paw';
  const swim = clamp01(v('swimming') + 0.3 * v('foot_width'));
  const carn = v('carnivory');
  const fromTraits = morphologyFromTraits(d.traits);
  const contract = clampMorphology({ ...fromTraits, ...features }, fromTraits);
  return {
    ...contract,
    look,
    warm: d.thermo === 'warm',
    size: v('body_size'),
    mass: adultMass(d),
    body: { muscle: v('muscle'), fat: v('fat_stores') },
    feet: { kind, width: v('foot_width'), webbed: v('foot_width') >= THRESHOLDS.webbed || swim >= 0.6, claws: clawLen, climbing, digging },
    wings: { stage: wingStage(v('wings')), amount: v('wings') },
    cover: {
      fur: layer(fur, look.cover === 'fur'),
      scales: layer(scales, look.cover === 'scales'),
      armor: layer(armor, look.cover === 'armor'),
      dominant: look.cover,
    },
    carnivory: carn,
    nose: v('keen_nose'),
    horns: carn < 0.34 && v('body_size') > 0.6 ? clamp01((v('body_size') - 0.6) / 0.4 + 0.3) : 0,
    coloring: { kind: look.coloring, camo: v('camouflage'), display: v('coloration'), toxins: v('toxins'), palette: palette(look) },
    extras: { birth: d.birth, seasonalCoat: v('seasonal_coat') >= THRESHOLDS.strategy && d.thermo === 'warm', swim },
  };
}

// ------------------------------------------------------------------ description

const level3 = (v: number, low: string, mid: string, high: string) => (v < 0.33 ? low : v > 0.66 ? high : mid);
const NUMBER = ['no', 'one', 'two', 'three', 'four', 'five', 'six', 'seven', 'eight'];

/** The avatar in words, for its aria-label: the world's `describeLook` plus the details drawn. */
export function describeAvatar(m: AvatarModel): string {
  const bits: string[] = [];
  const f = m.feet;
  const feet = f.kind === 'hoof' ? 'hooves' : f.kind === 'claw' ? (f.digging ? 'digging claws' : f.climbing ? 'climbing claws' : 'claws') : 'soft feet';
  const legs = m.leg_count === 4 ? 'legs' : `${NUMBER[m.leg_count]} legs`;
  if (m.leg_count === 0) bits.push('no legs');
  else bits.push(`${level3(m.leg_length, 'short', 'medium', 'long')} ${legs} with ${f.webbed ? `webbed ${feet === 'soft feet' ? 'feet' : feet}` : feet}`);
  if (m.body.muscle > 0.5) bits.push('muscular');
  if (m.body.fat > 0.5) bits.push('stout');
  if (!m.tail_present) bits.push('no tail');
  else bits.push(`${level3(m.tail_length, 'a short', 'a', 'a long')} ${m.tail_shape === 'thin' ? '' : `${m.tail_shape} `}tail`);
  if (m.wings.stage === 'flaps') bits.push('gliding flaps');
  if (m.wings.stage === 'wings') bits.push('wings');
  if (hasFangs(m)) bits.push(m.fang_size > 0.66 ? 'long fangs' : 'fangs');
  const eyes = m.eye_count === 2 ? 'eyes' : `${NUMBER[m.eye_count]} eyes`;
  if (m.eye_size > 0.5) bits.push(`large ${eyes}`);
  else if (m.eye_count !== 2) bits.push(eyes);
  const ears = m.ear_count === 2 ? 'ears' : `${NUMBER[m.ear_count]} ears`;
  const earShape = m.ear_shape === 'round' ? '' : `${m.ear_shape} `;
  if (m.ear_count === 0) bits.push('no ears');
  else if (m.ear_size > 0.5) bits.push(`large ${earShape}${ears}`);
  else if (m.ear_count !== 2 || earShape) bits.push(`${earShape}${ears}`);
  if (m.snout_length > 0.75) bits.push('long snout');
  else if (m.snout_length < 0.25) bits.push('short snout');
  if (m.nose > 0.5) bits.push('keen nose');
  if (m.horns > 0) bits.push('horns');
  const sec = (['fur', 'scales', 'armor'] as const).filter((c) => m.cover[c].role === 'secondary');
  if (sec.length) bits.push(`some ${sec.map((c) => (c === 'armor' ? 'armor plates' : c)).join(' and ')}`);
  if (m.coloring.kind !== 'camo' && m.coloring.camo >= 0.3) bits.push('faint camouflage');
  if (m.coloring.toxins >= 0.33 && m.coloring.kind !== 'warning') bits.push('poison glands');
  if (m.extras.birth === 'incubated_eggs') bits.push('lays and guards eggs');
  if (m.extras.birth === 'abandoned_eggs') bits.push('lays eggs');
  if (m.extras.seasonalCoat) bits.push('grows a winter coat');
  return `${describeLook(m.look)}; ${bits.join(', ')}`;
}

// ------------------------------------------------------------------ shapes

/** Which part of the body a shape belongs to: the renderer animates some parts. */
export type Part = 'shadow' | 'wing-far' | 'legs-far' | 'tail' | 'body' | 'legs-near' | 'head' | 'wing-near' | 'badge';

interface Paint {
  fill?: number;
  alpha?: number;
  stroke?: number;
  width?: number;
}

export type AvatarShape = { part: Part } & Paint & (
  | { kind: 'ellipse'; x: number; y: number; rx: number; ry: number }
  /** Straight-edged polygon. */
  | { kind: 'poly'; points: Point[] }
  /** Smooth curve through the points (closed outline, or an open stroke). */
  | { kind: 'curve'; points: Point[]; closed: boolean }
  /** Straight polyline stroke. */
  | { kind: 'line'; points: Point[] }
);

export interface AvatarDrawing {
  /** Back to front. Creature shapes are in creature units with the ground at y = 0, facing +x. */
  shapes: AvatarShape[];
  /** Creature scale relative to the largest body (0.6..1). */
  scale: number;
  /** Rotation centers of animated parts, in creature units. */
  pivots: { tail: Point; wing: Point; breath: Point };
  label: string;
}

const OUTLINE = 0x15181c;
const BONE = 0xf4efe1;
const HORN = 0xe8e0c8;
const HOOF = 0x2b2522;

function mix(a: number, b: number, t: number): number {
  const ch = (c: number, s: number) => (c >> s) & 255;
  const m = (s: number) => Math.round(ch(a, s) + (ch(b, s) - ch(a, s)) * t) << s;
  return m(16) | m(8) | m(0);
}

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
function bezier(p0: Point, c: Point, p1: Point, n: number): Point[] {
  const out: Point[] = [];
  for (let i = 0; i <= n; i++) {
    const t = i / n;
    const u = 1 - t;
    out.push([u * u * p0[0] + 2 * u * t * c[0] + t * t * p1[0], u * u * p0[1] + 2 * u * t * c[1] + t * t * p1[1]]);
  }
  return out;
}

/** A small deterministic random sequence (patterns stay put as sliders move). */
function rng(seed: number): () => number {
  let s = seed >>> 0;
  return () => {
    s = (s * 1664525 + 1013904223) >>> 0;
    return s / 4294967296;
  };
}

/** The avatar's shapes for a morphology. */
export function avatarDrawing(m: AvatarModel): AvatarDrawing {
  const out: AvatarShape[] = [];
  const add = (part: Part, s: Omit<AvatarShape, 'part'> & Record<string, unknown>) => out.push({ part, ...s } as AvatarShape);
  const p = m.coloring.palette;
  const { muscle: mu, fat: fa } = m.body;
  const arm = m.cover.armor.amount;
  const warm = m.warm;
  const ow = m.cover.dominant === 'armor' ? 2.4 : 1.6; // outline width

  // --- proportions (creature units; the largest body is about 300 wide)
  const rx = (warm ? 52 : 62) * (1 + 0.08 * mu + 0.06 * arm);
  const ry = (warm ? 30 : 21) * (1 + 0.28 * fa + 0.15 * mu + 0.12 * arm) * (1 + 0.12 * m.size);
  const pairs = Math.max(0, Math.round(m.leg_count / 2));
  const legLen = (warm ? 20 : 13) + (warm ? 48 : 30) * m.leg_length;
  const legW = (8 + 5 * mu + 4 * m.size + 2 * fa) * (pairs > 2 ? 0.8 : 1);
  const clear = legLen * (warm ? 0.92 : 0.62);
  // Legless bodies rest on the ground.
  const cy = pairs === 0 ? -ry * (1 + 0.22 * fa) - 1 : -(clear + ry * 0.75);
  const hump = ry * 0.18 * mu;
  // Body edge at angle t (0 front, π/2 belly, -π/2 back).
  const edge = (t: number): Point => {
    const s = Math.sin(t);
    let y = ry * s * (s > 0 ? 1 + 0.22 * fa * s : 1);
    if (s < 0) y -= hump * Math.exp(-((t + 1.15) ** 2) / 0.2);
    return [rx * Math.cos(t), cy + y];
  };
  const bodyPts: Point[] = [];
  for (let i = 0; i < 32; i++) bodyPts.push(edge((i / 32) * Math.PI * 2));
  const inBody = (x: number, y: number, k = 0.82) => (x / (rx * k)) ** 2 + ((y - cy) / (ry * k)) ** 2 < 1;

  // Head: smaller relative to big bodies.
  const hr = (warm ? 21 : 17) * (1.18 - 0.32 * m.size) * (1 + 0.08 * mu);
  const hx = rx * 0.92 + hr * 0.75;
  const hy = cy - ry * (warm ? 0.85 : 0.35) - hr * 0.15;

  const pal = (far: boolean, c: number) => (far ? mix(c, OUTLINE, 0.35) : c);
  const R = rng(7);

  // ------------------------------------------------------------ far wing
  const wingRoot: Point = [rx * 0.25, cy - ry * 0.7];
  const wing = (far: boolean): void => {
    if (m.wings.stage !== 'wings') return;
    const span = rx * (1.05 + 1.1 * clamp01((m.wings.amount - 0.5) / 0.5));
    const [ox, oy] = far ? [rx * 0.18, -4] : [0, 0];
    const o = (x: number, y: number): Point => [wingRoot[0] + ox + x * span, wingRoot[1] + oy + y * span];
    const wrist = o(-0.25, -0.75);
    const tips = [o(-1.05, -0.95), o(-1.18, -0.48), o(-0.98, -0.08)];
    const back: Point = [-rx * 0.45 + ox, cy - ry * 0.65 + oy];
    const scallop = (a: Point, b: Point): Point => [(a[0] + b[0]) / 2 + span * 0.06, (a[1] + b[1]) / 2 + span * 0.08];
    const part = far ? 'wing-far' : 'wing-near';
    add(part, {
      kind: 'curve', closed: true,
      points: [o(0, 0), o(-0.1, -0.45), wrist, tips[0], scallop(tips[0], tips[1]), tips[1], scallop(tips[1], tips[2]), tips[2], scallop(tips[2], back), back],
      fill: pal(far, p.light), alpha: far ? 0.9 : 0.88, stroke: OUTLINE, width: 1.4,
    });
    add(part, { kind: 'line', points: [o(0, 0), o(-0.1, -0.45), wrist], stroke: pal(far, p.dark), width: 3.2 });
    for (const t of tips) add(part, { kind: 'line', points: [wrist, t], stroke: pal(far, p.dark), width: 1.6 });
    add(part, { kind: 'poly', points: [wrist, [wrist[0] + 5, wrist[1] - 7], [wrist[0] + 7, wrist[1] - 1]], fill: BONE, stroke: OUTLINE, width: 0.8 });
  };

  // ------------------------------------------------------------ legs
  // Pairs spread from hip to shoulder; a biped stands on one pair under the hips.
  const legXs = pairs === 1 ? [-rx * 0.15] : Array.from({ length: pairs }, (_, i) => rx * (-0.55 + (1.05 * i) / (pairs - 1)));
  const isFront = (i: number) => pairs > 1 && i >= pairs / 2;
  const leg = (x: number, front: boolean, far: boolean): void => {
    const part = far ? 'legs-far' : 'legs-near';
    const dx = far ? rx * 0.12 : 0;
    const top: Point = [x + dx, cy + ry * 0.25];
    const dir = front ? 1 : -1;
    let joints: Point[];
    if (warm) {
      joints = front
        ? [top, [top[0] + 1, -legLen * 0.55], [top[0] + 2, -legW * 0.3]]
        : [top, [top[0] + legLen * 0.22, -legLen * 0.62], [top[0] - legLen * 0.12, -legLen * 0.28], [top[0] - legLen * 0.05, -legW * 0.3]];
    } else {
      // Sprawling: the knee juts out at belly height.
      joints = [top, [top[0] + dir * legLen * 0.45, cy + ry * 0.75], [top[0] + dir * legLen * 0.32, -legW * 0.3]];
    }
    const widths = joints.map((_, i) => (i === 0 ? legW * (1.25 + 0.5 * mu) : legW * (0.8 - (0.25 * i) / joints.length)));
    const fill = pal(far, p.base);
    add(part, { kind: 'curve', closed: true, points: limbOutline(joints, widths), fill, stroke: OUTLINE, width: 1.2 });
    // Armor bands / scales on the legs.
    const lower = joints.slice(1);
    if (m.cover.scales.role !== 'none') {
      const a = m.cover.scales.role === 'dominant' ? 0.75 : 0.35 + 0.6 * m.cover.scales.amount;
      for (let i = 0; i < lower.length; i++) {
        const [jx, jy] = lower[i];
        const prev = joints[i];
        for (const t of [0.3, 0.65]) {
          const sx = prev[0] + (jx - prev[0]) * t;
          const sy = prev[1] + (jy - prev[1]) * t;
          const s = legW * 0.28;
          add(part, { kind: 'curve', closed: false, points: [[sx - s, sy], [sx, sy + s * 0.8], [sx + s, sy]], stroke: pal(far, p.dark), width: 1, alpha: a });
        }
      }
    }
    // Foot.
    const [fx, fy] = joints[joints.length - 1];
    const fw = legW * (1.0 + 1.5 * m.feet.width);
    const fh = legW * 0.55;
    const fc = pal(far, p.dark);
    if (m.feet.webbed) {
      add(part, { kind: 'poly', points: [[fx - fw * 0.15, fy], [fx + fw * 0.95, -0.5], [fx - fw * 0.45, -0.5]], fill: pal(far, p.light), alpha: 0.85, stroke: OUTLINE, width: 0.8 });
    }
    if (m.feet.kind === 'hoof') {
      add(part, { kind: 'poly', points: [[fx - fw * 0.42, fy - fh * 0.2], [fx + fw * 0.42, fy - fh * 0.2], [fx + fw * 0.6, 0], [fx - fw * 0.5, 0]], fill: far ? mix(HOOF, OUTLINE, 0.3) : HOOF, stroke: OUTLINE, width: 1 });
      add(part, { kind: 'line', points: [[fx + fw * 0.05, fy], [fx + fw * 0.1, 0]], stroke: 0x55493f, width: 0.8 });
    } else {
      add(part, { kind: 'ellipse', x: fx + fw * 0.12, y: -fh * 0.5, rx: fw * 0.6, ry: fh * 0.55, fill: fc, stroke: OUTLINE, width: 1 });
      if (m.feet.kind === 'claw') {
        const cl = 2 + 9 * m.feet.claws * (front && m.feet.digging ? 1.4 : 1);
        const n = 3;
        for (let i = 0; i < n; i++) {
          const bx = fx + fw * (0.25 + 0.2 * i);
          const hook = m.feet.climbing ? cl * 0.6 : cl * 0.2;
          add(part, {
            kind: 'curve', closed: false,
            points: [[bx, -fh * 0.4], [bx + cl * 0.7, -fh * 0.3 + hook * 0.2], [bx + cl, hook * 0.3]],
            stroke: far ? mix(BONE, OUTLINE, 0.4) : BONE, width: front && m.feet.digging ? 2.6 : 1.7,
          });
        }
      } else {
        for (let i = 0; i < 3; i++) add(part, { kind: 'line', points: [[fx + fw * (0.3 + 0.16 * i), -fh * 0.95], [fx + fw * (0.3 + 0.16 * i), -fh * 0.55]], stroke: OUTLINE, width: 0.7, alpha: 0.6 });
      }
    }
    // Warning bands on the legs.
    if (m.coloring.kind === 'warning') {
      const [ax, ay] = joints[1];
      add(part, { kind: 'line', points: [[ax - legW * 0.45, ay - 3], [ax + legW * 0.45, ay + 3]], stroke: pal(far, p.accent), width: legW * 0.35 });
    }
  };

  // ------------------------------------------------------------ tail
  const tailRoot: Point = warm ? [-rx * 0.9, cy - ry * 0.25] : [-rx * 0.92, cy + ry * 0.05];
  const tailPart = (): void => {
    if (!m.tail_present) return;
    const L = m.tail_length;
    const shape = m.tail_shape;
    const tl = 6 + 105 * L * (warm ? 0.9 : 1.15);
    const tb = (warm ? 0.32 : 0.55) * ry * (1 + 0.3 * fa) * (0.6 + 0.4 * Math.min(1, L * 3));
    const ground = (y: number) => Math.min(-2, y);
    const end: Point = warm ? [tailRoot[0] - tl * 0.85, ground(tailRoot[1] - tl * 0.2 + tl * 0.35 * (1 - L))] : [tailRoot[0] - tl, ground(tailRoot[1] + tl * 0.45)];
    const ctrl: Point = warm ? [tailRoot[0] - tl * 0.45, tailRoot[1] - tl * 0.45] : [tailRoot[0] - tl * 0.55, tailRoot[1] - tl * 0.05];
    let line = bezier(tailRoot, ctrl, end, 10);
    if (shape === 'prehensile') {
      // The last third curls into a coil.
      line = bezier(tailRoot, ctrl, end, 7);
      const [ax, ay] = line[line.length - 2];
      const [bx, by] = end;
      const dir = Math.atan2(by - ay, bx - ax);
      const r0 = Math.max(4, tl * 0.13);
      // Curl downward: center to the left of travel (screen coords).
      const cxc = bx + Math.cos(dir - Math.PI / 2) * -r0;
      const cyc = by + Math.sin(dir - Math.PI / 2) * -r0;
      const a0 = Math.atan2(by - cyc, bx - cxc);
      for (let i = 1; i <= 6; i++) {
        const a = a0 - (i / 6) * Math.PI * 1.5;
        const r = r0 * (1 - 0.09 * i);
        line.push([cxc + Math.cos(a) * r, Math.min(-2, cyc + Math.sin(a) * r)]);
      }
    }
    const n = line.length;
    const widths = line.map((_, i) => 1.5 + (tb - 1.5) * (1 - i / n) ** 0.9);
    const outline = limbOutline(line, widths);
    if (shape === 'fin') {
      // A ridge along the top and a paddle at the tip.
      const fin = line.slice(3).map(([x, y], i, a): Point => [x, y - widths[i + 3] / 2 - 6 * Math.sin((Math.PI * (i + 1)) / (a.length + 1)) - 2]);
      add('tail', { kind: 'curve', closed: true, points: [...fin, ...line.slice(3).reverse()], fill: p.dark, stroke: OUTLINE, width: 1 });
      const [ex, ey] = end;
      const fs = 8 + tb * 1.1;
      add('tail', { kind: 'curve', closed: true, points: [[ex + 4, ey], [ex - fs * 0.9, ey - fs], [ex - fs * 0.55, ey], [ex - fs * 0.9, Math.min(-1, ey + fs)]], fill: p.dark, stroke: OUTLINE, width: 1.2 });
    }
    if (shape === 'bushy') {
      // A fluffy brush: overlapping tufts, darker round the edge.
      const fluff = (i: number) => (tb * 0.6 + 4 + 8 * L) * (0.55 + 0.6 * Math.sin((Math.PI * (i + 1.5)) / (n + 1)));
      for (let i = 1; i < n; i++) add('tail', { kind: 'ellipse', x: line[i][0], y: line[i][1], rx: fluff(i) * 1.1, ry: fluff(i), fill: p.dark, stroke: OUTLINE, width: 1 });
      for (let i = 1; i < n; i++) add('tail', { kind: 'ellipse', x: line[i][0] + 1, y: line[i][1] - 1, rx: fluff(i) * 0.8, ry: fluff(i) * 0.7, fill: p.base });
      for (let i = 2; i < n; i += 2) add('tail', { kind: 'line', points: [[line[i][0] + 3, line[i][1] - 2], [line[i][0] - 4, line[i][1] + 2]], stroke: p.dark, width: 0.9, alpha: 0.7 });
    } else {
      add('tail', { kind: 'curve', closed: true, points: outline, fill: p.base, stroke: OUTLINE, width: ow });
    }
    if (shape === 'club') {
      const [ex, ey] = end;
      const cr = 5 + tb * 0.9 + 5 * L;
      add('tail', { kind: 'ellipse', x: ex - cr * 0.3, y: ey, rx: cr * 1.25, ry: cr, fill: p.dark, stroke: OUTLINE, width: 1.6 });
      for (const [dx, dy] of [[-0.9, -0.7], [0.2, -0.95], [-1.3, 0.2], [-0.4, 0.9]] as Point[]) {
        add('tail', { kind: 'poly', points: [[ex - cr * 0.3 + dx * cr * 0.9 - 2, ey + dy * cr * 0.85], [ex - cr * 0.3 + dx * cr * 1.3, ey + dy * cr * 1.25], [ex - cr * 0.3 + dx * cr * 0.9 + 2, ey + dy * cr * 0.85]], fill: BONE, stroke: OUTLINE, width: 0.8 });
      }
    }
    if (shape !== 'bushy') {
      if (m.coloring.kind === 'warning') {
        for (const i of [3, 6, 8]) {
          if (i >= n - 1) continue;
          const a = line[i];
          const w = widths[i];
          add('tail', { kind: 'line', points: [[a[0] - 1.5, a[1] - w * 0.48], [a[0] + 1.5, a[1] + w * 0.48]], stroke: p.accent, width: Math.max(2, w * 0.45) });
        }
      }
      if (m.cover.scales.role !== 'none') {
        const a = m.cover.scales.role === 'dominant' ? 0.75 : 0.35 + 0.6 * m.cover.scales.amount;
        for (let i = 1; i < n - 2; i++) {
          const [x, y] = line[i];
          const s = Math.max(1.5, widths[i] * 0.22);
          add('tail', { kind: 'curve', closed: false, points: [[x - s, y], [x, y + s * 0.8], [x + s, y]], stroke: p.dark, width: 0.9, alpha: a });
        }
      }
      if (m.cover.armor.role === 'dominant') {
        for (let i = 1; i < n - 2; i += 2) {
          const [x, y] = line[i];
          add('tail', { kind: 'line', points: [[x - 1, y - widths[i] * 0.5], [x + 1, y + widths[i] * 0.5]], stroke: OUTLINE, width: 1.2 });
        }
      }
      // A furry body ends a thin tail in a tuft.
      if (shape === 'thin' && m.cover.fur.role === 'dominant' && L > 0.2) {
        const [x, y] = line[n - 1];
        const len = 6 + 10 * m.cover.fur.amount;
        const pts: Point[] = [];
        for (let i = 0; i < 10; i++) {
          const a = (i / 10) * Math.PI * 2;
          const k = i % 2 === 0 ? 1 : 0.5;
          pts.push([x + Math.cos(a) * len * k - len * 0.3, y + Math.sin(a) * len * k * 0.7]);
        }
        add('tail', { kind: 'poly', points: pts, fill: p.dark, stroke: OUTLINE, width: 0.8 });
      }
    }
  };

  // ------------------------------------------------------------ ears (behind the head)
  // Cold-blooded bodies with small ears show an ear opening instead.
  const earHole = !warm && m.ear_size < 0.5 && m.ear_count > 0;
  const ears = (): void => {
    if (earHole || m.ear_count <= 0) return;
    const ea = hr * (0.5 + 1.15 * m.ear_size);
    const shape = m.ear_shape;
    const earAt = (bx: number, by: number, far: boolean) => {
      const b1: Point = [bx - hr * 0.22, by + hr * 0.1];
      const b2: Point = [bx + hr * 0.25, by - hr * 0.08];
      let pts: Point[];
      if (shape === 'pointed') {
        pts = [b1, [bx - ea * 0.35, by - ea], b2];
      } else if (shape === 'long') {
        const h2 = ea * 1.9;
        const w = hr * 0.42;
        const lean = ea * 0.45;
        pts = [b1, [bx - hr * 0.15 - lean * 0.45, by - h2 * 0.5], [bx - lean - w * 0.35, by - h2], [bx - lean + w * 0.3, by - h2 * 0.98], [bx + hr * 0.18 - lean * 0.4, by - h2 * 0.5], b2];
      } else if (shape === 'fan') {
        const e2 = ea * 1.5;
        pts = [b2, [bx + e2 * 0.1, by - e2 * 0.6], [bx - e2 * 0.6, by - e2 * 0.75], [bx - e2 * 1.05, by - e2 * 0.1], [bx - e2 * 0.8, by + e2 * 0.6], [bx - e2 * 0.2, by + e2 * 0.45], b1];
      } else {
        const tip: Point = [bx - ea * 0.35, by - ea];
        const m6: Point = [b1[0] + (tip[0] - b1[0]) * 0.6, b1[1] + (tip[1] - b1[1]) * 0.6];
        pts = [b1, m6, [tip[0] + ea * 0.12, tip[1] + ea * 0.06], [b2[0] + ea * 0.1, b2[1] - ea * 0.5], b2];
      }
      const kind = shape === 'pointed' ? 'poly' : 'curve';
      const fill = far ? pal(true, p.base) : p.base;
      add('head', { kind, closed: true, points: pts, fill, stroke: OUTLINE, width: 1.2 } as never);
      if (!far) {
        const c: Point = [(b1[0] + b2[0]) / 2, (b1[1] + b2[1]) / 2];
        const inner = pts.map(([x, y]): Point => [c[0] + (x - c[0]) * 0.62, c[1] + (y - c[1]) * 0.62 - ea * 0.06]);
        add('head', { kind, closed: true, points: inner, fill: mix(p.light, 0xe9a6a6, 0.35) } as never);
      }
    };
    // Mirrored pairs (far ear peeking out behind the near one), front to back.
    const pairsE = Math.floor(m.ear_count / 2);
    for (let k = 0; k < pairsE; k++) {
      const bx = hx - hr * (0.45 + 0.32 * k);
      const by = hy - hr * (0.72 - 0.12 * k);
      earAt(bx + hr * 0.28, by - 2, true);
      earAt(bx, by, false);
    }
  };

  // ------------------------------------------------------------ head geometry
  // Carnivory shapes the snout (narrower, toothier) and lengthens it; `snout_length` sets its base length.
  const sn = m.carnivory;
  const snL = hr * (0.15 + 0.9 * m.snout_length + 0.45 * sn + 0.2 * m.nose);
  const snD = hr * (0.85 - 0.3 * sn);
  const tipX = hx + hr * 0.55 + snL;
  const tipTopY = hy - hr * 0.3 + hr * 0.12 * sn;
  const headPts: Point[] = [
    [hx - hr * 0.95, hy + hr * 0.1],
    [hx - hr * 0.75, hy - hr * 0.72],
    [hx - hr * 0.05, hy - hr * 1.0],
    [hx + hr * 0.6, hy - hr * 0.72],
    [tipX - snL * 0.4, tipTopY - hr * 0.08],
    [tipX, tipTopY],
    [tipX + snD * (0.3 - 0.15 * sn), tipTopY + snD * 0.45],
    [tipX - snD * 0.08, tipTopY + snD * 0.95],
    [tipX - snL * 0.55, hy + hr * 0.55 + snD * 0.1],
    [hx + hr * 0.1, hy + hr * 0.88],
    [hx - hr * 0.6, hy + hr * 0.72],
  ];
  const neckPts: Point[] = [edge(-1.0), [hx - hr * 0.6, hy - hr * 0.55], [hx + hr * 0.1, hy + hr * 0.6], edge(0.35)];
  const mouthA: Point = [tipX - snD * 0.1, tipTopY + snD * 0.7];
  const mouthB: Point = [hx + hr * 0.3, hy + hr * (0.3 + 0.1 * sn)];

  // ================================================================ draw
  add('shadow', { kind: 'ellipse', x: rx * 0.05, y: 0, rx: rx * 1.25, ry: 4 + ry * 0.08, fill: 0x000000, alpha: 0.28 });
  wing(true);
  legXs.forEach((x, i) => leg(x, isFront(i), true));
  tailPart();

  // Fur fringe behind the body (dominant: all round; secondary: a mane).
  const furLen = 1.5 + 4.5 * m.cover.fur.amount;
  if (m.cover.fur.role === 'dominant') {
    const pts: Point[] = [];
    const n = 76;
    for (let i = 0; i < n; i++) {
      const t = (i / n) * Math.PI * 2;
      const [x, y] = edge(t);
      const k = i % 2 === 0 ? furLen * (0.7 + 0.5 * R()) : furLen * 0.35;
      const nx = x / rx;
      const ny = (y - cy) / ry;
      const len = Math.hypot(nx, ny) || 1;
      pts.push([x + (nx / len) * k, y + (ny / len) * k]);
    }
    add('body', { kind: 'poly', points: pts, fill: p.dark, stroke: OUTLINE, width: 0.8 });
  } else if (m.cover.fur.role === 'secondary') {
    const pts: Point[] = [];
    for (let i = 0; i <= 12; i++) {
      const t = -1.25 + (i / 12) * 1.0;
      const [x, y] = edge(t);
      pts.push(i % 2 === 0 ? [x, y + 2] : [x - 2, y - furLen * 0.9]);
    }
    add('body', { kind: 'poly', points: pts, fill: p.dark, alpha: 0.4 + 0.5 * m.cover.fur.amount, stroke: OUTLINE, width: 0.6 });
  }
  ears();

  // Silhouette: one outline round neck, body and head, then the fills.
  for (const pts of [neckPts, bodyPts]) add('body', { kind: 'curve', closed: true, points: pts, fill: OUTLINE, stroke: OUTLINE, width: ow * 2 });
  add('head', { kind: 'curve', closed: true, points: headPts, fill: OUTLINE, stroke: OUTLINE, width: ow * 2 });
  add('body', { kind: 'curve', closed: true, points: neckPts, fill: p.base });
  add('body', { kind: 'curve', closed: true, points: bodyPts, fill: p.base });
  // Countershading: a paler belly.
  add('body', { kind: 'ellipse', x: 0, y: cy + ry * 0.48, rx: rx * 0.74, ry: ry * 0.42, fill: p.light, alpha: m.cover.dominant === 'fur' ? 0.45 : 0.75 });

  // Coloring patterns.
  const c = m.coloring;
  if (c.camo >= THRESHOLDS.secondary && c.kind !== 'warning') {
    const n = Math.round(3 + 7 * c.camo);
    const blotch = c.kind === 'camo' ? p.accent : p.dark;
    const r2 = rng(11);
    for (let i = 0; i < n; i++) {
      const x = (r2() * 2 - 1) * rx * 0.75;
      const y = cy + (r2() * 2 - 1.3) * ry * 0.6;
      if (!inBody(x, y)) continue;
      add('body', { kind: 'ellipse', x, y, rx: rx * (0.08 + 0.1 * r2()), ry: ry * (0.12 + 0.12 * r2()), fill: blotch, alpha: 0.25 + 0.6 * c.camo });
    }
  }
  if (c.kind === 'warning') {
    for (const t of [-0.5, -0.05, 0.4]) {
      const top1 = edge(-Math.PI / 2 - t * 1.2 - 0.12);
      const top2 = edge(-Math.PI / 2 - t * 1.2 + 0.12);
      const bot2 = edge(Math.PI / 2 + t * 1.2 - 0.18);
      const bot1 = edge(Math.PI / 2 + t * 1.2 + 0.08);
      add('body', { kind: 'poly', points: [top1, top2, bot2, bot1], fill: p.accent });
    }
  } else if (c.display >= 0.33) {
    // Display spots in the accent color, more and bolder with more color.
    const r3 = rng(23);
    const n = Math.round(2 + 6 * c.display);
    for (let i = 0; i < n; i++) {
      const x = (r3() * 2 - 1) * rx * 0.7;
      const y = cy - ry * 0.1 + (r3() * 2 - 1) * ry * 0.5;
      if (!inBody(x, y, 0.75)) continue;
      const s = 2.5 + 3 * r3();
      add('body', { kind: 'ellipse', x, y, rx: s, ry: s, fill: c.kind === 'bright' ? p.accent : p.light, alpha: 0.5 + 0.5 * c.display });
    }
  }

  // Coverings on the body.
  if (m.cover.scales.role === 'dominant') {
    const s = 3.2 + 1.8 * (1 - m.cover.scales.amount);
    for (let row = 0, y = cy - ry * 0.8; y < cy + ry * 0.9; y += s * 1.5, row++) {
      for (let x = -rx + (row % 2) * s; x < rx; x += s * 2.2) {
        if (!inBody(x, y, 0.9)) continue;
        add('body', { kind: 'curve', closed: false, points: [[x - s, y], [x, y + s * 0.8], [x + s, y]], stroke: p.dark, width: 0.8, alpha: 0.7 });
      }
    }
  }
  if (m.cover.fur.role === 'dominant') {
    // Hair strokes on the flank.
    const r4 = rng(5);
    for (let i = 0; i < 26; i++) {
      const x = (r4() * 2 - 1) * rx * 0.8;
      const y = cy + (r4() * 2 - 1.2) * ry * 0.6;
      if (!inBody(x, y, 0.8)) continue;
      add('body', { kind: 'line', points: [[x, y], [x - 2 - furLen * 0.3, y + 1.5 + furLen * 0.2]], stroke: p.dark, width: 0.9, alpha: 0.55 });
    }
  }
  if (m.cover.armor.role !== 'none') {
    const dom = m.cover.armor.role === 'dominant';
    const n = dom ? 7 : 3;
    const size = ry * (dom ? 0.24 + 0.2 * arm : 0.12 + 0.12 * arm);
    for (let i = 0; i < n; i++) {
      const t = -Math.PI / 2 + (dom ? -1.05 + (2.1 * i) / (n - 1) : -0.4 + (0.8 * i) / (n - 1));
      const [x, y] = edge(t);
      add('body', { kind: 'ellipse', x, y: y + size * 0.35, rx: size * 1.15, ry: size, fill: dom ? p.dark : mix(p.dark, p.base, 0.3), alpha: dom ? 1 : 0.55 + 0.4 * arm, stroke: OUTLINE, width: dom ? 1.4 : 0.9 });
      if (dom) add('body', { kind: 'ellipse', x: x - size * 0.2, y: y + size * 0.05, rx: size * 0.45, ry: size * 0.3, fill: p.light, alpha: 0.5 });
    }
    if (dom) {
      for (const fx of [-0.5, -0.15, 0.2, 0.52]) {
        const a = edge(-Math.PI / 2 - fx * 1.2);
        const b = edge(Math.PI / 2 + fx * 1.0);
        add('body', { kind: 'curve', closed: false, points: [[a[0], a[1] + size], [(a[0] + b[0]) / 2 + 3, (a[1] + b[1]) / 2], [b[0], b[1] - ry * 0.25]], stroke: OUTLINE, width: 1.4, alpha: 0.75 });
      }
    }
  }
  if (m.cover.scales.role === 'secondary') {
    // A few scales along the lower flank.
    const a = 0.3 + 0.6 * m.cover.scales.amount;
    for (let x = -rx * 0.6; x < rx * 0.6; x += 9) {
      const y = cy + ry * 0.62;
      if (!inBody(x, y, 0.92)) continue;
      add('body', { kind: 'curve', closed: false, points: [[x - 3, y], [x, y + 2.4], [x + 3, y]], stroke: p.dark, width: 0.8, alpha: a });
    }
  }

  // Bright display crest along the back.
  if (c.kind === 'bright' || (c.kind !== 'warning' && c.display >= 0.33)) {
    const pts: Point[] = [];
    const h = ry * (0.25 + 0.4 * c.display);
    for (let i = 0; i <= 8; i++) {
      const t = -Math.PI / 2 - 0.9 + (i / 8) * 1.4;
      const [x, y] = edge(t);
      pts.push(i % 2 === 1 ? [x, y - h] : [x, y + 2]);
    }
    add('body', { kind: 'poly', points: pts, fill: p.accent, alpha: c.kind === 'bright' ? 1 : 0.7, stroke: OUTLINE, width: 1 });
  }

  // Gliding flaps: a membrane between the legs.
  if (m.wings.stage === 'flaps') {
    const depth = ry * (0.25 + 1.1 * clamp01((m.wings.amount - THRESHOLDS.flaps) / (THRESHOLDS.wings - THRESHOLDS.flaps)));
    const fxF = pairs > 1 ? legXs[legXs.length - 1] : rx * 0.5;
    const fxH = pairs > 1 ? legXs[0] : -rx * 0.55;
    const yb = cy + ry * 0.55 + depth;
    const pts: Point[] = [[fxF, cy + ry * 0.05], [fxH, cy + ry * 0.05], [fxH - 3, yb]];
    for (let i = 1; i <= 4; i++) {
      const x = fxH + ((fxF - fxH) * i) / 4;
      pts.push([x - (fxF - fxH) / 8, yb - depth * 0.35], [x, yb]);
    }
    add('body', { kind: 'curve', closed: true, points: pts, fill: p.light, alpha: 0.8, stroke: OUTLINE, width: 1.2 });
  }

  legXs.forEach((x, i) => leg(x, isFront(i), false));

  // ------------------------------------------------------------ head
  add('head', { kind: 'curve', closed: true, points: headPts, fill: p.base });
  // Muzzle: paler on herbivores and omnivores.
  if (sn < 0.67) add('head', { kind: 'ellipse', x: tipX - snL * 0.25, y: tipTopY + snD * 0.6, rx: snL * 0.55 + hr * 0.1, ry: snD * 0.42, fill: p.light, alpha: 0.7 });
  if (m.cover.armor.role === 'dominant') {
    add('head', { kind: 'curve', closed: true, points: [[hx - hr * 0.5, hy - hr * 0.72], [hx + hr * 0.1, hy - hr * 0.98], [hx + hr * 0.75, hy - hr * 0.62], [hx + hr * 0.1, hy - hr * 0.55]], fill: p.dark, stroke: OUTLINE, width: 1.2 });
  }
  if (m.cover.scales.role === 'dominant') {
    for (const [x, y] of [[hx - hr * 0.5, hy + hr * 0.2], [hx - hr * 0.2, hy + hr * 0.45], [hx - hr * 0.6, hy - hr * 0.25]] as Point[]) {
      add('head', { kind: 'curve', closed: false, points: [[x - 2.5, y], [x, y + 2], [x + 2.5, y]], stroke: p.dark, width: 0.8, alpha: 0.7 });
    }
  }
  if (c.kind === 'warning') {
    add('head', { kind: 'curve', closed: true, points: [[hx - hr * 0.2, hy - hr * 0.95], [hx + hr * 0.25, hy - hr * 0.92], [hx + hr * 0.05, hy + hr * 0.8], [hx - hr * 0.4, hy + hr * 0.75]], fill: p.accent });
  }
  // Fur tufts on the cheek.
  if (m.cover.fur.role !== 'none') {
    const a = m.cover.fur.role === 'dominant' ? 1 : 0.6;
    add('head', { kind: 'poly', points: [[hx - hr * 0.3, hy + hr * 0.45], [hx - hr * 0.7, hy + hr * (0.75 + 0.02 * furLen)], [hx - hr * 0.45, hy + hr * 0.6], [hx - hr * 0.85, hy + hr * 0.55], [hx - hr * 0.6, hy + hr * 0.3]], fill: p.dark, alpha: a });
  }
  // Poison gland behind the eye.
  if (c.toxins >= 0.33) {
    add('head', { kind: 'ellipse', x: hx - hr * 0.45, y: hy - hr * 0.3, rx: hr * (0.2 + 0.18 * c.toxins), ry: hr * (0.14 + 0.1 * c.toxins), fill: c.kind === 'warning' ? p.light : mix(p.base, 0xb7d36b, 0.4), stroke: OUTLINE, width: 0.8 });
    for (const d of [-0.1, 0.1]) add('head', { kind: 'ellipse', x: hx - hr * (0.45 + d), y: hy - hr * 0.32, rx: 0.9, ry: 0.9, fill: OUTLINE, alpha: 0.6 });
  }
  // Ear opening on cold-blooded heads.
  if (earHole) {
    add('head', { kind: 'ellipse', x: hx - hr * 0.55, y: hy + hr * 0.05, rx: hr * (0.12 + 0.28 * m.ear_size), ry: hr * (0.14 + 0.3 * m.ear_size), fill: p.dark, stroke: OUTLINE, width: 0.8 });
  }
  // Mouth, teeth and fangs.
  add('head', { kind: 'curve', closed: false, points: [mouthA, [(mouthA[0] + mouthB[0]) / 2, (mouthA[1] + mouthB[1]) / 2 + hr * 0.04], mouthB], stroke: OUTLINE, width: 1.3 });
  if (sn > 0.5) {
    const n = Math.round(3 + 5 * sn);
    const ts = hr * 0.09 * (0.6 + sn);
    for (let i = 0; i < n; i++) {
      const t = (i + 0.5) / (n + 1);
      const x = mouthA[0] + (mouthB[0] - mouthA[0]) * t;
      const y = mouthA[1] + (mouthB[1] - mouthA[1]) * t;
      add('head', { kind: 'poly', points: [[x - ts, y], [x, y + ts * 1.5], [x + ts, y]], fill: BONE, stroke: OUTLINE, width: 0.5 });
    }
  }
  if (hasFangs(m)) {
    const fl = hr * (0.18 + 0.95 * m.fang_size);
    const fw = hr * (0.1 + 0.07 * m.fang_size);
    const fx = mouthA[0] - snL * 0.22;
    const fy = mouthA[1] + (mouthB[1] - mouthA[1]) * 0.15 - 1;
    add('head', { kind: 'curve', closed: true, points: [[fx - fw, fy], [fx - fw * 0.4, fy + fl * 0.6], [fx + fw * 0.15, fy + fl], [fx + fw * 0.5, fy + fl * 0.45], [fx + fw * 0.6, fy]], fill: BONE, stroke: OUTLINE, width: 0.9 });
  }
  // Nose: nostril, and a moist nose pad and whiskers when keen.
  const nose = m.nose;
  const nx = tipX + snD * (0.15 - 0.1 * sn);
  const ny = tipTopY + snD * 0.28;
  if (warm && nose >= 0.33) add('head', { kind: 'ellipse', x: nx, y: ny, rx: hr * (0.14 + 0.16 * nose), ry: hr * (0.1 + 0.12 * nose), fill: 0x2a2224, stroke: OUTLINE, width: 0.8 });
  add('head', { kind: 'ellipse', x: nx - hr * 0.04, y: ny + hr * 0.02, rx: hr * (0.05 + 0.08 * nose), ry: hr * (0.035 + 0.05 * nose), fill: OUTLINE });
  if (warm && nose >= 0.5) {
    for (const d of [-0.15, 0, 0.15]) {
      add('head', { kind: 'curve', closed: false, points: [[nx - snL * 0.3, ny + hr * 0.25], [nx - snL * 0.1 + hr * 0.5, ny + hr * (0.2 + d * 2)], [nx + hr * 0.9, ny + hr * (0.15 + d * 4)]], stroke: 0xd8d2c4, width: 0.6, alpha: 0.8 });
    }
  }
  // Eyes (contract v1): pair k at ±min(170°, eye_azimuth + 50°·k), one of
  // each pair visible from the side; an odd one at 0° on the midline at the
  // brow, seen edge-on. Azimuth slides an eye from the face (0) to the back
  // of the head (180) and foreshortens it near either end.
  const er = hr * (0.17 + 0.22 * m.eye_size);
  const eyeAt = (az: number, row: number) => {
    const a = (Math.max(0, Math.min(180, az)) * Math.PI) / 180;
    const fore = 0.4 + 0.6 * Math.sin(a);
    const ex = az === 0 ? hx + hr * 0.62 : hx - hr * 0.05 + hr * 0.62 * Math.cos(a);
    // Later pairs sit a little higher, so eyes at close azimuths don't merge.
    const ey = (az === 0 ? hy - hr * 0.62 : hy - hr * 0.28) - row * er * 0.9;
    const erx = er * fore;
    add('head', { kind: 'ellipse', x: ex, y: ey, rx: erx * 1.12 + 0.6, ry: er * 1.12, fill: OUTLINE });
    if (warm) {
      add('head', { kind: 'ellipse', x: ex, y: ey, rx: erx, ry: er, fill: 0x2b1d14 });
      add('head', { kind: 'ellipse', x: ex + erx * 0.15, y: ey, rx: erx * 0.55, ry: er * 0.55, fill: 0x0b0b0b });
    } else {
      add('head', { kind: 'ellipse', x: ex, y: ey, rx: erx, ry: er, fill: 0xd9a52b });
      add('head', { kind: 'ellipse', x: ex + erx * 0.1, y: ey, rx: erx * 0.22, ry: er * 0.85, fill: 0x0b0b0b });
    }
    add('head', { kind: 'ellipse', x: ex + erx * 0.35, y: ey - er * 0.35, rx: er * 0.25 * fore, ry: er * 0.25, fill: 0xffffff, alpha: 0.9 });
  };
  const eyePairs = Math.floor(m.eye_count / 2);
  for (let k = eyePairs - 1; k >= 0; k--) eyeAt(Math.max(1, pairAzimuth(m.eye_azimuth, k)), k);
  if (m.eye_count % 2 === 1) eyeAt(0, 0);
  if (sn >= 0.67) add('head', { kind: 'line', points: [[hx - hr * 0.1, hy - hr * 0.62], [hx + hr * 0.55, hy - hr * 0.42]], stroke: OUTLINE, width: 1.8 });
  // Horns.
  if (m.horns > 0) {
    const hl = hr * (0.6 + 0.9 * m.horns);
    const base: Point = [hx - hr * 0.1, hy - hr * 0.85];
    const horn: Point[] = [base, [base[0] - hl * 0.25, base[1] - hl * 0.55], [base[0] - hl * 0.05, base[1] - hl * 0.95], [base[0] + hl * 0.25, base[1] - hl * 1.05]];
    add('head', { kind: 'curve', closed: false, points: horn, stroke: OUTLINE, width: hr * 0.26 });
    add('head', { kind: 'curve', closed: false, points: horn, stroke: HORN, width: hr * 0.16 });
  }

  wing(false);

  return {
    shapes: out,
    scale: 0.6 + 0.4 * m.size,
    pivots: { tail: tailRoot, wing: wingRoot, breath: [0, 0] },
    label: describeAvatar(m),
  };
}

// ------------------------------------------------------------------ badges

/** Small icons beside the creature: eggs for egg-layers, a snowflake for a winter coat. Centered on (0, 0). */
export function badgeShapes(m: AvatarModel): { egg: AvatarShape[]; coat: AvatarShape[] } {
  const egg: AvatarShape[] = [];
  const coat: AvatarShape[] = [];
  const eggAt = (x: number, y: number) => {
    egg.push({ part: 'badge', kind: 'ellipse', x, y, rx: 6, ry: 8, fill: 0xf3ead2, stroke: OUTLINE, width: 1.2 });
    egg.push({ part: 'badge', kind: 'ellipse', x: x - 1.5, y: y + 1, rx: 1.1, ry: 1.1, fill: 0xa08a64 });
    egg.push({ part: 'badge', kind: 'ellipse', x: x + 2, y: y - 3, rx: 0.9, ry: 0.9, fill: 0xa08a64 });
  };
  if (m.extras.birth === 'incubated_eggs') {
    egg.push({ part: 'badge', kind: 'curve', closed: true, points: [[-14, 2], [0, 12], [14, 2], [0, 6]], fill: 0x8a6a3e, stroke: OUTLINE, width: 1 });
    eggAt(-4, -2);
    eggAt(5, -1);
  } else if (m.extras.birth === 'abandoned_eggs') {
    eggAt(-8, 2);
    eggAt(4, 3);
    eggAt(-2, -7);
  }
  if (m.extras.seasonalCoat) {
    for (let i = 0; i < 3; i++) {
      const a = (i * Math.PI) / 3;
      const [c, s] = [Math.cos(a) * 10, Math.sin(a) * 10];
      coat.push({ part: 'badge', kind: 'line', points: [[-c, -s], [c, s]], stroke: 0xbfe3ff, width: 2 });
    }
    coat.push({ part: 'badge', kind: 'ellipse', x: 0, y: 0, rx: 2.5, ry: 2.5, fill: 0xbfe3ff });
  }
  return { egg, coat };
}

// ------------------------------------------------------------------ renderer

/** The SVG viewBox the avatar is drawn in: ground line at `GROUND`. */
export const VIEW = { w: 300, h: 200, ground: 180 } as const;

const fmt = (v: number) => (Math.round(v * 10) / 10).toString();

function pathOf(points: Point[], closed: boolean): string {
  const n = points.length;
  if (n < 3) return `M${points.map(([x, y]) => `${fmt(x)} ${fmt(y)}`).join('L')}`;
  const at = (i: number): Point => (closed ? points[(i + n) % n] : points[Math.max(0, Math.min(n - 1, i))]);
  let d = `M${fmt(points[0][0])} ${fmt(points[0][1])}`;
  const last = closed ? n : n - 1;
  for (let i = 0; i < last; i++) {
    const p0 = at(i - 1), p1 = at(i), p2 = at(i + 1), p3 = at(i + 2);
    const c1: Point = [p1[0] + (p2[0] - p0[0]) / 6, p1[1] + (p2[1] - p0[1]) / 6];
    const c2: Point = [p2[0] - (p3[0] - p1[0]) / 6, p2[1] - (p3[1] - p1[1]) / 6];
    d += `C${fmt(c1[0])} ${fmt(c1[1])} ${fmt(c2[0])} ${fmt(c2[1])} ${fmt(p2[0])} ${fmt(p2[1])}`;
  }
  return closed ? `${d}Z` : d;
}

function paint(s: Paint, filled: boolean): string {
  const a: string[] = [];
  a.push(filled && s.fill !== undefined ? `fill="${cssColor(s.fill)}"` : 'fill="none"');
  if (s.stroke !== undefined && (s.width ?? 1) > 0) a.push(`stroke="${cssColor(s.stroke)}" stroke-width="${fmt(s.width ?? 1)}"`);
  if (s.alpha !== undefined && s.alpha < 1) a.push(`opacity="${Math.max(0, Math.round(s.alpha * 100) / 100)}"`);
  return a.join(' ');
}

function shapeSvg(s: AvatarShape): string {
  switch (s.kind) {
    case 'ellipse':
      return `<ellipse cx="${fmt(s.x)}" cy="${fmt(s.y)}" rx="${fmt(Math.max(0, s.rx))}" ry="${fmt(Math.max(0, s.ry))}" ${paint(s, true)}/>`;
    case 'poly':
      return `<polygon points="${s.points.map(([x, y]) => `${fmt(x)},${fmt(y)}`).join(' ')}" ${paint(s, true)}/>`;
    case 'curve':
      return `<path d="${pathOf(s.points, s.closed)}" ${paint(s, s.closed)}/>`;
    case 'line':
      return `<polyline points="${s.points.map(([x, y]) => `${fmt(x)},${fmt(y)}`).join(' ')}" ${paint(s, false)}/>`;
  }
}

/** Bounds of shapes in their own units. */
export function shapeBounds(shapes: AvatarShape[]): { minX: number; maxX: number; minY: number; maxY: number } {
  let minX = Infinity, maxX = -Infinity, minY = Infinity, maxY = -Infinity;
  const add = (x: number, y: number, pad = 0) => {
    minX = Math.min(minX, x - pad); maxX = Math.max(maxX, x + pad); minY = Math.min(minY, y - pad); maxY = Math.max(maxY, y + pad);
  };
  for (const s of shapes) {
    const pad = (s.width ?? 0) / 2;
    if (s.kind === 'ellipse') {
      add(s.x - s.rx, s.y - s.ry, pad);
      add(s.x + s.rx, s.y + s.ry, pad);
    } else {
      for (const [x, y] of s.points) add(x, y, pad);
    }
  }
  return { minX, maxX, minY, maxY };
}

export interface SvgOptions {
  /** Idle breathing, tail sway and wing flex (SMIL; leave off for prefers-reduced-motion). */
  animate: boolean;
}

/**
 * The avatar as an SVG document string. The creature is scaled by body size
 * (and shrunk further only if it would not fit), standing on a ground line.
 */
export function avatarSvg(m: AvatarModel, opts: SvgOptions): string {
  const d = avatarDrawing(m);
  const pad = 8;
  const b = shapeBounds(d.shapes);
  const fit = Math.min((VIEW.w - 2 * pad) / (b.maxX - b.minX), (VIEW.ground - pad) / -b.minY, (VIEW.h - VIEW.ground) / Math.max(1, b.maxY));
  const k = Math.min(d.scale * 1.35, fit);
  const tx = VIEW.w / 2 - (k * (b.minX + b.maxX)) / 2;

  const groups: string[] = [];
  let cur: Part | null = null;
  let buf: string[] = [];
  const anim = (part: Part): string => {
    if (!opts.animate) return '';
    const rot = (pv: Point, deg: number, dur: string) => {
      const [x, y] = pv.map(fmt);
      return `<animateTransform attributeName="transform" type="rotate" values="0 ${x} ${y};${deg} ${x} ${y};0 ${x} ${y};${-deg * 0.6} ${x} ${y};0 ${x} ${y}" dur="${dur}" repeatCount="indefinite" calcMode="spline" keySplines=".45 0 .55 1;.45 0 .55 1;.45 0 .55 1;.45 0 .55 1"/>`;
    };
    if (part === 'tail') return rot(d.pivots.tail, m.tail_length > 0.1 ? 6 : 2, '3.6s');
    if (part === 'wing-near' || part === 'wing-far') return rot(d.pivots.wing, -4, '2.8s');
    return '';
  };
  const flush = () => {
    if (cur && buf.length) groups.push(`<g data-part="${cur}">${anim(cur)}${buf.join('')}</g>`);
    buf = [];
  };
  let shadow = '';
  for (const s of d.shapes) {
    if (s.part === 'shadow') {
      shadow += shapeSvg(s);
      continue;
    }
    if (s.part !== cur) {
      flush();
      cur = s.part;
    }
    buf.push(shapeSvg(s));
  }
  flush();
  const breathe = opts.animate
    ? '<animateTransform attributeName="transform" type="scale" values="1 1;1.004 1.02;1 1" dur="3.2s" repeatCount="indefinite" calcMode="spline" keySplines=".45 0 .55 1;.45 0 .55 1"/>'
    : '';
  const badges = badgeShapes(m);
  const badge = (shapes: AvatarShape[], x: number, y: number, name: string) =>
    shapes.length ? `<g data-badge="${name}" transform="translate(${x} ${y})">${shapes.map(shapeSvg).join('')}</g>` : '';
  return (
    `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 ${VIEW.w} ${VIEW.h}" preserveAspectRatio="xMidYMax meet" aria-hidden="true" focusable="false">` +
    `<line x1="10" y1="${VIEW.ground}" x2="${VIEW.w - 10}" y2="${VIEW.ground}" stroke="#30363d" stroke-width="1.5" stroke-linecap="round"/>` +
    `<g transform="translate(${fmt(tx)} ${VIEW.ground}) scale(${Math.round(k * 1000) / 1000})">` +
    shadow +
    `<g data-part="creature">${breathe}${groups.join('')}</g></g>` +
    badge(badges.egg, 24, 24, 'egg') +
    badge(badges.coat, VIEW.w - 22, 22, 'coat') +
    `</svg>`
  );
}
