/** Helpers shared by the creature tests (split over several files so vitest runs the exhaustive ones in parallel). */
import type { Appearance } from '../protocol';
import { DEFAULT_LOOK, EAR_SHAPES, TAIL_LOOKS, type CreatureSpec } from './creature';
import { spriteDrawing, type SpriteDrawing } from './spriteArt';
import { drawingShapes } from './avatar';
import { drawingBounds } from '../art/render';
import type { Shape as ArtShape } from '../art/geom';

/** Time for the tests that draw every class (thousands of drawings): more than the default 5 s, for a loaded CI machine. */
export const EXHAUSTIVE_MS = 30_000;

export const look = (over: Partial<Appearance> = {}): Appearance => ({ ...DEFAULT_LOOK, ...over });
export const adult = (over: Partial<Appearance> = {}): CreatureSpec => ({ look: look(over), juvenile: false });

/** Every value of every appearance field, for exhaustive checks. */
export function allLooks(): Appearance[] {
  const out: Appearance[] = [];
  for (const size of [0, 1, 2, 3] as const)
    for (const diet of ['herbivore', 'omnivore', 'carnivore'] as const)
      for (const warm of [false, true])
        for (const locomotion of ['walk', 'swim', 'glide', 'fly'] as const)
          for (const cover of ['skin', 'fur', 'scales', 'armor'] as const)
            for (const coloring of ['drab', 'camo', 'bright', 'warning'] as const) out.push({ size, diet, warm, locomotion, cover, coloring });
  return out;
}

export const draw = (s: CreatureSpec): SpriteDrawing => spriteDrawing(s);
/** What a texture shows: the drawing and how it sits on the marker (not the model it came from). */
export const drawn = (s: CreatureSpec): string => {
  const { model: _model, ...rest } = draw(s);
  return JSON.stringify(rest);
};
/** Silhouette pieces with a tag (`leg` matches `leg-0`, `ear` matches `ear-far-1`). */
export const pieces = (s: CreatureSpec, tag: string) => draw(s).drawing.groups.flatMap((g) => g.pieces.filter((p) => p.tag === tag || p.tag.startsWith(`${tag}-`)));
export const tagged = (s: CreatureSpec, tag: string) => drawingShapes(draw(s).drawing).filter((x) => x.tag === tag);
export type Ellipse = Extract<ArtShape, { kind: 'ellipse' }>;
/** Eyes, found by their glint (back to front: a forward pair's far eye first). */
export const glints = (s: CreatureSpec) => tagged(s, 'glint') as Ellipse[];
export const differs = (a: Partial<Appearance>, b: Partial<Appearance>, body: Partial<Appearance> = {}) => drawn(adult({ ...body, ...a })) !== drawn(adult({ ...body, ...b }));

/** Texture pixels the drawing covers, relative to the marker centre (px at marker scale 1). */
export function pixelBounds(sd: SpriteDrawing): { minX: number; maxX: number; minY: number; maxY: number } {
  const b = drawingBounds(sd.drawing);
  return { minX: b.minX * sd.k, maxX: b.maxX * sd.k, minY: b.minY * sd.k + sd.ground, maxY: b.maxY * sd.k + sd.ground };
}

/** Looks covering every morphology value on a spread of body plans. */
export function morphLooks(): Appearance[] {
  const bodies: Partial<Appearance>[] = [
    {}, { warm: false, cover: 'scales', diet: 'carnivore' }, { locomotion: 'swim', warm: false }, { locomotion: 'fly', size: 0 }, { size: 3, diet: 'omnivore', cover: 'armor' },
  ];
  const morphs: Partial<Appearance>[] = [];
  for (const eyes of [1, 2, 3, 4, 5, 6]) for (const eye_size of [0, 1, 2] as const) for (const eye_azimuth of [0, 1, 2] as const) morphs.push({ eyes, eye_size, eye_azimuth });
  for (const ears of [0, 2, 4]) for (const ear_shape of EAR_SHAPES) for (const ear_size of [0, 1, 2] as const) morphs.push({ ears, ear_shape, ear_size });
  for (const legs of [0, 2, 4, 6, 8]) morphs.push({ legs });
  for (const tail of TAIL_LOOKS) for (const tail_size of [0, 1, 2] as const) morphs.push({ tail, tail_size });
  for (const snout of [0, 1, 2] as const) for (const fangs of [0, 1, 2] as const) morphs.push({ snout, fangs });
  const out: Appearance[] = [];
  for (const size of [0, 1, 2, 3] as const) for (const b of bodies) for (const m of morphs) out.push(look({ size, ...b, ...m }));
  return out;
}
