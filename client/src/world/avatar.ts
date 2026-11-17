// The founder avatar: a side-view portrait of a design, for the creation
// screen and the body builder. Two pure steps, so both are unit-tested:
//
//   DesignSpec ──morphologyOf──▶ AvatarModel ──avatarDrawing──▶ AvatarDrawing
//
// and a renderer (`avatarSvg`) that turns the drawing into an SVG string.
//
// `AvatarModel` is the one place slider values become body geometry and
// visible features. It extends the body plan (`Morphology` from
// `../morphology`, docs/morphology-contract.md: eye count/placement, ear
// count/shape, leg count, tail shape, snout, sizes) with the rest of the
// visible body, grouped by region (body, legs, tail, wings, cover, head,
// coloring, extras). The design editor passes the design's morphology (set
// by the body builder); without one, the traits give the default body plan.
//
// The drawing is assembled from the creature art library (`../art`, see
// art/STYLE.md): every part (torso, head, ears, eyes, legs and feet, tail,
// wings, coverings, markings) is a parametric vector part with anchor
// points, painted from one palette and rendered in one house style (merged
// silhouettes with a single even outline, one highlight and one shadow tone,
// light from the top left), so any combination reads as one creature. The
// hue follows the in-world sprite's palette per colouring class
// (`creature.ts`), and the coarse classes (`lookOf`) mirror the server's
// `appearance.rs`. Unlike the sprite it is continuous: secondary coverings
// and sub-threshold patterns show faintly, so mixed designs read.

import type { Appearance, BirthSpec, DesignSpec } from '../protocol';
import { adultMass, flight, geneValue } from '../bodyMath';
import {
  clampMorphology, FANG_MIN, hasFangs, hasHorns, morphologyFromTraits, pairAzimuth, TAIL_PRESENT_MIN, type Morphology,
} from '../morphology';
import { describeLook, palette, type Palette } from './creature';
import { clamp01, ellipsePts, rng, scallopRing, type Part, type Piece, type Point, type Shape } from '../art/geom';
import { creaturePalette, INK, mix, type CreaturePalette } from '../art/palette';
import { drawingBounds, drawingSvg, fmt, shapeSvg, type Drawing, type Group } from '../art/render';
import { body } from '../art/parts/body';
import { antler, eyeShapes, eyeSpot, fang, head, horn, neck, spikeHorn, tusk } from '../art/parts/head';
import { ear, earLength } from '../art/parts/ears';
import { foot, leg, type FootStyle } from '../art/parts/legs';
import { tail } from '../art/parts/tail';
import { flaps, wing } from '../art/parts/wings';
import {
  armorPlates, camoBlotches, coatPattern, countershade, crest, featherField, featherRim, furMarks, furRim, saddle, scaleField, skinCreases, spots, warningBands, type Lod,
} from '../art/parts/cover';

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
  /** Horn length 0..1 drawn: the body plan's `horn_size` when horns show (0 below `HORNS_MIN`). */
  horns: number;
  coloring: { kind: Coloring; camo: number; display: number; toxins: number; palette: Palette };
  extras: { birth: BirthSpec; seasonalCoat: boolean; swim: number };
}


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
    // Horns are a body-plan locus (Phase 3), no longer a cosmetic rule for
    // large herbivores.
    horns: hasHorns(contract) ? contract.horn_size : 0,
    coloring: { kind: look.coloring, camo: v('camouflage'), display: v('coloration'), toxins: v('toxins'), palette: palette(look) },
    extras: { birth: d.birth, seasonalCoat: v('seasonal_coat') >= THRESHOLDS.strategy && d.thermo === 'warm', swim },
  };
}

// ------------------------------------------------------------------ description

const level3 = (v: number, low: string, mid: string, high: string) => (v < 0.33 ? low : v > 0.66 ? high : mid);
const NUMBER = ['no', 'one', 'two', 'three', 'four', 'five', 'six', 'seven', 'eight'];

/** The avatar in words, for its aria-label: the caption (`captionOf`) plus the details drawn. */
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
  if (m.horns > 0) bits.push(m.horn_form === 'branched' ? 'antlers' : m.horn_form === 'straight' ? 'straight horns' : 'curved horns');
  if (m.pattern !== 'plain') bits.push(m.pattern === 'spots' ? 'a spotted coat' : m.pattern === 'stripes' ? 'a striped coat' : 'a mottled coat');
  if (m.wings.stage === 'wings' && m.wing_aspect < 0.35) bits.push('broad wings');
  if (m.wings.stage === 'wings' && m.wing_aspect > 0.65) bits.push('long narrow wings');
  const sec = (['fur', 'scales', 'armor'] as const).filter((c) => m.cover[c].role === 'secondary');
  if (sec.length) bits.push(`some ${sec.map((c) => (c === 'armor' ? 'armor plates' : c)).join(' and ')}`);
  if (m.coloring.kind !== 'camo' && m.coloring.camo >= 0.3) bits.push('faint camouflage');
  if (m.coloring.toxins >= 0.33 && m.coloring.kind !== 'warning') bits.push('poison glands');
  if (m.extras.birth === 'incubated_eggs') bits.push('lays and guards eggs');
  if (m.extras.birth === 'abandoned_eggs') bits.push('lays eggs');
  if (m.extras.seasonalCoat) bits.push('grows a winter coat');
  return `${captionOf(m)}; ${bits.join(', ')}`;
}

// ------------------------------------------------------------------ covering

/**
 * The covering the avatar draws, and the word for it in the caption: one
 * derivation for both (a caption that says "fur" must come with a pelt).
 * The server's class (`look.cover`), except that a furred body with wings
 * or gliding flaps grows plumage: feathers.
 */
export type CoverStyle = Cover | 'feathers';

export function coveringOf(m: Pick<AvatarModel, 'cover' | 'wings'>): CoverStyle {
  return m.cover.dominant === 'fur' && m.wings.amount > THRESHOLDS.flaps ? 'feathers' : m.cover.dominant;
}

/** The preview caption: the world's `describeLook`, naming the covering the avatar draws. */
export function captionOf(m: AvatarModel): string {
  return describeLook(m.look).replace(` · ${m.look.cover} · `, ` · ${coveringOf(m)} · `);
}

// ------------------------------------------------------------------ drawing

export type { Part, Point, Shape as AvatarShape };

/** Where the builder's drag handles ride (creature units). */
export interface AvatarAnchors {
  snoutTip: Point;
  /** Fang tip (the root when there are no fangs). */
  fangTip: Point;
  /** Tip of the first near ear; null without ears. */
  earTip: Point | null;
  /** Front-most near foot on the ground; null without legs. */
  foot: Point | null;
  /** End of the tail's centreline; null without a tail. */
  tailTip: Point | null;
  /** Near the horn's tip, linear in `horn_size` (drawn or not: the builder's horn handle rides it). */
  hornTip: Point;
  /** The torso: centre and half sizes. */
  body: { cx: number; cy: number; rx: number; ry: number };
}

export interface AvatarDrawing extends Drawing {
  /** Creature scale relative to the largest body (0.6..1). */
  scale: number;
  /** Rotation centres of animated parts, in creature units. */
  pivots: { tail: Point; wing: Point };
  anchors: AvatarAnchors;
  label: string;
}

export interface DrawOptions {
  /** Portrait (the preview card) or sprite (32–64 px: no textures, bolder shapes). See art/STYLE.md. */
  lod?: Lod;
}

/** The creature's paint box (art/palette.ts) for a model: the world palette's hue, saturated by the coloration gene. */
export function paintOf(m: AvatarModel): CreaturePalette {
  return creaturePalette({ world: m.coloring.palette, coloring: m.coloring.kind, vividness: m.coloring.display, warm: m.warm });
}

const piece = (tag: string, points: Point[], smooth = true): Piece => ({ tag, points, smooth });

/** The avatar's drawing for a model: groups of silhouette pieces, details and overlays (art/render.ts). */
export function avatarDrawing(m: AvatarModel, opts: DrawOptions = {}): AvatarDrawing {
  const lod: Lod = opts.lod ?? 'portrait';
  const pal = paintOf(m);
  const { muscle: mu, fat: fa } = m.body;
  const arm = m.cover.armor.amount;
  const warm = m.warm;
  const style = coveringOf(m);
  const furry = m.cover.fur.role === 'dominant' ? m.cover.fur.amount : 0;
  const c = m.coloring;

  // --- proportions (creature units; the largest body is about 300 wide)
  const pairs = Math.max(0, Math.round(m.leg_count / 2));
  const legless = pairs === 0;
  const rx = (warm ? 50 : 58) * (1 + 0.06 * mu + 0.05 * arm) * (legless ? 1.18 : 1);
  const ry = (warm ? 28 : 19) * (1 + 0.32 * fa + 0.12 * mu + 0.1 * arm) * (1 + 0.1 * m.size) * (legless ? 0.9 : 1);
  const legLen = (warm ? 18 : 12) + (warm ? 52 : 30) * m.leg_length;
  const legW = (7.5 + 4 * mu + 3 * m.size + 2 * fa) * (pairs > 2 ? 0.8 : 1);
  const clear = legLen * (warm ? 0.88 : 0.6);
  const cy = legless ? -ry * (1 + 0.28 * fa) - 1 : -(clear + ry * 0.78);
  const B = body({ rx, ry, cy, chest: warm ? 0.6 : 0.2, sag: fa, hump: mu, boxy: Math.max(mu * 0.7, arm * 0.8) });

  const hr = (warm ? 21 : 16) * (1.22 - 0.34 * m.size) * (1 + 0.06 * mu);
  const hx = rx * 0.98 + hr * 0.5;
  const hy = cy - ry * (warm ? (legless ? 0.45 : 1.0) : 0.3) - hr * 0.15;
  const H = head({ hx, hy, hr, snout: m.snout_length, carn: m.carnivory, cold: !warm });

  // --- groups, back to front
  const groups: Group[] = [];
  const main: Group = { part: 'body', tones: pal.body, pieces: [], detail: [], over: [] };

  // Wings (far one first).
  const wingRoot: Point = [rx * 0.2, cy - ry * 0.72];
  const wingSpan = rx * (0.95 + 1.0 * clamp01((m.wings.amount - THRESHOLDS.wings) / (1 - THRESHOLDS.wings)));
  const wingGroup = (far: boolean): Group | null => {
    if (m.wings.stage !== 'wings') return null;
    const feathered = style === 'feathers';
    // Feathered wings fold along the flank (from the shoulder); membrane wings are raised over the back.
    const root: Point = feathered
      ? (far ? [rx * 0.5, cy - ry * 0.62] : [rx * 0.42, cy - ry * 0.32])
      : (far ? [wingRoot[0] + rx * 0.16, wingRoot[1] - 5] : wingRoot);
    const span = feathered ? rx * (1.35 + 0.45 * clamp01((m.wings.amount - THRESHOLDS.wings) / (1 - THRESHOLDS.wings))) : wingSpan;
    const w = wing({ root, span, feathered, far, lod, aspect: m.wing_aspect }, pal);
    return {
      part: far ? 'wing-far' : 'wing-near', tones: far ? pal.far : pal.body, pieces: w.pieces.map((p) => piece('wing', p)), detail: w.detail, over: [],
      sway: { pivot: root, deg: feathered ? -2 : -4, dur: 2.8 },
    };
  };
  const wf = wingGroup(true);
  if (wf) groups.push(wf);

  // Legs: pairs spread from hip to shoulder; a biped stands on one pair under the middle.
  const legXs = pairs === 1 ? [-rx * 0.08] : Array.from({ length: pairs }, (_, i) => rx * (-0.58 + (1.16 * i) / (pairs - 1)));
  const isFront = (i: number) => pairs > 1 && i >= pairs / 2;
  const footStyle: FootStyle = m.feet.webbed ? 'webbed' : m.feet.kind === 'hoof' ? 'hoof' : m.feet.kind === 'claw' ? 'claw' : 'plain';
  const belly = cy + ry * 0.75;
  const buildLeg = (x: number, front: boolean, far: boolean, g: Group, i: number) => {
    const top: Point = [x + (far ? rx * 0.1 : 0), cy + ry * 0.3 - (far ? 2 : 0)];
    const L = leg({ top, len: -top[1], w: legW, front, sprawl: !warm, muscle: mu, belly });
    g.pieces.push(piece(`leg-${i}`, L.limb), piece(`mass-${i}`, L.mass));
    const ft = foot(footStyle, L.ankle, legW, m.feet.width, pal, { far, claw: m.feet.claws, dig: m.feet.digging && front, climb: m.feet.climbing, lod });
    for (const p of ft.pieces) g.pieces.push(piece(`foot-${i}`, p, footStyle !== 'hoof'));
    g.detail.push(...ft.detail.map((s) => ({ ...s, part: g.part })));
    g.over.push(...ft.over.map((s) => ({ ...s, part: g.part })));
    if (!far) {
      // The thigh / shoulder contour across the flank (inside the body only).
      if (lod === 'portrait' && !legless) g.detail.push({ part: 'legs-near', tag: 'contour', kind: 'curve', closed: false, points: L.contour, stroke: pal.body.shade, width: 1.5, alpha: 0.9 });
    }
    if (c.kind === 'warning') {
      const [ax, ay] = L.joints[Math.min(2, L.joints.length - 1)];
      if (ay > cy + ry) g.detail.push({ part: g.part, tag: 'band', kind: 'line', points: [[ax - legW, ay - 2], [ax + legW, ay + 2]], stroke: pal.accent, width: legW * 0.5, cap: 'butt' });
    }
    return L;
  };
  const farLegs: Group = { part: 'legs-far', tones: pal.far, pieces: [], detail: [], over: [] };
  legXs.forEach((x, i) => buildLeg(x, isFront(i), true, farLegs, i));
  if (farLegs.pieces.length) groups.push(farLegs);

  // Tail (its own group behind the body: the rump hides the join, and it sways).
  const tailRoot: Point = warm ? B.anchors.tailRoot : [-rx * 0.82, cy + ry * 0.05];
  let tailTipPt: Point | null = null;
  if (m.tail_present) {
    const L = m.tail_length;
    const base = (warm ? 0.36 : 0.62) * ry * (1 + 0.3 * fa) * (m.tail_shape === 'thin' && warm ? 0.75 : 1) * (0.6 + 0.4 * Math.min(1, L * 3));
    const T = tail({ root: tailRoot, len: L, shape: m.tail_shape, base, warm, furry, feathered: style === 'feathers', lod }, pal);
    tailTipPt = T.tip;
    const tg: Group = { part: 'tail', tones: pal.body, pieces: T.pieces.map((p) => piece('tail', p)), detail: [...T.detail], over: [...T.over], sway: { pivot: tailRoot, deg: L > 0.1 ? 5 : 2, dur: 3.6 } };
    const n = T.centre.length;
    const ring = (i: number, color: number, w: number, alpha = 1) => {
      const [x, y] = T.centre[i];
      const [px, py] = T.centre[Math.max(0, i - 1)];
      const a = Math.atan2(y - py, x - px) + Math.PI / 2;
      const h = T.widths[i] * 0.7;
      tg.detail.push({ part: 'tail', tag: 'ring', kind: 'line', points: [[x - Math.cos(a) * h, y - Math.sin(a) * h], [x + Math.cos(a) * h, y + Math.sin(a) * h]], stroke: color, width: w, alpha, cap: 'butt' });
    };
    if (m.tail_shape !== 'bushy') {
      if (c.kind === 'warning') for (const i of [3, 6, 9]) if (i < n - 1) ring(i, pal.accent, Math.max(3, T.widths[i] * 0.55));
      if (style === 'scales' || m.cover.scales.role !== 'none') {
        const a = style === 'scales' ? 0.6 : 0.25 + 0.5 * m.cover.scales.amount;
        if (lod === 'portrait') for (let i = 1; i < n - 1; i++) {
          const [x, y] = T.centre[i];
          const s = Math.max(1.6, T.widths[i] * 0.22);
          for (const dy of [-0.25, 0.1]) tg.detail.push({ part: 'tail', tag: 'scale', kind: 'curve', closed: false, points: [[x - s, y + dy * T.widths[i]], [x, y + dy * T.widths[i] + s * 0.8], [x + s, y + dy * T.widths[i]]], stroke: pal.body.shade, width: 0.8, alpha: a });
        }
      }
      if (style === 'armor') for (let i = 1; i < n - 1; i += 2) ring(i, pal.ink, 1.2, 0.7);
    }
    groups.push(tg);
  }

  // Ears: the far ear of each pair behind the head, near ears in the main silhouette.
  const earHole = !warm && m.ear_size < 0.5 && m.ear_count > 0;
  let earTip: Point | null = null;
  const flatK = warm ? 1 : 0.78;
  if (!earHole && m.ear_count > 0) {
    const len = earLength(m.ear_shape, m.ear_size, hr);
    const farEars: Group = { part: 'ears-far', tones: pal.far, pieces: [], detail: [], over: [] };
    for (let k = 0; k < Math.floor(m.ear_count / 2); k++) {
      const base: Point = [hx - hr * (0.38 + 0.36 * k), hy - hr * (0.72 - 0.14 * k) * flatK];
      const far = ear(m.ear_shape, [base[0] + hr * 0.3, base[1] - hr * 0.06], len * 0.95, hr, -0.12);
      farEars.pieces.push(piece(`ear-far-${k}`, far.points));
      farEars.detail.push({ part: 'ears-far', kind: 'curve', closed: true, points: far.inner, fill: mix(pal.inner, pal.far.shade, 0.4) });
      const near = ear(m.ear_shape, base, len, hr, 0.04 * k);
      main.pieces.push(piece(`ear-${k}`, near.points));
      main.detail.push({ part: 'head', tag: 'inner-ear', kind: 'curve', closed: true, points: near.inner, fill: pal.inner });
      if (k === 0) earTip = near.tip;
    }
    groups.push(farEars);
  }

  // --- main silhouette: body (rimmed by its covering), neck, head, near legs, extras
  const bodyRim = style === 'fur' ? furRim(B, furry) : style === 'feathers' ? featherRim(B, m.cover.fur.amount) : B.points;
  main.pieces.unshift(piece('body', bodyRim));
  main.pieces.push(piece('neck', neck(B.anchors.neckTop, B.anchors.neckLow, H)), piece('head', H.points));
  if (style === 'fur' || style === 'feathers') {
    // Chest ruff and cheek tuft (soft locks on the silhouette).
    const [nx, ny] = B.anchors.neckLow;
    const ruff = scallopRing(ellipsePts(nx - rx * 0.04, ny - ry * 0.12, ry * (0.32 + 0.08 * furry), ry * (0.45 + 0.1 * furry), 14, 0.3), 7, () => 2 + 3 * furry, 0.5, rng(29), true);
    main.pieces.push(piece('ruff', ruff));
    const [cx0, cy0] = H.anchors.cheek;
    main.pieces.push(piece('cheek', [[cx0 + hr * 0.25, cy0 - hr * 0.2], [cx0 - hr * 0.35, cy0 - hr * 0.05], [cx0 - hr * 0.62, cy0 + hr * 0.32], [cx0 - hr * 0.3, cy0 + hr * 0.4], [cx0 - hr * 0.5, cy0 + hr * 0.66], [cx0, cy0 + hr * 0.56], [cx0 + hr * 0.4, cy0 + hr * 0.4]]));
  }
  if (m.cover.fur.role === 'secondary') {
    // A soft mane along the neck.
    const a = B.anchors.neckTop;
    const b = H.anchors.nape;
    const mane = 2.5 + 5 * m.cover.fur.amount;
    const base: Point[] = [];
    for (let i = 0; i <= 6; i++) base.push([a[0] + (b[0] - a[0]) * (i / 6), a[1] + (b[1] - a[1]) * (i / 6)]);
    const strip: Point[] = [...base, ...base.slice().reverse().map(([x, y]): Point => [x - 1, y + mane * 1.2])];
    const pts = scallopRing(strip, 8, () => mane * 0.6, 0.6, rng(31), true);
    main.pieces.push(piece('mane', pts));
    main.detail.push({ part: 'body', tag: 'mane', kind: 'curve', closed: true, points: pts, fill: pal.body.shade, alpha: 0.35 + 0.4 * m.cover.fur.amount });
  }
  if (style === 'feathers') {
    // A three-feather crest on the crown.
    const [bx, by] = [hx - hr * 0.1, hy - hr * 0.92];
    main.pieces.push(piece('crest', [[bx + 3, by + 3], [bx - hr * 0.2, by - hr * 0.7], [bx - hr * 0.05, by - hr * 0.2], [bx - hr * 0.55, by - hr * 0.55], [bx - hr * 0.2, by - hr * 0.05], [bx - hr * 0.75, by - hr * 0.2], [bx - 4, by + 4]]));
  }
  // Display crest along the back (bright colouring, or a strong display gene).
  const crestOn = style !== 'feathers' && c.kind !== 'warning' && (c.kind === 'bright' || c.display >= 0.6);
  if (crestOn) {
    const pts = crest(B, ry * (0.14 + 0.26 * c.display));
    main.pieces.push(piece('frill', pts));
    main.detail.push({ part: 'body', tag: 'frill', kind: 'curve', closed: true, points: pts, fill: pal.accent, alpha: c.kind === 'bright' ? 1 : 0.75 });
  }
  // Gliding flaps: a membrane between the legs.
  if (m.wings.stage === 'flaps') {
    const depth = ry * (0.2 + 0.9 * clamp01((m.wings.amount - THRESHOLDS.flaps) / (THRESHOLDS.wings - THRESHOLDS.flaps)));
    const front: Point = [pairs > 1 ? legXs[legXs.length - 1] : rx * 0.5, cy + ry * 0.2];
    const hind: Point = [pairs > 1 ? legXs[0] : -rx * 0.55, cy + ry * 0.2];
    const f = flaps(front, hind, ry * 0.55 + depth, pal);
    main.pieces.push(piece('flaps', f.piece));
    main.detail.push(...f.detail);
  }

  // Body markings and covering texture (clipped to the silhouette, under the near legs' contours).
  const markings: Shape[] = [];
  markings.push(...countershade(B, pal, style === 'armor' ? 0.7 : 1));
  if (c.kind === 'drab' || c.kind === 'camo') markings.push(...saddle(B, pal, c.kind === 'camo' ? 0.6 : 1));
  if (c.camo >= THRESHOLDS.secondary && c.kind !== 'warning') markings.push(...camoBlotches(B, c.kind === 'camo' ? c.camo : c.camo * 0.6, pal, lod));
  // The body plan's coat pattern (Phase 3), under the colouring's own marks.
  markings.push(...coatPattern(B, m.pattern, pal, lod));
  if (c.kind === 'warning') markings.push(...warningBands(B, pal.accent));
  else if (c.display >= 0.33) markings.push(...spots(B, c.display, c.kind === 'bright' ? pal.accent : pal.belly, lod));
  if (style === 'scales') markings.push(...scaleField(B, pal, lod));
  else if (m.cover.scales.role === 'secondary') markings.push(...scaleField(B, pal, lod, m.cover.scales.amount));
  if (style === 'fur') markings.push(...furMarks(B, furry, pal, lod));
  if (style === 'feathers') markings.push(...featherField(B, pal, lod));
  if (style === 'skin') markings.push(...skinCreases(B, pal, lod));
  main.detail.unshift(...markings);

  // Near legs (in the main silhouette: they grow out of the body, no seam).
  let footPt: Point | null = null;
  legXs.forEach((x, i) => {
    const L = buildLeg(x, isFront(i), false, main, i);
    if (i === legXs.length - 1) footPt = [L.ankle[0], 0];
  });

  // Armor plates over the back (overlays with their own outline: armor is meant to look fitted on).
  if (m.cover.armor.role !== 'none') main.over.push(...armorPlates(B, arm, pal, m.cover.armor.role === 'dominant', lod));

  // --- head details
  const D = main.detail;
  const O = main.over;
  const { snL, snD } = H;
  if (m.carnivory < 0.67) D.push({ part: 'head', tag: 'muzzle', kind: 'ellipse', ...H.anchors.muzzle, fill: pal.belly, alpha: 0.85 });
  // Throat and jaw countershading.
  D.push({ part: 'head', tag: 'muzzle', kind: 'ellipse', x: hx + hr * 0.1, y: hy + hr * 0.75 * flatK, rx: hr * 0.75, ry: hr * 0.32, fill: pal.belly, alpha: 0.6 });
  if (c.kind === 'warning') {
    D.push({ part: 'head', tag: 'band', kind: 'curve', closed: true, points: [[hx - hr * 0.25, hy - hr * 1.3], [hx + hr * 0.2, hy - hr * 1.25], [hx + hr * 0.05, hy + hr * 1.2], [hx - hr * 0.45, hy + hr * 1.1]], fill: pal.accent });
  }
  if (style === 'armor') {
    O.push({ part: 'head', tag: 'plate', kind: 'curve', closed: true, points: [[hx - hr * 0.55, hy - hr * 0.75 * flatK], [hx + hr * 0.05, hy - hr * 1.06 * flatK], [hx + hr * 0.7, hy - hr * 0.86 * flatK], [hx + hr * 0.15, hy - hr * 0.55 * flatK]], fill: mix(mix(pal.body.base, pal.bone, 0.28), pal.body.shade, 0.15), stroke: pal.ink, width: 1.3 });
  }
  if (style === 'scales' && lod === 'portrait') {
    for (const [x, y] of [[hx - hr * 0.55, hy + hr * 0.15], [hx - hr * 0.25, hy + hr * 0.4], [hx - hr * 0.6, hy - hr * 0.25], [hx - hr * 0.2, hy - hr * 0.62]] as Point[]) {
      D.push({ part: 'head', tag: 'scale', kind: 'curve', closed: false, points: [[x - 2.6, y], [x, y + 2.2], [x + 2.6, y]], stroke: pal.body.shade, width: 0.9, alpha: 0.7 });
    }
  }
  // Poison gland behind the eye.
  if (c.toxins >= 0.33) {
    O.push({ part: 'head', tag: 'gland', kind: 'ellipse', x: hx - hr * 0.42, y: hy - hr * 0.25, rx: hr * (0.2 + 0.18 * c.toxins), ry: hr * (0.14 + 0.1 * c.toxins), fill: c.kind === 'warning' ? pal.belly : mix(pal.body.base, 0xb7d36b, 0.45), stroke: pal.ink, width: 1 });
  }
  // Ear opening on cold-blooded heads (a pelt covers it).
  if (earHole && style !== 'fur') {
    D.push({ part: 'head', tag: 'ear-hole', kind: 'ellipse', x: hx - hr * 0.55, y: hy + hr * 0.02, rx: hr * (0.12 + 0.28 * m.ear_size), ry: hr * (0.15 + 0.3 * m.ear_size), fill: pal.body.shade, stroke: pal.ink, width: 0.9 });
  }
  // Mouth line, teeth, fangs or tusks.
  const [mA, mM, mB] = H.anchors.mouth;
  D.push({ part: 'head', tag: 'mouth', kind: 'curve', closed: false, points: [mA, mM, mB], stroke: pal.ink, width: 1.4 });
  D.push({ part: 'head', tag: 'mouth', kind: 'curve', closed: false, points: [[mB[0] + hr * 0.12, mB[1] - hr * 0.1], mB, [mB[0] + hr * 0.08, mB[1] + hr * 0.1]], stroke: pal.ink, width: 1.2 });
  if (m.carnivory > 0.5) {
    const n = Math.round(2 + 4 * m.carnivory);
    const ts = hr * 0.08 * (0.6 + m.carnivory);
    for (let i = 0; i < n; i++) {
      const t = (i + 0.7) / (n + 1);
      const x = mA[0] + (mB[0] - mA[0]) * t * 0.8;
      const y = mA[1] + (mB[1] - mA[1]) * t * 0.8;
      O.push({ part: 'head', tag: 'tooth', kind: 'poly', points: [[x - ts, y], [x, y + ts * 1.5], [x + ts, y]], fill: pal.bone, stroke: pal.ink, width: 0.6 });
    }
  }
  let fangTip: Point = H.anchors.fangRoot;
  const tusks = m.carnivory < 0.34;
  if (hasFangs(m)) {
    if (tusks) {
      const len = hr * 1.3 * m.fang_size;
      O.push(tusk(H.anchors.tuskRoot, len, hr * (0.09 + 0.06 * m.fang_size), pal));
      fangTip = [H.anchors.tuskRoot[0] + len * 0.6, H.anchors.tuskRoot[1] - len * 0.65];
    } else {
      const len = hr * 1.1 * m.fang_size;
      O.push(fang(H.anchors.fangRoot, len, hr * (0.09 + 0.07 * m.fang_size), pal));
      fangTip = [H.anchors.fangRoot[0], H.anchors.fangRoot[1] + len];
    }
  } else {
    fangTip = tusks ? H.anchors.tuskRoot : H.anchors.fangRoot;
  }
  // Nose: a pad on warm snouts, a nostril on all; whiskers when keen.
  const [nx, ny] = H.anchors.nose;
  if (warm && (m.nose >= 0.25 || m.carnivory >= 0.34)) O.push({ part: 'head', tag: 'nose', kind: 'ellipse', x: nx, y: ny, rx: hr * (0.13 + 0.14 * m.nose), ry: hr * (0.1 + 0.1 * m.nose), fill: pal.nose, stroke: pal.ink, width: 0.9 });
  O.push({ part: 'head', tag: 'nostril', kind: 'ellipse', x: nx - hr * 0.05, y: ny + hr * 0.03, rx: hr * (0.05 + 0.06 * m.nose), ry: hr * (0.035 + 0.04 * m.nose), fill: pal.ink });
  if (warm && m.nose >= 0.5 && lod === 'portrait') {
    for (const d of [-0.12, 0, 0.12]) {
      O.push({ part: 'head', tag: 'whisker', kind: 'curve', closed: false, points: [[nx - snL * 0.25 - snD * 0.1, ny + hr * 0.25], [nx + hr * 0.3, ny + hr * (0.22 + d * 2)], [nx + hr * 0.85, ny + hr * (0.18 + d * 4)]], stroke: 0xf4eee2, width: 0.7, alpha: 0.85 });
    }
  }
  // Eyes (contract v1): pair k at ±min(170°, eye_azimuth + 50°·k), one of
  // each pair visible from the side, the far eye of a forward-set pair
  // peeking past the bridge; an odd eye on the midline at the brow.
  const er = hr * (0.16 + 0.21 * m.eye_size) * (1.08 - 0.12 * m.size);
  const ep = { size: m.eye_size, cold: !warm, lod };
  const eyePairs = Math.floor(m.eye_count / 2);
  const az0 = Math.max(1, pairAzimuth(m.eye_azimuth, 0));
  if (eyePairs > 0 && az0 < 35) O.push(...eyeShapes(eyeSpot(H, az0, 0, er, true), ep, pal, true));
  for (let k = eyePairs - 1; k >= 0; k--) O.push(...eyeShapes(eyeSpot(H, Math.max(1, pairAzimuth(m.eye_azimuth, k)), k, er), ep, pal));
  if (m.eye_count % 2 === 1) O.push(...eyeShapes(eyeSpot(H, 0, 0, er), ep, pal));
  // A predator's brow ridge.
  if (m.carnivory >= 0.67) O.push({ part: 'head', tag: 'brow', kind: 'curve', closed: false, points: [[hx - hr * 0.05, hy - hr * 0.66 * flatK], [hx + hr * 0.3, hy - hr * 0.68 * flatK], [hx + hr * 0.6, hy - hr * 0.52 * flatK]], stroke: pal.ink, width: 1.8 });
  // Horns (Phase 3): sprites draw them bolder, so they read at marker size.
  const hornBold = lod === 'sprite' ? 1.7 : 1;
  const hornLen = (size: number) => hr * (0.6 + 1.3 * size) * (lod === 'sprite' ? 1.15 : 1) * (m.horn_form === 'branched' ? 1.15 : 1);
  if (m.horns > 0) {
    const len = hornLen(m.horns);
    const base = H.anchors.hornBase;
    if (m.horn_form === 'branched') O.push(...antler(base, len, hr * 0.2 * hornBold, pal, lod));
    else if (m.horn_form === 'straight') O.push(...spikeHorn(base, len, hr * 0.22 * hornBold, pal));
    else O.push(...horn(base, len, hr * 0.24 * hornBold, pal));
  }
  // The tip of each form's centreline (`head.ts`), linear in horn size.
  const tipK: Point = m.horn_form === 'branched' ? [0.35, 0.95] : m.horn_form === 'straight' ? [0.42, 0.9] : [0.3, 1];
  const hornTip: Point = [H.anchors.hornBase[0] - hornLen(m.horn_size) * tipK[0], H.anchors.hornBase[1] - hornLen(m.horn_size) * tipK[1]];

  groups.push(main);
  const wn = wingGroup(false);
  if (wn) groups.push(wn);

  return {
    groups,
    shadow: { part: 'shadow', kind: 'ellipse', x: rx * 0.1, y: 0, rx: rx * 1.2, ry: 3.5 + ry * 0.1, fill: pal.ink, alpha: 0.2 },
    ink: pal.ink,
    scale: 0.6 + 0.4 * m.size,
    pivots: { tail: tailRoot, wing: wingRoot },
    anchors: {
      snoutTip: H.anchors.snoutTip, fangTip, earTip, foot: footPt, tailTip: tailTipPt,
      hornTip,
      body: { cx: 0, cy, rx, ry },
    },
    label: describeAvatar(m),
  };
}

/** Every detail and overlay shape of a drawing (for tests and bounds). */
export function drawingShapes(d: Drawing): Shape[] {
  return d.groups.flatMap((g) => [...g.detail, ...g.over]);
}

/** Bounds of shapes in their own units. */
export function shapeBounds(shapes: Shape[]): { minX: number; maxX: number; minY: number; maxY: number } {
  let minX = Infinity, maxX = -Infinity, minY = Infinity, maxY = -Infinity;
  const addPt = (x: number, y: number, pad = 0) => {
    minX = Math.min(minX, x - pad); maxX = Math.max(maxX, x + pad); minY = Math.min(minY, y - pad); maxY = Math.max(maxY, y + pad);
  };
  for (const s of shapes) {
    const pad = (s.width ?? 0) / 2;
    if (s.kind === 'ellipse') {
      addPt(s.x - s.rx, s.y - s.ry, pad);
      addPt(s.x + s.rx, s.y + s.ry, pad);
    } else for (const [x, y] of s.points) addPt(x, y, pad);
  }
  return { minX, maxX, minY, maxY };
}

// ------------------------------------------------------------------ badges

/** Small icons beside the creature: eggs for egg-layers, a snowflake for a winter coat. Centred on (0, 0). */
export function badgeShapes(m: AvatarModel): { egg: Shape[]; coat: Shape[] } {
  const egg: Shape[] = [];
  const coat: Shape[] = [];
  const eggAt = (x: number, y: number) => {
    egg.push({ part: 'badge', kind: 'ellipse', x, y, rx: 6, ry: 8, fill: 0xf3ead2, stroke: INK, width: 1.2 });
    egg.push({ part: 'badge', kind: 'ellipse', x: x - 1.5, y: y + 1, rx: 1.1, ry: 1.1, fill: 0xa08a64 });
    egg.push({ part: 'badge', kind: 'ellipse', x: x + 2, y: y - 3, rx: 0.9, ry: 0.9, fill: 0xa08a64 });
  };
  if (m.extras.birth === 'incubated_eggs') {
    egg.push({ part: 'badge', kind: 'curve', closed: true, points: [[-14, 2], [0, 12], [14, 2], [0, 6]], fill: 0x8a6a3e, stroke: INK, width: 1 });
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
      const [cc, ss] = [Math.cos(a) * 10, Math.sin(a) * 10];
      coat.push({ part: 'badge', kind: 'line', points: [[-cc, -ss], [cc, ss]], stroke: 0xbfe3ff, width: 2 });
    }
    coat.push({ part: 'badge', kind: 'ellipse', x: 0, y: 0, rx: 2.5, ry: 2.5, fill: 0xbfe3ff });
  }
  return { egg, coat };
}

// ------------------------------------------------------------------ renderer

/** The SVG viewBox the avatar is drawn in: ground line at `ground`. */
export const VIEW = { w: 300, h: 200, ground: 180 } as const;
/** Outline weight on screen (viewBox units), whatever the creature's size. */
export const OUTLINE_PX = 1.9;
/** Highlight and shade rims (creature units). */
export const RIM = { light: 2.4, shade: 5.5 } as const;

export interface SvgOptions {
  /** Idle breathing, tail sway and wing flex (SMIL; leave off for prefers-reduced-motion). */
  animate: boolean;
  /** Id prefix for the clip paths (two avatars on one page need different ones). Default `av`. */
  id?: string;
}

/** The creature's markup at a scale (no <svg> wrapper): for embedding in another drawing (the builder's side view). */
export function avatarMarkup(d: AvatarDrawing, k: number, opts: SvgOptions): string {
  const breathe = opts.animate
    ? '<animateTransform attributeName="transform" type="scale" values="1 1;1.004 1.018;1 1" dur="3.2s" repeatCount="indefinite" calcMode="spline" keySplines=".45 0 .55 1;.45 0 .55 1"/>'
    : '';
  const shadow = d.shadow ? shapeSvg(d.shadow) : '';
  const body = drawingSvg({ ...d, shadow: null }, { id: opts.id ?? 'av', outline: OUTLINE_PX / k, rim: RIM, animate: opts.animate });
  return `${shadow}<g data-part="creature">${breathe}${body}</g>`;
}

/**
 * The avatar as an SVG document string. The creature is scaled by body size
 * (and shrunk further only if it would not fit), standing on a ground line.
 */
export function avatarSvg(m: AvatarModel, opts: SvgOptions): string {
  const d = avatarDrawing(m);
  const pad = 8;
  const b = drawingBounds(d);
  const fit = Math.min((VIEW.w - 2 * pad) / (b.maxX - b.minX), (VIEW.ground - pad) / -b.minY, (VIEW.h - VIEW.ground) / Math.max(1, b.maxY));
  const k = Math.min(d.scale * 1.35, fit);
  const tx = VIEW.w / 2 - (k * (b.minX + b.maxX)) / 2;
  const badges = badgeShapes(m);
  const badge = (shapes: Shape[], x: number, y: number, name: string) =>
    shapes.length ? `<g data-badge="${name}" transform="translate(${x} ${y})">${shapes.map(shapeSvg).join('')}</g>` : '';
  return (
    `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 ${VIEW.w} ${VIEW.h}" preserveAspectRatio="xMidYMax meet" aria-hidden="true" focusable="false">` +
    `<line x1="10" y1="${VIEW.ground}" x2="${VIEW.w - 10}" y2="${VIEW.ground}" stroke="#30363d" stroke-width="1.5" stroke-linecap="round"/>` +
    `<g transform="translate(${fmt(tx)} ${VIEW.ground}) scale(${Math.round(k * 1000) / 1000})">` +
    avatarMarkup(d, k, opts) +
    `</g>` +
    badge(badges.egg, 24, 24, 'egg') +
    badge(badges.coat, VIEW.w - 22, 22, 'coat') +
    `</svg>`
  );
}

/**
 * A small sprite of the creature (`size` px square, transparent): the sprite
 * level of detail (no textures, bolder outline), centred and fitted. How the
 * world map can draw founders with this library (art/STYLE.md, "Sprites").
 */
export function avatarSpriteSvg(m: AvatarModel, size: number, opts: { id?: string } = {}): string {
  const d = avatarDrawing(m, { lod: 'sprite' });
  const b = drawingBounds(d);
  const pad = 2;
  const k = Math.min((size - 2 * pad) / (b.maxX - b.minX), (size - 2 * pad) / (b.maxY - b.minY));
  const tx = size / 2 - (k * (b.minX + b.maxX)) / 2;
  const ty = size / 2 - (k * (b.minY + b.maxY)) / 2;
  const body = drawingSvg({ ...d, shadow: null }, { id: opts.id ?? 'sp', outline: Math.max(1, size / 40) / k, rim: { light: 3 / (k * 1.2), shade: 6 / (k * 1.2) }, animate: false });
  return `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 ${size} ${size}" width="${size}" height="${size}"><g transform="translate(${fmt(tx)} ${fmt(ty)}) scale(${Math.round(k * 1000) / 1000})">${body}</g></svg>`;
}
