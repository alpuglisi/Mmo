// Creature sprites as data: from an organism's visible body plan (the
// server's `look`) to a list of simple shapes, drawn in side view facing +x
// around (0, 0) in screen pixels. Pure (no PixiJS) so it is unit-tested;
// `sprites.ts` renders each distinct class once into a cached texture.
//
// Cues, so a glance tells body plans apart at marker size:
// - size class → body radius; juveniles are smaller with a bigger head;
// - diet → herbivores a round muzzle (horns when large), omnivores a short
//   snout, carnivores a long pointed snout with fangs and a brow;
// - endothermy → warm-blooded bodies are round with ears and a stubby tail,
//   cold-blooded ones long and low with a tapering tail;
// - cover → fur is a ragged fringe, scales a row of dots, armor plates and a
//   heavy outline, bare skin a pale belly;
// - coloring → drab browns, camouflage blotches, bright display colors, or
//   yellow-and-black warning bands;
// - locomotion → legs, fins and a tail fin (swimmers), a membrane (gliders)
//   or a raised wing (fliers).
//
// Morphology (docs/morphology-contract.md, quantized by the server), drawn in
// the same side view; the defaults (`morphOf` of a look without the fields)
// draw exactly the sprites above:
// - legs → 0, 2, 4, 6 or 8 visible legs (swimmers show fins instead);
// - ears → 0, 2 or 4: the near ear of each pair (contract v1),
//   shaped round (today's ear), pointed, long or fan, in three
//   sizes; round ears are flush on cold-blooded and swimming bodies, as today;
// - eyes → the near-side eyes (one, a single midline eye, is bigger and more
//   frontal), in three sizes; placement: forward-set eyes (predators) sit at
//   the front of the head near the snout with the far eye peeking past the
//   near one, side-set eyes (prey) high and back on the side of the head;
// - tail → none, thin (today's tail for the body type), bushy, fin, club or a
//   prehensile curl, in three lengths;
// - snout → three lengths of the diet's muzzle or snout; fangs → none, small
//   (today's carnivore fangs) or large;
// - detail → markers drawn smaller than `NEAR_SCALE` (zoomed out) use the
//   class sprite (`far`: morphology at its defaults), so few textures serve
//   a crowded wide view.

import type { Appearance, EarShape, NearbyOrganism, OwnOrganism, SizeBucket, TailLook } from '../protocol';

export type Point = [number, number];

/** Which feature a shape draws (for tests; drawing ignores it). */
export type Part = 'leg' | 'ear' | 'eye' | 'tail' | 'fang' | 'snout';

export type Shape =
  | { kind: 'ellipse'; x: number; y: number; rx: number; ry: number; fill?: number; alpha?: number; stroke?: number; width?: number; part?: Part }
  | { kind: 'poly'; points: Point[]; fill?: number; alpha?: number; stroke?: number; width?: number; part?: Part }
  | { kind: 'line'; points: Point[]; stroke: number; width: number; alpha?: number; part?: Part };

/** The morphology a sprite draws: every field present, clamped to its values. */
export interface Morph {
  eyes: number;
  eyeSize: SizeBucket;
  /** Eye placement: 0 forward-set, 1 the default, 2 side-set (`look.eye_azimuth`). */
  eyeSet: SizeBucket;
  ears: number;
  earShape: EarShape;
  earSize: SizeBucket;
  legs: number;
  tail: TailLook;
  tailSize: SizeBucket;
  snout: SizeBucket;
  fangs: SizeBucket;
}

export const EAR_SHAPES: readonly EarShape[] = ['round', 'pointed', 'long', 'fan'];
export const TAIL_LOOKS: readonly TailLook[] = ['none', 'thin', 'bushy', 'fin', 'club', 'prehensile'];
const LEG_COUNTS = [0, 2, 4, 6, 8] as const;
/** Length/size factor of each size bucket (medium is today's sprite). */
const EYE_SCALE = [0.75, 1, 1.3] as const;
const EAR_SCALE = [0.7, 1, 1.35] as const;
const TAIL_SCALE = [0.7, 1, 1.35] as const;
const SNOUT_SCALE = [0.7, 1, 1.4] as const;

/** What is drawn for a creature: its appearance class and life stage. */
export interface CreatureSpec {
  look: Appearance;
  juvenile: boolean;
  /**
   * Far detail (markers drawn small): the class sprite, morphology at its
   * defaults, so zoomed out every body plan shares one texture per class.
   */
  far?: boolean;
}

/** Small icon over a marker for what the organism is doing. */
export type Hint = 'sleep' | 'flee' | 'hunt' | 'mate';

/** For organisms from a server that doesn't send `look`. */
export const DEFAULT_LOOK: Appearance = { size: 1, diet: 'herbivore', warm: true, locomotion: 'walk', cover: 'fur', coloring: 'drab' };

const OUTLINE = 0x15181c;
const SIZE_RADIUS = [4, 5.5, 7, 9] as const;
/** Juveniles are drawn at this fraction of the adult size. */
export const JUVENILE_SCALE = 0.65;

/** Markers drawn smaller than this scale use the far (class) sprite. */
export const NEAR_SCALE = 1;

export function specOf(o: OwnOrganism | NearbyOrganism, far = false): CreatureSpec {
  const s: CreatureSpec = { look: o.look ?? DEFAULT_LOOK, juvenile: !o.matured };
  if (far) s.far = true;
  return s;
}

function bucket(v: unknown, dflt: SizeBucket): SizeBucket {
  return typeof v === 'number' && Number.isFinite(v) ? (Math.min(2, Math.max(0, Math.round(v))) as SizeBucket) : dflt;
}

function count(v: unknown, lo: number, hi: number, dflt: number): number {
  return typeof v === 'number' && Number.isFinite(v) ? Math.min(hi, Math.max(lo, Math.round(v))) : dflt;
}

/**
 * The look's morphology with defaults for absent fields (older servers):
 * two eyes, two round ears, four legs, a thin tail, medium sizes, and fangs
 * on carnivores only, which draws today's sprites. Values out of range are
 * clamped, so a key has a bounded number of values per field.
 */
export function morphOf(l: Appearance): Morph {
  const legs = count(l.legs, 0, 8, 4);
  // Contract v1: ears come in pairs (0, 2 or 4).
  const ears = count(l.ears, 0, 4, 2);
  return {
    eyes: count(l.eyes, 1, 6, 2),
    eyeSize: bucket(l.eye_size, 1),
    eyeSet: bucket(l.eye_azimuth, 1),
    ears: ears >= 3 ? 4 : ears >= 1 ? 2 : 0,
    earShape: EAR_SHAPES.includes(l.ear_shape as EarShape) ? (l.ear_shape as EarShape) : 'round',
    earSize: bucket(l.ear_size, 1),
    legs: LEG_COUNTS.reduce((best, n) => (Math.abs(n - legs) < Math.abs(best - legs) ? n : best), 4),
    tail: TAIL_LOOKS.includes(l.tail as TailLook) ? (l.tail as TailLook) : 'thin',
    tailSize: bucket(l.tail_size, 1),
    snout: bucket(l.snout, 1),
    fangs: bucket(l.fangs, l.diet === 'carnivore' ? 1 : 0),
  };
}

/**
 * What of the morphology the sprite shows: `morphOf` with what is not drawn
 * collapsed to one value (swimmers' legs, flush round ears, an absent tail's
 * length), so looks that draw the same share a texture.
 */
export function visibleMorph(l: Appearance, far = false): Morph {
  const m = morphOf(far ? classOf(l) : l);
  const swim = l.locomotion === 'swim';
  if (swim) m.legs = 0;
  if (m.ears === 0 || (m.earShape === 'round' && (!l.warm || swim))) {
    m.ears = 0;
    m.earShape = 'round';
    m.earSize = 1;
  }
  if (m.tail === 'none') m.tailSize = 1;
  // A single eye sits on the midline wherever the pairs would go.
  if (m.eyes === 1) m.eyeSet = 1;
  return m;
}

/** The look's class fields only (what a far sprite draws). */
function classOf(l: Appearance): Appearance {
  return { size: l.size, diet: l.diet, warm: l.warm, locomotion: l.locomotion, cover: l.cover, coloring: l.coloring };
}

/** Eye placement in the key: nothing for the default, so default keys are unchanged. */
const EYE_SET_CODE = ['f', '', 's'] as const;
const TAIL_CODE: Record<TailLook, string> = { none: 'n', thin: 't', bushy: 'b', fin: 'f', club: 'c', prehensile: 'p' };

/**
 * Cache key of a creature texture: one per distinct drawing (appearance
 * class, visible morphology and stage). Every field is quantized, so the
 * number of keys is bounded (see README, "Creature sprites").
 */
export function creatureKey(s: CreatureSpec): string {
  const l = s.look;
  // One letter per field value (skin and scales differ in their second letter).
  const cover = l.cover === 'scales' ? 'c' : l.cover[0];
  const m = visibleMorph(l, s.far);
  return `${l.size}${l.diet[0]}${l.warm ? 'w' : 'c'}${l.locomotion[0]}${cover}${l.coloring[0]}${s.juvenile ? 'j' : 'a'}`
    + `-${m.eyes}${m.eyeSize}${EYE_SET_CODE[m.eyeSet]}${m.ears}${m.earShape[0]}${m.earSize}${m.legs}${TAIL_CODE[m.tail]}${m.tailSize}${m.snout}${m.fangs}`;
}

/** Marker scale for a zoom (pixels per tile): 1 at the default zoom, bigger zoomed in, smaller zoomed out. */
export function markerScaleFor(zoom: number): number {
  return Math.min(1.9, Math.max(0.75, 0.7 + zoom / 26));
}

/** Body radius in screen pixels (before the zoom-dependent marker scale). */
export function bodyRadius(s: CreatureSpec): number {
  const size = Math.min(3, Math.max(0, Math.round(s.look.size)));
  return SIZE_RADIUS[size] * (s.juvenile ? JUVENILE_SCALE : 1);
}

/** Half the side of the square a creature (with wings, tail and icon room) fits in. */
export function spriteExtent(s: CreatureSpec): number {
  return Math.ceil(bodyRadius(s) * 3.2) + 2;
}

/** Half the side of the body texture: `spriteExtent`, or more when long features reach past it. */
export function bodyExtent(s: CreatureSpec, shapes: Shape[] = creatureShapes(s)): number {
  let m = 0;
  for (const sh of shapes) {
    const pad = sh.kind === 'line' ? sh.width / 2 : (sh.width ?? 0) / 2;
    if (sh.kind === 'ellipse') m = Math.max(m, Math.abs(sh.x) + sh.rx + pad, Math.abs(sh.y) + sh.ry + pad);
    else for (const [x, y] of sh.points) m = Math.max(m, Math.abs(x) + pad, Math.abs(y) + pad);
  }
  return Math.max(spriteExtent(s), Math.ceil(m + 1));
}

const SIZE_WORDS = ['tiny', 'small', 'medium', 'large'] as const;
const LOCOMOTION_WORDS = { walk: 'walks', swim: 'swims', glide: 'glides', fly: 'flies' } as const;
const COLORING_WORDS = { drab: 'drab', camo: 'camouflaged', bright: 'brightly colored', warning: 'warning colors' } as const;

/** The body plan in words, for the inspector: "small warm-blooded herbivore · fur · drab · walks". */
export function describeLook(l: Appearance): string {
  const size = SIZE_WORDS[Math.min(3, Math.max(0, l.size))];
  return `${size} ${l.warm ? 'warm' : 'cold'}-blooded ${l.diet} · ${l.cover} · ${COLORING_WORDS[l.coloring]} · ${LOCOMOTION_WORDS[l.locomotion]}`;
}

export function actionHint(action: string): Hint | null {
  switch (action) {
    case 'rest':
    case 'dormant':
      return 'sleep';
    case 'flee':
      return 'flee';
    case 'hunt':
    case 'scavenge':
      return 'hunt';
    case 'seek_mate':
      return 'mate';
    default:
      return null;
  }
}

export interface Palette {
  base: number;
  dark: number;
  light: number;
  /** Pattern color: blotches, bands, crest. */
  accent: number;
}

/**
 * How far a fur coat reaches beyond the body, as a fraction of its size: the
 * world sprite's fringe, and the founder avatar's pelt at the fur threshold
 * (avatar.ts `furReach`), so a furred body reads as fur in both. The covering
 * itself is `Appearance.cover` (appearance.rs), which the caption
 * (`describeLook`) names: one class for the words and both drawings.
 */
export const FUR_FRINGE = 0.18;

export function palette(l: Appearance): Palette {
  switch (l.coloring) {
    case 'camo':
      return { base: 0x71804a, dark: 0x4b5632, light: 0x9aa676, accent: 0x3f4a26 };
    case 'bright':
      return { base: 0xe2519d, dark: 0x9c2d6a, light: 0xf7a6cf, accent: 0x3fd4e8 };
    case 'warning':
      return { base: 0xffc61a, dark: 0xb38600, light: 0xffe27a, accent: 0x1b1b1b };
    case 'drab':
    default:
      return l.warm
        ? { base: 0xa47a52, dark: 0x6e4f33, light: 0xd2b48c, accent: 0x7d5b3c }
        : { base: 0x8d936b, dark: 0x5c6146, light: 0xbcc19a, accent: 0x6b7050 };
  }
}

/** Positions of `n` visible legs along the body, as fractions of its half-length (4 is today's). */
function legPositions(n: number): number[] {
  switch (n) {
    case 0: return [];
    case 2: return [-0.18, 0.22];
    case 6: return [-0.65, -0.4, -0.15, 0.1, 0.35, 0.6];
    case 8: return [-0.7, -0.5, -0.3, -0.1, 0.1, 0.3, 0.5, 0.7];
    default: return [-0.6, -0.25, 0.3, 0.6];
  }
}

/** Where the visible ears sit (head-radius offsets from today's ear, back to front): each pair shows its near ear. */
function earSlots(n: number): { dx: number; dy: number }[] {
  if (n === 0) return [];
  return n >= 4 ? [{ dx: -0.42, dy: 0.18 }, { dx: 0, dy: 0 }] : [{ dx: 0, dy: 0 }];
}

/** How far eye placement moves the paired eyes (head radii): forward, default, side. */
const EYE_SET_SHIFT: readonly Point[] = [[0.2, -0.02], [0, 0], [-0.28, -0.24]];

/**
 * Where the visible eyes sit on the head and their size relative to today's
 * eye (main eye first). Contract v1: an odd count has one eye on the midline
 * (on the forehead; alone, a big frontal eye), pairs further back show their
 * near eye higher and further back on the head.
 */
function eyeSlots(n: number, set: SizeBucket = 1): { x: number; y: number; k: number }[] {
  // Placement moves the paired eyes: forward toward the snout, or to the
  // side, higher and further back. The midline eye stays on the brow (a
  // little higher and forward over forward-set pairs, to clear them).
  const [dx, dy] = EYE_SET_SHIFT[set];
  const pair = (x: number, y: number, k: number) => ({ x: x + dx, y: y + dy, k });
  const main = pair(0.25, -0.2, 1);
  const upper = pair(-0.12, -0.52, 0.75);
  const lower = pair(-0.3, -0.02, 0.7);
  const mid = set === 0 ? { x: 0.6, y: -0.66, k: 0.7 } : { x: 0.52, y: -0.56, k: 0.75 };
  switch (n) {
    case 1: return [{ x: 0.42, y: -0.3, k: 1.3 }];
    case 3: return [main, mid];
    case 4: return [main, upper];
    case 5: return [main, upper, mid];
    case 6: return [main, upper, lower];
    default: return [main];
  }
}

/** The shapes of a creature, back to front, facing +x. */
export function creatureShapes(s: CreatureSpec): Shape[] {
  const l = s.look;
  const m = visibleMorph(l, s.far);
  const r = bodyRadius(s);
  const p = palette(l);
  const out: Shape[] = [];
  const swim = l.locomotion === 'swim';
  const armored = l.cover === 'armor';
  const outlineW = armored ? 1.6 : 1;
  // Body proportions: warm-blooded round, cold-blooded long and low.
  const rx = r * (l.warm ? 1.15 : 1.4);
  const ry = r * (l.warm ? 0.85 : 0.62);
  const headR = r * (l.warm ? 0.52 : 0.44) * (s.juvenile ? 1.2 : 1);
  const hx = rx * 0.9;
  const hy = -ry * (l.warm ? 0.45 : 0.15);

  // Wings behind the body.
  if (l.locomotion === 'fly') {
    out.push({ kind: 'poly', points: [[-rx * 0.2, -ry * 0.4], [-rx * 1.1, -ry * 2.4], [rx * 0.1, -ry * 1.6], [rx * 0.35, -ry * 0.5]],
      fill: p.light, stroke: OUTLINE, width: 1 });
    out.push({ kind: 'line', points: [[-rx * 0.1, -ry * 0.5], [-rx * 0.75, -ry * 1.9]], stroke: p.dark, width: 0.8 });
  } else if (l.locomotion === 'glide') {
    out.push({ kind: 'poly', points: [[-rx * 0.9, 0], [-rx * 1.05, ry * 0.95], [rx * 0.9, ry * 0.95], [rx * 0.7, 0]],
      fill: p.light, alpha: 0.9, stroke: OUTLINE, width: 0.8 });
  }

  // Legs (walkers, gliders, fliers), or fins.
  if (!swim) {
    const legLen = ry * (l.warm ? 1.0 : 0.7) + r * 0.25;
    // Many legs are thinner (arthropod-like).
    const legW = Math.max(1.2, r * 0.22) * (m.legs > 4 ? 0.75 : 1);
    for (const fx of legPositions(m.legs)) {
      const x = rx * fx;
      out.push({ kind: 'line', points: [[x, ry * 0.4], [x + (l.warm ? 0 : rx * 0.12), ry * 0.4 + legLen]], stroke: OUTLINE, width: legW, part: 'leg' });
    }
  } else {
    out.push({ kind: 'poly', points: [[-rx * 0.1, ry * 0.5], [-rx * 0.5, ry * 1.35], [rx * 0.25, ry * 0.7]], fill: p.dark, stroke: OUTLINE, width: 0.8 });
    // Dorsal fin.
    out.push({ kind: 'poly', points: [[-rx * 0.35, -ry * 0.85], [-rx * 0.05, -ry * 1.75], [rx * 0.3, -ry * 0.85]], fill: p.dark, stroke: OUTLINE, width: 0.8 });
  }

  out.push(...tailShapes(m, l.warm, swim, r, rx, ry, p, outlineW));

  // Fur: a ragged fringe just outside the body.
  if (l.cover === 'fur') {
    const n = 14;
    const pts: Point[] = [];
    for (let i = 0; i < n * 2; i++) {
      const a = (i / (n * 2)) * Math.PI * 2;
      const k = i % 2 === 0 ? 1 + FUR_FRINGE : 1.0;
      pts.push([Math.cos(a) * rx * k, Math.sin(a) * ry * k]);
    }
    out.push({ kind: 'poly', points: pts, fill: p.dark, stroke: OUTLINE, width: 0.6 });
  }

  // Body.
  out.push({ kind: 'ellipse', x: 0, y: 0, rx, ry, fill: p.base, stroke: OUTLINE, width: l.cover === 'fur' ? 0 : outlineW });
  if (l.cover === 'skin') out.push({ kind: 'ellipse', x: 0, y: ry * 0.35, rx: rx * 0.7, ry: ry * 0.4, fill: p.light, alpha: 0.85 });

  // Coloring patterns.
  if (l.coloring === 'camo') {
    out.push({ kind: 'ellipse', x: -rx * 0.4, y: -ry * 0.2, rx: rx * 0.25, ry: ry * 0.3, fill: p.accent, alpha: 0.85 });
    out.push({ kind: 'ellipse', x: rx * 0.25, y: ry * 0.15, rx: rx * 0.2, ry: ry * 0.25, fill: p.accent, alpha: 0.85 });
  } else if (l.coloring === 'warning') {
    for (const fx of [-0.45, 0.05]) {
      out.push({ kind: 'poly', points: [[rx * fx, -ry * 0.95], [rx * (fx + 0.2), -ry * 0.95], [rx * (fx + 0.12), ry * 0.95], [rx * (fx - 0.08), ry * 0.95]], fill: p.accent });
    }
  } else if (l.coloring === 'bright') {
    // Display crest along the back.
    out.push({ kind: 'poly', points: [[-rx * 0.5, -ry * 0.8], [-rx * 0.25, -ry * 1.45], [0, -ry * 0.9], [rx * 0.25, -ry * 1.4], [rx * 0.45, -ry * 0.75]],
      fill: p.accent, stroke: OUTLINE, width: 0.7 });
  }

  // Covering details.
  if (l.cover === 'scales') {
    for (const fx of [-0.5, -0.15, 0.2]) out.push({ kind: 'ellipse', x: rx * fx, y: -ry * 0.35, rx: r * 0.16, ry: r * 0.16, fill: p.dark });
  } else if (armored) {
    for (const fx of [-0.45, -0.05, 0.35]) {
      out.push({ kind: 'line', points: [[rx * fx, -ry * 0.95], [rx * (fx + 0.05), ry * 0.6]], stroke: OUTLINE, width: 1.2 });
    }
  }

  // Ears, behind the head.
  for (const slot of earSlots(m.ears)) out.push(earShape(m, hx + slot.dx * headR, hy + slot.dy * headR, headR, p));
  // Horns on large herbivores.
  if (l.diet === 'herbivore' && l.size >= 2 && !s.juvenile) {
    out.push({ kind: 'line', points: [[hx + headR * 0.1, hy - headR * 0.8], [hx + headR * 0.2, hy - headR * 1.7], [hx + headR * 0.7, hy - headR * 1.9]], stroke: 0xe8e0c8, width: 1.4 });
  }

  // Head and snout (its length scaled by the snout bucket; medium is today's).
  out.push({ kind: 'ellipse', x: hx, y: hy, rx: headR, ry: headR, fill: p.base, stroke: OUTLINE, width: outlineW });
  const sk = SNOUT_SCALE[m.snout];
  /** Where fangs hang from: x (head radii from the head center) and the jaw's y there. */
  let fangAt: Point[];
  if (l.diet === 'herbivore') {
    const mx = 0.85 + (sk - 1) * 0.9;
    const mrx = 0.5 * (sk > 1 ? 1 + (sk - 1) * 0.8 : 1);
    if (sk > 1) {
      // A long face: a bridge from the head to the muzzle.
      out.push({ kind: 'poly', points: [[hx + headR * 0.4, hy - headR * 0.35], [hx + headR * mx, hy - headR * 0.05], [hx + headR * mx, hy + headR * 0.65], [hx + headR * 0.4, hy + headR * 0.75]],
        fill: p.base, stroke: OUTLINE, width: 0.8, part: 'snout' });
    }
    out.push({ kind: 'ellipse', x: hx + headR * mx, y: hy + headR * 0.3, rx: headR * mrx, ry: headR * 0.42, fill: p.light, stroke: OUTLINE, width: 0.7, part: 'snout' });
    fangAt = [[mx - 0.05, 0.62], [mx + mrx * 0.55, 0.55]];
  } else if (l.diet === 'omnivore') {
    const tip = 0.6 + 1.0 * sk;
    out.push({ kind: 'poly', points: [[hx + headR * 0.6, hy - headR * 0.45], [hx + headR * tip, hy + headR * 0.2], [hx + headR * 0.6, hy + headR * 0.6]],
      fill: p.base, stroke: OUTLINE, width: 0.8, part: 'snout' });
    // The jaw runs from (0.6, 0.6) to the tip at (tip, 0.2).
    const jaw = (f: number): Point => [0.6 + (tip - 0.6) * f, 0.6 - 0.4 * f - 0.05];
    fangAt = [jaw(0.25), jaw(0.55)];
  } else {
    const tip = 0.5 + 1.6 * sk;
    out.push({ kind: 'poly', points: [[hx + headR * 0.5, hy - headR * 0.55], [hx + headR * tip, hy + headR * 0.15], [hx + headR * 0.5, hy + headR * 0.65]],
      fill: p.dark, stroke: OUTLINE, width: 0.9, part: 'snout' });
    // Today's fangs at 1.0 and 1.45 head radii, moved with the snout's length.
    fangAt = [[0.5 + 0.5 * sk, 0.35], [0.5 + 0.95 * sk, 0.35]];
  }
  if (m.fangs > 0) {
    const big = m.fangs === 2;
    for (const [fx, fy] of fangAt) {
      out.push({ kind: 'poly', points: big
        ? [[hx + headR * (fx - 0.04), hy + headR * fy], [hx + headR * (fx + 0.16), hy + headR * (fy + 1.0)], [hx + headR * (fx + 0.38), hy + headR * (fy - 0.05)]]
        : [[hx + headR * fx, hy + headR * fy], [hx + headR * (fx + 0.18), hy + headR * (fy + 0.6)], [hx + headR * (fx + 0.32), hy + headR * (fy - 0.05)]],
      fill: 0xffffff, ...(big ? { stroke: OUTLINE, width: 0.5 } : {}), part: 'fang' });
    }
  }
  // Eyes (a brow over the main one on carnivores).
  const ek = EYE_SCALE[m.eyeSize];
  eyeSlots(m.eyes, m.eyeSet).forEach((e, i) => {
    const ex = hx + headR * e.x;
    const ey = hy + headR * e.y;
    const er = headR * 0.32 * e.k * ek;
    if (i === 0 && m.eyeSet === 0 && m.eyes >= 2) {
      // Forward-set: the far eye of the front pair peeks over the bridge of
      // the snout, ahead of and above the near one (binocular, predator look).
      const fx = ex + er * 0.75;
      const fy = ey - er * 0.8;
      const fr = er * 0.72;
      out.push({ kind: 'ellipse', x: fx, y: fy, rx: fr, ry: fr, fill: 0xffffff, part: 'eye' });
      out.push({ kind: 'ellipse', x: fx + fr * 0.3, y: fy, rx: fr * 0.55, ry: fr * 0.55, fill: OUTLINE });
    }
    out.push({ kind: 'ellipse', x: ex, y: ey, rx: er, ry: er, fill: 0xffffff, part: 'eye' });
    out.push({ kind: 'ellipse', x: ex + er * 0.25, y: ey, rx: er * 0.53125, ry: er * 0.53125, fill: OUTLINE });
    if (i === 0 && l.diet === 'carnivore') {
      const b = er / 0.32; // today's brow, in head radii at the medium eye
      out.push({ kind: 'line', points: [[ex - b * 0.35, ey - b * 0.5], [ex + b * 0.4, ey - b * 0.25]], stroke: OUTLINE, width: 1.1 });
    }
  });
  return out;
}

/** One ear with its base on the head at (ax, ay): today's ear (round, medium) is the reference. */
function earShape(m: Morph, ax: number, ay: number, h: number, p: Palette): Shape {
  const k = EAR_SCALE[m.earSize];
  // Today's ear in head radii: base from (-0.6, -0.6) to (0.05, -0.75), tip at (-0.35, -1.55).
  const b1: Point = [ax - h * 0.6, ay - h * 0.6];
  const b2: Point = [ax + h * 0.05, ay - h * 0.75];
  const cx = (b1[0] + b2[0]) / 2;
  const cy = (b1[1] + b2[1]) / 2;
  const at = (dx: number, dy: number): Point => [cx + h * dx, cy + h * dy];
  switch (m.earShape) {
    case 'pointed':
      // Taller and sharper than the round ear, on the same base.
      return { kind: 'poly', points: [at(-0.34, 0.04), at(0.1 * k, -1.65 * k), at(0.34, -0.04)], fill: p.dark, stroke: OUTLINE, width: 0.7, part: 'ear' };
    case 'long':
      // A long rabbit ear swept back, with a rounded tip.
      return { kind: 'poly', points: [at(-0.3, 0.05), at(-0.6 * k, -1.4 * k), at(-0.95 * k, -2.2 * k), at(-0.62 * k, -2.45 * k), at(-0.15 * k, -1.5 * k), at(0.3, -0.05)],
        fill: p.dark, stroke: OUTLINE, width: 0.7, part: 'ear' };
    case 'fan':
      // A broad round flap standing out behind the head.
      return { kind: 'ellipse', x: cx - h * 0.45 * k, y: cy - h * 0.05 * k, rx: h * 0.72 * k, ry: h * 0.85 * k, fill: p.dark, stroke: OUTLINE, width: 0.7, part: 'ear' };
    case 'round':
    default:
      return { kind: 'poly', points: [b1, at(-0.075 * k, -0.875 * k), b2], fill: p.dark, stroke: OUTLINE, width: 0.7, part: 'ear' };
  }
}

/** The tail, by shape and length; `thin` is today's tail for the body type. */
function tailShapes(m: Morph, warm: boolean, swim: boolean, r: number, rx: number, ry: number, p: Palette, outlineW: number): Shape[] {
  const k = TAIL_SCALE[m.tailSize];
  // Scale a tail's points away from its root (bx, by) by the length factor.
  const grow = (pts: Point[], bx: number, by: number): Point[] => pts.map(([x, y]) => [bx + (x - bx) * k, by + (y - by) * k]);
  const finPts = (bx: number): Point[] => [[bx, 0], [bx - rx * 0.65, -ry * 0.95], [bx - rx * 0.45, 0], [bx - rx * 0.65, ry * 0.95]];
  switch (m.tail) {
    case 'none':
      return [];
    case 'thin':
      if (swim) return [{ kind: 'poly', points: grow(finPts(-rx * 0.9), -rx * 0.9, 0), fill: p.dark, stroke: OUTLINE, width: 0.8, part: 'tail' }];
      if (warm) {
        return [{ kind: 'poly', points: grow([[-rx * 0.85, -ry * 0.35], [-rx * 1.35, -ry * 0.75], [-rx * 0.9, ry * 0.15]], -rx * 0.85, -ry * 0.1),
          fill: p.dark, stroke: OUTLINE, width: 0.8, part: 'tail' }];
      }
      return [{ kind: 'poly', points: grow([[-rx * 0.8, -ry * 0.4], [-rx * 2.0, ry * 0.15], [-rx * 0.8, ry * 0.45]], -rx * 0.8, 0),
        fill: p.base, stroke: OUTLINE, width: outlineW, part: 'tail' }];
    case 'bushy': {
      // A ragged plume raised up and back.
      // Its long axis runs from the rump up and back (40° above horizontal).
      const cx = -rx * 0.9 - r * 0.45 * k;
      const cy = -ry * 0.3 - r * 0.4 * k;
      const a0 = (40 * Math.PI) / 180;
      const pts: Point[] = [];
      for (let i = 0; i < 16; i++) {
        const a = (i / 16) * Math.PI * 2;
        const rr = i % 2 === 0 ? 1 : 0.78;
        const ux = Math.cos(a) * r * 0.8 * k * rr;
        const uy = Math.sin(a) * r * 0.45 * k * rr;
        pts.push([cx + ux * Math.cos(a0) - uy * Math.sin(a0), cy + ux * Math.sin(a0) + uy * Math.cos(a0)]);
      }
      return [{ kind: 'poly', points: pts, fill: p.dark, stroke: OUTLINE, width: 0.6, part: 'tail' }];
    }
    case 'fin': {
      // A tall crescent fin on a short stalk.
      const bx = -rx * 0.85;
      return [{ kind: 'poly', points: grow([[bx, -ry * 0.25], [bx - rx * 0.75, -ry * 1.3], [bx - rx * 0.42, 0], [bx - rx * 0.75, ry * 1.1], [bx, ry * 0.25]], bx, 0),
        fill: p.dark, stroke: OUTLINE, width: 0.8, part: 'tail' }];
    }
    case 'club': {
      // A tapering tail ending in a heavy knob.
      const bx = -rx * 0.8;
      const tip: Point = [bx - r * 1.5 * k, ry * 0.1];
      return [
        { kind: 'poly', points: [[bx, -ry * 0.35], tip, [bx, ry * 0.4]], fill: p.base, stroke: OUTLINE, width: outlineW, part: 'tail' },
        { kind: 'ellipse', x: tip[0], y: tip[1], rx: r * 0.38 * k, ry: r * 0.32 * k, fill: p.dark, stroke: OUTLINE, width: 1, part: 'tail' },
      ];
    }
    case 'prehensile': {
      // A long tail curling up at the end: an outlined stroke.
      const bx = -rx * 0.85;
      const L = r * k;
      const pts: Point[] = [[bx, -ry * 0.05], [bx - L * 0.6, -ry * 0.05 - L * 0.08], [bx - L * 1.1, -L * 0.3], [bx - L * 1.2, -L * 0.8], [bx - L * 0.9, -L * 1.08],
        [bx - L * 0.58, -L * 0.9], [bx - L * 0.66, -L * 0.6]];
      const w = Math.max(1.2, r * 0.24);
      return [
        { kind: 'line', points: pts, stroke: OUTLINE, width: w + 1.2, part: 'tail' },
        { kind: 'line', points: pts, stroke: p.dark, width: w, part: 'tail' },
      ];
    }
  }
}

/** The shapes of an action icon, centered on (0, 0), about 8 px across. */
export function hintShapes(h: Hint): Shape[] {
  switch (h) {
    case 'sleep':
      return [
        { kind: 'line', points: [[-4, -3], [0, -3], [-4, 1], [0, 1]], stroke: 0x0d1117, width: 3 },
        { kind: 'line', points: [[-4, -3], [0, -3], [-4, 1], [0, 1]], stroke: 0xa5c8ff, width: 1.4 },
        { kind: 'line', points: [[1, -6], [4, -6], [1, -3], [4, -3]], stroke: 0x0d1117, width: 2.6 },
        { kind: 'line', points: [[1, -6], [4, -6], [1, -3], [4, -3]], stroke: 0xa5c8ff, width: 1.1 },
      ];
    case 'flee':
      return [
        { kind: 'ellipse', x: 0, y: 0, rx: 4.5, ry: 4.5, fill: 0xf0b72f, stroke: 0x0d1117, width: 1 },
        { kind: 'line', points: [[0, -2.6], [0, 0.8]], stroke: 0x0d1117, width: 1.5 },
        { kind: 'ellipse', x: 0, y: 2.3, rx: 0.8, ry: 0.8, fill: 0x0d1117 },
      ];
    case 'hunt':
      return [
        { kind: 'ellipse', x: 0, y: 0, rx: 4, ry: 4, stroke: 0x0d1117, width: 3 },
        { kind: 'ellipse', x: 0, y: 0, rx: 4, ry: 4, stroke: 0xff5a4f, width: 1.4 },
        { kind: 'line', points: [[-5.5, 0], [-1.5, 0]], stroke: 0xff5a4f, width: 1.4 },
        { kind: 'line', points: [[1.5, 0], [5.5, 0]], stroke: 0xff5a4f, width: 1.4 },
        { kind: 'line', points: [[0, -5.5], [0, -1.5]], stroke: 0xff5a4f, width: 1.4 },
        { kind: 'line', points: [[0, 1.5], [0, 5.5]], stroke: 0xff5a4f, width: 1.4 },
      ];
    case 'mate':
      return [
        { kind: 'poly', points: [[0, 4], [-4.2, -0.2], [-4, -3], [-2, -4.2], [0, -2.6], [2, -4.2], [4, -3], [4.2, -0.2]],
          fill: 0xff7eb6, stroke: 0x0d1117, width: 1 },
      ];
  }
}
