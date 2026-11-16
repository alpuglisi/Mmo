// World sprites from the creature art library (art/STYLE.md, "Sprites"):
// an organism's quantized look (`creature.ts` `visibleMorph`, the cache key)
// becomes the founder avatar's model (`avatar.ts` `AvatarModel`), drawn at
// the sprite level of detail, so a creature on the map is the animal of its
// avatar and the gallery, in the same style.
//
//   CreatureSpec ──spriteModel──▶ AvatarModel ──avatarDrawing(lod sprite)──▶ SpriteDrawing
//
// Pure (no DOM, no PixiJS) so it is unit-tested; `sprites.ts` rasterises a
// drawing once per key with the canvas renderer (`art/canvas.ts`).
//
// The model depends on nothing but what the key holds (the class fields,
// the visible morphology and the life stage), so one key is one drawing.
// The buckets map to gene values well inside each bucket, a little past the
// middle where that helps a feature read at marker size (eyes, ears, fangs).
// Sprite-only touches: swimmers grow a dorsal fin (their legs are fins).

import type { Appearance, SizeBucket } from '../protocol';
import { drawingBounds } from '../art/render';
import type { Piece, Point } from '../art/geom';
import { avatarDrawing, type AvatarDrawing, type AvatarModel } from './avatar';
import { bodyRadius, palette, visibleMorph, type CreatureSpec, type Morph } from './creature';

/** Gene value of each size bucket, per feature. */
const EYE_SIZE = [0.25, 0.6, 1] as const;
/** Ear sizes; cold-blooded heads draw an ear opening below 0.5, so their buckets start there (ears that show). */
const EAR_SIZE = { warm: [0.15, 0.5, 0.9], cold: [0.5, 0.72, 0.95] } as const;
const TAIL_LENGTH = [0.25, 0.5, 0.85] as const;
const SNOUT = [0.15, 0.5, 0.9] as const;
const FANG = [0, 0.55, 0.95] as const;
/** Eye placement bucket → the pair angle: forward-set (both eyes show), the default, side-set. */
export const EYE_AZIMUTH = [20, 60, 110] as const;
/** Body size gene per size class (heft follows it; heads shrink with it, so it stays low: big heads read at marker size). */
const SIZE_GENE = [0.1, 0.3, 0.5, 0.75] as const;
const CARNIVORY = { herbivore: 0.15, omnivore: 0.5, carnivore: 0.85 } as const;

/**
 * Pixels per creature unit at marker scale 1, as a fraction of the body
 * radius: a warm body (50 units long each side of its centre) is about as
 * long as the old sprite's (1.15 radii each way), a little more.
 */
export const UNIT_PER_RADIUS = 1 / 40;
/** The ground line sits this many body radii below the marker centre: on the owner ring (`sprites.ts`). */
export const GROUND_PER_RADIUS = 0.95;
/** Outline width on screen (px at marker scale 1): bolder than the portrait's, as STYLE.md asks of small sprites. */
export const SPRITE_OUTLINE_PX = 1.15;

/** The drawing of one sprite texture, with how it sits on the marker. */
export interface SpriteDrawing {
  model: AvatarModel;
  drawing: AvatarDrawing;
  /** Pixels (at marker scale 1) per creature unit. */
  k: number;
  /** The ground line (y = 0), in px below the marker centre. */
  ground: number;
  /** Outline width and rim widths, in creature units. */
  outline: number;
  rim: { light: number; shade: number };
  /** Half the texture's width and height (px at marker scale 1), centred on the marker so a flip mirrors in place. */
  half: { w: number; h: number };
}

const pick = <T>(a: readonly T[], b: SizeBucket): T => a[b];

/** The avatar model a world sprite draws for a spec (only the key's fields go in). */
export function spriteModel(s: CreatureSpec): AvatarModel {
  const l = s.look;
  const m: Morph = visibleMorph(l, s.far);
  const look: Appearance = { size: l.size, diet: l.diet, warm: l.warm, locomotion: l.locomotion, cover: l.cover, coloring: l.coloring };
  const size = SIZE_GENE[Math.min(3, Math.max(0, Math.round(l.size)))] * (s.juvenile ? 0.45 : 1);
  const carn = CARNIVORY[l.diet] ?? 0.15;
  const swim = l.locomotion === 'swim';
  const wings = l.locomotion === 'fly' ? 0.85 : l.locomotion === 'glide' ? 0.4 : 0;
  const amount = 0.8;
  const layer = (c: Appearance['cover']) => ({ amount: l.cover === c ? amount : 0, role: l.cover === c ? 'dominant' as const : 'none' as const });
  const big = l.size >= 2 && !s.juvenile;
  return {
    eye_count: m.eyes,
    eye_azimuth: EYE_AZIMUTH[m.eyeSet],
    eye_size: pick(EYE_SIZE, m.eyeSize),
    ear_count: m.ears as 0 | 2 | 4,
    ear_shape: m.earShape,
    ear_size: pick(l.warm ? EAR_SIZE.warm : EAR_SIZE.cold, m.earSize),
    leg_count: m.legs as 0 | 2 | 4 | 6 | 8,
    // Shortish legs: a compact silhouette at marker size.
    leg_length: l.warm ? 0.35 : 0.3,
    tail_present: m.tail !== 'none',
    tail_shape: m.tail === 'none' ? 'thin' : m.tail,
    tail_length: m.tail === 'none' ? 0 : pick(TAIL_LENGTH, m.tailSize),
    snout_length: pick(SNOUT, m.snout),
    fang_size: pick(FANG, m.fangs),
    look,
    warm: l.warm,
    size,
    mass: [0.5, 4, 20, 80][l.size] ?? 4,
    body: { muscle: l.diet === 'carnivore' ? 0.5 : 0.2, fat: l.diet === 'herbivore' ? 0.35 : 0.15 },
    feet: {
      kind: l.diet === 'carnivore' ? 'claw' : l.diet === 'herbivore' && big ? 'hoof' : 'paw',
      width: swim ? 0.7 : 0.4,
      webbed: swim,
      claws: l.diet === 'carnivore' ? 0.55 : 0,
      climbing: false,
      digging: false,
    },
    wings: { stage: wings > 0.5 ? 'wings' : wings > 0.25 ? 'flaps' : 'none', amount: wings },
    cover: { fur: layer('fur'), scales: layer('scales'), armor: layer('armor'), dominant: l.cover },
    carnivory: carn,
    nose: 0.3,
    // Horns on large adult herbivores (as the old sprite and the avatar).
    horns: l.diet === 'herbivore' && big ? 0.35 + 0.25 * (l.size - 2) : 0,
    coloring: {
      kind: l.coloring,
      camo: l.coloring === 'camo' ? 0.8 : 0,
      display: l.coloring === 'bright' ? 0.85 : 0,
      toxins: l.coloring === 'warning' ? 0.8 : 0,
      palette: palette(look),
    },
    extras: { birth: 'live', seasonalCoat: false, swim: swim ? 0.8 : 0 },
  };
}

/** A swimmer's dorsal fin, grown from the back into the main silhouette. */
function dorsalFin(d: AvatarDrawing): void {
  const main = d.groups.find((g) => g.part === 'body');
  if (!main) return;
  const { cy, rx, ry } = d.anchors.body;
  const pts: Point[] = [[-rx * 0.42, cy - ry * 0.7], [-rx * 0.28, cy - ry * 1.05], [-rx * 0.02, cy - ry * 1.62], [rx * 0.1, cy - ry * 1.5], [rx * 0.22, cy - ry * 0.75]];
  const fin: Piece = { tag: 'fin', points: pts, smooth: true };
  main.pieces.splice(1, 0, fin);
  // A ray or two in the shade tone.
  main.detail.push({ part: 'body', tag: 'fin', kind: 'line', points: [[-rx * 0.15, cy - ry * 0.85], [-rx * 0.04, cy - ry * 1.4]], stroke: main.tones.shade, width: 2.2, alpha: 0.7 });
}

/** The drawing of a creature's sprite texture, sized for a marker at scale 1. */
export function spriteDrawing(s: CreatureSpec): SpriteDrawing {
  const model = spriteModel(s);
  const drawing = avatarDrawing(model, { lod: 'sprite' });
  if (s.look.locomotion === 'swim') dorsalFin(drawing);
  const r = bodyRadius(s);
  const k = r * UNIT_PER_RADIUS;
  const ground = r * GROUND_PER_RADIUS;
  const outline = SPRITE_OUTLINE_PX / k;
  // Rims in px: one highlight and one shade step that still read at 10-30 px.
  const rim = { light: Math.max(0.7, r * 0.13) / k, shade: Math.max(1.4, r * 0.3) / k };
  const b = drawingBounds(drawing);
  const pad = SPRITE_OUTLINE_PX + 1;
  const half = {
    w: Math.ceil(Math.max(-b.minX, b.maxX) * k + pad),
    h: Math.ceil(Math.max(-(b.minY * k + ground), b.maxY * k + ground) + pad),
  };
  return { model, drawing, k, ground, outline, rim, half };
}
