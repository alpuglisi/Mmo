// Creature sprites: what a map marker draws for an organism's visible body
// plan (the server's `look`), and the cache key of that drawing. Pure (no
// PixiJS) so it is unit-tested. The drawing itself comes from the creature
// art library (`spriteArt.ts`: the look as an avatar model, drawn at the
// sprite level of detail, art/STYLE.md), so map creatures are the animals
// of the founder avatar and the gallery; `sprites.ts` rasterises each
// distinct drawing once into a cached texture.
//
// Cues, so a glance tells body plans apart at marker size:
// - size class → body radius; juveniles are smaller with a bigger head;
// - diet → the muzzle (deep and round on herbivores, square with a brow on
//   carnivores), horns on large herbivores, tusks or fangs;
// - endothermy → warm-blooded bodies round and upright, cold-blooded ones
//   long and low with sprawled legs and a wedge head;
// - cover → fur (soft clumped rim), feathers (fur with wings), scales,
//   armor plates, or bare skin;
// - coloring → drab, camouflage blotches, a bright crest and spots, or
//   yellow and ink-black warning bands;
// - locomotion → a dorsal fin and webbed fins (swimmers), gliding flaps or
//   wings.
//
// Morphology (docs/morphology-contract.md, quantized by the server): legs
// (0, 2, 4, 6, 8), ears (count, shape, size), eyes (count, size and
// placement: forward-set eyes show the far eye past the snout, side-set
// sit high and back), tail (shape and length), snout length and fangs.
// Absent fields (older servers) take the contract defaults (`morphOf`).
// - detail → markers drawn smaller than `NEAR_SCALE` (zoomed out) use the
//   class sprite (`far`: morphology at its defaults), so few textures serve
//   a crowded wide view.

import type { Appearance, EarShape, NearbyOrganism, OwnOrganism, SizeBucket, TailLook } from '../protocol';

export type Point = [number, number];

/** A shape of an overlay icon (`hintShapes`), centred on the marker, in screen pixels. */
export type Shape =
  | { kind: 'ellipse'; x: number; y: number; rx: number; ry: number; fill?: number; alpha?: number; stroke?: number; width?: number }
  | { kind: 'poly'; points: Point[]; fill?: number; alpha?: number; stroke?: number; width?: number }
  | { kind: 'line'; points: Point[]; stroke: number; width: number; alpha?: number };

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
 * on carnivores only. Values out of range are
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
 * length, a swimmer's thin tail, which is a tail fin), so looks that draw the
 * same share a texture.
 */
export function visibleMorph(l: Appearance, far = false): Morph {
  const m = morphOf(far ? classOf(l) : l);
  const swim = l.locomotion === 'swim';
  if (swim) m.legs = 0;
  if (swim && m.tail === 'thin') m.tail = 'fin';
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

/** Half the side of the square a creature (with wings, tail and icon room) fits in: the overlay textures' frame. */
export function spriteExtent(s: CreatureSpec): number {
  return Math.ceil(bodyRadius(s) * 3.2) + 2;
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

/** The colours of a colouring class; the art library takes its hue from here (`art/palette.ts` `creaturePalette`). */
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
