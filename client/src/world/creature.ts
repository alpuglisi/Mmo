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

import type { Appearance, NearbyOrganism, OwnOrganism } from '../protocol';

export type Point = [number, number];

export type Shape =
  | { kind: 'ellipse'; x: number; y: number; rx: number; ry: number; fill?: number; alpha?: number; stroke?: number; width?: number }
  | { kind: 'poly'; points: Point[]; fill?: number; alpha?: number; stroke?: number; width?: number }
  | { kind: 'line'; points: Point[]; stroke: number; width: number; alpha?: number };

/** What is drawn for a creature: its appearance class and life stage. */
export interface CreatureSpec {
  look: Appearance;
  juvenile: boolean;
}

/** Small icon over a marker for what the organism is doing. */
export type Hint = 'sleep' | 'flee' | 'hunt' | 'mate';

/** For organisms from a server that doesn't send `look`. */
export const DEFAULT_LOOK: Appearance = { size: 1, diet: 'herbivore', warm: true, locomotion: 'walk', cover: 'fur', coloring: 'drab' };

const OUTLINE = 0x15181c;
const SIZE_RADIUS = [4, 5.5, 7, 9] as const;
/** Juveniles are drawn at this fraction of the adult size. */
export const JUVENILE_SCALE = 0.65;

export function specOf(o: OwnOrganism | NearbyOrganism): CreatureSpec {
  return { look: o.look ?? DEFAULT_LOOK, juvenile: !o.matured };
}

/** Cache key of a creature texture: one per distinct appearance class and stage. */
export function creatureKey(s: CreatureSpec): string {
  const l = s.look;
  // One letter per field value (skin and scales differ in their second letter).
  const cover = l.cover === 'scales' ? 'c' : l.cover[0];
  return `${l.size}${l.diet[0]}${l.warm ? 'w' : 'c'}${l.locomotion[0]}${cover}${l.coloring[0]}${s.juvenile ? 'j' : 'a'}`;
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

/** The shapes of a creature, back to front, facing +x. */
export function creatureShapes(s: CreatureSpec): Shape[] {
  const l = s.look;
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
    for (const fx of [-0.6, -0.25, 0.3, 0.6]) {
      const x = rx * fx;
      out.push({ kind: 'line', points: [[x, ry * 0.4], [x + (l.warm ? 0 : rx * 0.12), ry * 0.4 + legLen]], stroke: OUTLINE, width: Math.max(1.2, r * 0.22) });
    }
  } else {
    out.push({ kind: 'poly', points: [[-rx * 0.1, ry * 0.5], [-rx * 0.5, ry * 1.35], [rx * 0.25, ry * 0.7]], fill: p.dark, stroke: OUTLINE, width: 0.8 });
    // Dorsal fin.
    out.push({ kind: 'poly', points: [[-rx * 0.35, -ry * 0.85], [-rx * 0.05, -ry * 1.75], [rx * 0.3, -ry * 0.85]], fill: p.dark, stroke: OUTLINE, width: 0.8 });
  }

  // Tail.
  if (swim) {
    out.push({ kind: 'poly', points: [[-rx * 0.9, 0], [-rx * 1.55, -ry * 0.95], [-rx * 1.35, 0], [-rx * 1.55, ry * 0.95]],
      fill: p.dark, stroke: OUTLINE, width: 0.8 });
  } else if (l.warm) {
    out.push({ kind: 'poly', points: [[-rx * 0.85, -ry * 0.35], [-rx * 1.35, -ry * 0.75], [-rx * 0.9, ry * 0.15]], fill: p.dark, stroke: OUTLINE, width: 0.8 });
  } else {
    out.push({ kind: 'poly', points: [[-rx * 0.8, -ry * 0.4], [-rx * 2.0, ry * 0.15], [-rx * 0.8, ry * 0.45]], fill: p.base, stroke: OUTLINE, width: outlineW });
  }

  // Fur: a ragged fringe just outside the body.
  if (l.cover === 'fur') {
    const n = 14;
    const pts: Point[] = [];
    for (let i = 0; i < n * 2; i++) {
      const a = (i / (n * 2)) * Math.PI * 2;
      const k = i % 2 === 0 ? 1.18 : 1.0;
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

  // Ears (warm-blooded land animals).
  if (l.warm && !swim) {
    out.push({ kind: 'poly', points: [[hx - headR * 0.6, hy - headR * 0.6], [hx - headR * 0.35, hy - headR * 1.55], [hx + headR * 0.05, hy - headR * 0.75]],
      fill: p.dark, stroke: OUTLINE, width: 0.7 });
  }
  // Horns on large herbivores.
  if (l.diet === 'herbivore' && l.size >= 2 && !s.juvenile) {
    out.push({ kind: 'line', points: [[hx + headR * 0.1, hy - headR * 0.8], [hx + headR * 0.2, hy - headR * 1.7], [hx + headR * 0.7, hy - headR * 1.9]], stroke: 0xe8e0c8, width: 1.4 });
  }

  // Head and snout.
  out.push({ kind: 'ellipse', x: hx, y: hy, rx: headR, ry: headR, fill: p.base, stroke: OUTLINE, width: outlineW });
  if (l.diet === 'herbivore') {
    out.push({ kind: 'ellipse', x: hx + headR * 0.85, y: hy + headR * 0.3, rx: headR * 0.5, ry: headR * 0.42, fill: p.light, stroke: OUTLINE, width: 0.7 });
  } else if (l.diet === 'omnivore') {
    out.push({ kind: 'poly', points: [[hx + headR * 0.6, hy - headR * 0.45], [hx + headR * 1.6, hy + headR * 0.2], [hx + headR * 0.6, hy + headR * 0.6]],
      fill: p.base, stroke: OUTLINE, width: 0.8 });
  } else {
    out.push({ kind: 'poly', points: [[hx + headR * 0.5, hy - headR * 0.55], [hx + headR * 2.1, hy + headR * 0.15], [hx + headR * 0.5, hy + headR * 0.65]],
      fill: p.dark, stroke: OUTLINE, width: 0.9 });
    // Fangs.
    for (const fx of [1.0, 1.45]) {
      out.push({ kind: 'poly', points: [[hx + headR * fx, hy + headR * 0.35], [hx + headR * (fx + 0.18), hy + headR * 0.95], [hx + headR * (fx + 0.32), hy + headR * 0.3]], fill: 0xffffff });
    }
  }
  // Eye (with a brow on carnivores).
  const ex = hx + headR * 0.25;
  const ey = hy - headR * 0.2;
  out.push({ kind: 'ellipse', x: ex, y: ey, rx: headR * 0.32, ry: headR * 0.32, fill: 0xffffff });
  out.push({ kind: 'ellipse', x: ex + headR * 0.08, y: ey, rx: headR * 0.17, ry: headR * 0.17, fill: OUTLINE });
  if (l.diet === 'carnivore') out.push({ kind: 'line', points: [[ex - headR * 0.35, ey - headR * 0.5], [ex + headR * 0.4, ey - headR * 0.25]], stroke: OUTLINE, width: 1.1 });
  return out;
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
