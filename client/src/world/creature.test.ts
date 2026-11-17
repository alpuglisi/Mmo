import { createHash } from 'node:crypto';
import { describe, expect, it } from 'vitest';
import type { Appearance, NearbyOrganism, OwnOrganism } from '../protocol';
import {
  actionHint, bodyRadius, describeLook, creatureKey, DEFAULT_LOOK, EAR_SHAPES, hintShapes, JUVENILE_SCALE, morphOf, palette, specOf,
  spriteExtent, TAIL_LOOKS, visibleMorph, type CreatureSpec,
} from './creature';
import { EYE_AZIMUTH, FAR_PARTS, FAR_SHIFT_PX, shiftFar, SPRITE_OUTLINE_PX, spriteDrawing, spriteModel, type SpriteDrawing } from './spriteArt';
import { avatarDrawing, drawingShapes, paintOf } from './avatar';
import { drawingBounds } from '../art/render';
import type { Shape as ArtShape } from '../art/geom';

/** Time for the tests that draw every class (thousands of drawings): more than the default 5 s, for a loaded CI machine. */
const EXHAUSTIVE_MS = 30_000;

const look = (over: Partial<Appearance> = {}): Appearance => ({ ...DEFAULT_LOOK, ...over });
const adult = (over: Partial<Appearance> = {}): CreatureSpec => ({ look: look(over), juvenile: false });

/** Every value of every appearance field, for exhaustive checks. */
function allLooks(): Appearance[] {
  const out: Appearance[] = [];
  for (const size of [0, 1, 2, 3] as const)
    for (const diet of ['herbivore', 'omnivore', 'carnivore'] as const)
      for (const warm of [false, true])
        for (const locomotion of ['walk', 'swim', 'glide', 'fly'] as const)
          for (const cover of ['skin', 'fur', 'scales', 'armor'] as const)
            for (const coloring of ['drab', 'camo', 'bright', 'warning'] as const) out.push({ size, diet, warm, locomotion, cover, coloring });
  return out;
}

const draw = (s: CreatureSpec): SpriteDrawing => spriteDrawing(s);
/** What a texture shows: the drawing and how it sits on the marker (not the model it came from). */
const drawn = (s: CreatureSpec): string => {
  const { model: _model, ...rest } = draw(s);
  return JSON.stringify(rest);
};
/** Silhouette pieces with a tag (`leg` matches `leg-0`, `ear` matches `ear-far-1`). */
const pieces = (s: CreatureSpec, tag: string) => draw(s).drawing.groups.flatMap((g) => g.pieces.filter((p) => p.tag === tag || p.tag.startsWith(`${tag}-`)));
const tagged = (s: CreatureSpec, tag: string) => drawingShapes(draw(s).drawing).filter((x) => x.tag === tag);
type Ellipse = Extract<ArtShape, { kind: 'ellipse' }>;
/** Eyes, found by their glint (back to front: a forward pair's far eye first). */
const glints = (s: CreatureSpec) => tagged(s, 'glint') as Ellipse[];
const differs = (a: Partial<Appearance>, b: Partial<Appearance>, body: Partial<Appearance> = {}) => drawn(adult({ ...body, ...a })) !== drawn(adult({ ...body, ...b }));

/** Texture pixels the drawing covers, relative to the marker centre (px at marker scale 1). */
function pixelBounds(sd: SpriteDrawing): { minX: number; maxX: number; minY: number; maxY: number } {
  const b = drawingBounds(sd.drawing);
  return { minX: b.minX * sd.k, maxX: b.maxX * sd.k, minY: b.minY * sd.k + sd.ground, maxY: b.maxY * sd.k + sd.ground };
}

describe('appearance classes', () => {
  it('gives every distinct appearance and life stage its own cache key', () => {
    const keys = new Set<string>();
    const looks = allLooks();
    for (const l of looks) for (const juvenile of [false, true]) keys.add(creatureKey({ look: l, juvenile }));
    expect(keys.size).toBe(looks.length * 2);
  });

  it('draws every class with the art library: the avatar of its sprite model, at sprite detail', () => {
    for (const l of allLooks()) {
      for (const juvenile of [false, true]) {
        const s = { look: l, juvenile };
        const sd = draw(s);
        const lib = avatarDrawing(spriteModel(s), { lod: 'sprite' });
        // Swimmers alone grow a dorsal fin into the main silhouette.
        const fins = sd.drawing.groups.flatMap((g) => g.pieces.filter((p) => p.tag === 'fin'));
        expect(fins.length, creatureKey(s)).toBe(l.locomotion === 'swim' ? 1 : 0);
        const strip = (d: typeof lib) => d.groups.map((g) => ({ ...g, pieces: g.pieces.filter((p) => p.tag !== 'fin'), detail: g.detail.filter((x) => x.tag !== 'fin') }));
        // The far-side groups sit FAR_SHIFT_PX forward and up (clear of the near parts' outline); the rest is the library's.
        shiftFar(lib, FAR_SHIFT_PX.x / sd.k, FAR_SHIFT_PX.y / sd.k);
        expect(JSON.stringify(strip(sd.drawing)) === JSON.stringify(strip(lib)), creatureKey(s)).toBe(true);
        for (const g of sd.drawing.groups) if (FAR_PARTS.has(g.part)) expect(g.tones, creatureKey(s)).toEqual(paintOf(sd.model).far);
        // Sprite detail: no fine textures (scales come as a coarse pattern).
        const tags = new Set(drawingShapes(sd.drawing).map((x) => x.tag));
        for (const t of ['hair', 'feather', 'crease', 'whisker', 'scute']) expect(tags.has(t), `${creatureKey(s)} ${t}`).toBe(false);
      }
    }
  }, EXHAUSTIVE_MS);

  it('sets the far-side legs, ears and wing back in the far ramp, a pixel forward and up, clear of the near outline', () => {
    for (const juvenile of [false, true]) for (const size of [0, 3] as const) {
      const s: CreatureSpec = { look: look({ size, locomotion: 'fly', cover: 'skin' }), juvenile };
      const sd = draw(s);
      const lib = avatarDrawing(spriteModel(s), { lod: 'sprite' });
      const pal = paintOf(sd.model);
      const parts = new Set<string>();
      sd.drawing.groups.forEach((g, i) => {
        const L = lib.groups[i];
        expect(g.part).toBe(L.part);
        const [a, b] = [g.pieces[0].points[0], L.pieces[0].points[0]];
        if (FAR_PARTS.has(g.part)) {
          parts.add(g.part);
          expect(g.tones).toEqual(pal.far);
          // In screen px at marker scale 1: about the outline width forward, half that up, whatever the size.
          expect((a[0] - b[0]) * sd.k).toBeCloseTo(FAR_SHIFT_PX.x, 6);
          expect((a[1] - b[1]) * sd.k).toBeCloseTo(FAR_SHIFT_PX.y, 6);
          expect(FAR_SHIFT_PX.x).toBeGreaterThanOrEqual(SPRITE_OUTLINE_PX * 0.75);
        } else {
          expect(a).toEqual(b);
        }
      });
      expect([...parts].sort()).toEqual(['ears-far', 'legs-far', 'wing-far']);
      // The library's own points are left alone (they may be its anchors).
      const before = JSON.stringify(lib);
      shiftFar({ ...lib }, 5, -5);
      expect(JSON.stringify(lib)).toBe(before);
    }
  });

  it('draws the same without the morphology fields, with the defaults as a server sends them, and zoomed out', () => {
    const hash = (far: boolean, fields: (l: Appearance) => Partial<Appearance>) => {
      const h = createHash('sha256');
      for (const l of allLooks()) for (const juvenile of [false, true]) h.update(drawn({ look: { ...l, ...fields(l) }, juvenile, far }));
      return h.digest('hex');
    };
    const absent = hash(false, () => ({}));
    const defaults = (l: Appearance): Partial<Appearance> => ({
      eyes: 2, eye_azimuth: 1, eye_size: 1, ears: 2, ear_shape: 'round', ear_size: 1, legs: 4, tail: 'thin', tail_size: 1, snout: 1, fangs: l.diet === 'carnivore' ? 1 : 0,
    });
    expect(hash(false, defaults)).toBe(absent);
    // Far detail ignores the morphology: the class sprite.
    expect(hash(true, () => ({ eyes: 6, eye_azimuth: 0, ears: 4, ear_shape: 'long', legs: 8, tail: 'club', tail_size: 2, snout: 2, fangs: 2 }))).toBe(absent);
  }, EXHAUSTIVE_MS);

  it('maps every class and stage to the same sprite model (golden)', () => {
    // sha256 of the sprite models of all 1,536 classes × stage: what each
    // look means to the art library (sizes, coverings, feet, wings, horns,
    // colours). Changing it changes every map creature: update it only on
    // purpose, after looking at the e2e sprite galleries.
    const h = createHash('sha256');
    const replacer = (_k: string, v: unknown) => (typeof v === 'number' ? Math.round(v * 1e6) / 1e6 : v);
    for (const l of allLooks()) for (const juvenile of [false, true]) h.update(JSON.stringify(spriteModel({ look: l, juvenile }), replacer));
    expect(h.digest('hex')).toBe('e9b6c9e75ba55ed88ca07d0967ec3c91e1a02b84462f39814823d88bbdcfe719');
    // The default founder (a server without morphology), in full.
    const { look: _look, ...m } = spriteModel(adult());
    expect(m).toMatchInlineSnapshot(`
      {
        "body": {
          "fat": 0.35,
          "muscle": 0.2,
        },
        "carnivory": 0.15,
        "coloring": {
          "camo": 0,
          "display": 0,
          "kind": "drab",
          "palette": {
            "accent": 8215356,
            "base": 10779218,
            "dark": 7229235,
            "light": 13808780,
          },
          "toxins": 0,
        },
        "cover": {
          "armor": {
            "amount": 0,
            "role": "none",
          },
          "dominant": "fur",
          "fur": {
            "amount": 0.8,
            "role": "dominant",
          },
          "scales": {
            "amount": 0,
            "role": "none",
          },
        },
        "ear_count": 2,
        "ear_shape": "round",
        "ear_size": 0.5,
        "extras": {
          "birth": "live",
          "seasonalCoat": false,
          "swim": 0,
        },
        "eye_azimuth": 60,
        "eye_count": 2,
        "eye_size": 0.6,
        "fang_size": 0,
        "feet": {
          "claws": 0,
          "climbing": false,
          "digging": false,
          "kind": "paw",
          "webbed": false,
          "width": 0.4,
        },
        "horn_form": "curved",
        "horn_size": 0,
        "horns": 0,
        "leg_count": 4,
        "leg_length": 0.35,
        "mass": 4,
        "nose": 0.3,
        "pattern": "plain",
        "size": 0.3,
        "snout_length": 0.5,
        "tail_length": 0.5,
        "tail_present": true,
        "tail_shape": "thin",
        "warm": true,
        "wing_aspect": 0.5,
        "wings": {
          "amount": 0,
          "stage": "none",
        },
      }
    `);
  });

  it('reads the look from an organism, with a default for older servers and juveniles from `matured`', () => {
    const own = { id: 1, matured: false, look: look({ diet: 'carnivore' }) } as unknown as OwnOrganism;
    expect(specOf(own)).toEqual({ look: look({ diet: 'carnivore' }), juvenile: true });
    const old = { id: 2, owner: 0, matured: true } as unknown as NearbyOrganism;
    expect(specOf(old)).toEqual({ look: DEFAULT_LOOK, juvenile: false });
  });

  it('sizes bodies by size class and draws juveniles smaller, with bigger heads', () => {
    const r = [0, 1, 2, 3].map((size) => bodyRadius(adult({ size: size as Appearance['size'] })));
    for (let i = 1; i < r.length; i++) expect(r[i]).toBeGreaterThan(r[i - 1]);
    const j = bodyRadius({ look: look({ size: 2 }), juvenile: true });
    expect(j).toBeCloseTo(r[2] * JUVENILE_SCALE);
    // On screen: wider sprites for bigger classes, juveniles narrower than adults.
    const width = (s: CreatureSpec) => {
      const b = pixelBounds(draw(s));
      return b.maxX - b.minX;
    };
    for (let i = 1; i < 4; i++) expect(width(adult({ size: i as Appearance['size'] }))).toBeGreaterThan(width(adult({ size: (i - 1) as Appearance['size'] })));
    expect(width({ look: look({ size: 2 }), juvenile: true })).toBeLessThan(width(adult({ size: 2 })));
    // The head, relative to the body: bigger on the juvenile.
    const headToBody = (s: CreatureSpec) => {
      const d = draw(s).drawing;
      const head = d.groups.flatMap((g) => g.pieces).find((p) => p.tag === 'head')!;
      const xs = head.points.map(([x]) => x);
      return (Math.max(...xs) - Math.min(...xs)) / d.anchors.body.rx;
    };
    expect(headToBody({ look: look({ size: 2 }), juvenile: true })).toBeGreaterThan(headToBody(adult({ size: 2 })));
  });

  it('keeps every drawing inside its texture, with its feet on the owner ring', () => {
    for (const l of allLooks()) {
      for (const juvenile of [false, true]) {
        const s = { look: l, juvenile };
        const sd = draw(s);
        const b = pixelBounds(sd);
        expect(Math.max(-b.minX, b.maxX), creatureKey(s)).toBeLessThanOrEqual(sd.half.w);
        expect(Math.max(-b.minY, b.maxY), creatureKey(s)).toBeLessThanOrEqual(sd.half.h);
        // The ground line (the feet) on the owner ring, under the marker centre.
        expect(sd.ground).toBeCloseTo(bodyRadius(s) * 0.95);
        // Wings and long tails grow the frame a little at most.
        expect(Math.max(sd.half.w, sd.half.h), creatureKey(s)).toBeLessThanOrEqual(spriteExtent(s) * 1.6);
      }
    }
  }, EXHAUSTIVE_MS);

  it('draws visibly different body plans', () => {
    expect(differs({ diet: 'carnivore' }, {})).toBe(true);
    expect(differs({ warm: false }, {})).toBe(true);
    expect(differs({ locomotion: 'swim' }, {})).toBe(true);
    expect(differs({ locomotion: 'fly' }, {})).toBe(true);
    expect(differs({ cover: 'scales' }, {})).toBe(true);
    expect(differs({ coloring: 'warning' }, {})).toBe(true);
    // Fliers reach well above the body (the raised wings); swimmers have no legs but a fin.
    expect(pixelBounds(draw(adult({ locomotion: 'fly', cover: 'skin' }))).minY).toBeLessThan(pixelBounds(draw(adult({ cover: 'skin' }))).minY - 3);
    expect(pieces(adult(), 'leg')).toHaveLength(4);
    expect(pieces(adult({ locomotion: 'swim' }), 'leg')).toHaveLength(0);
    expect(pieces(adult({ locomotion: 'swim' }), 'fin')).toHaveLength(1);
    // Coverings and colourings as the art library draws them; feathers on furred fliers.
    expect(tagged(adult({ cover: 'armor' }), 'plate').length).toBeGreaterThan(0);
    expect(tagged(adult({ coloring: 'warning' }), 'band').length).toBeGreaterThan(0);
    expect(tagged(adult({ coloring: 'camo' }), 'camo').length).toBeGreaterThan(0);
    expect(pieces(adult({ coloring: 'bright' }), 'frill')).toHaveLength(1);
    expect(pieces(adult({ locomotion: 'fly' }), 'crest')).toHaveLength(1);
    expect(pieces(adult({ locomotion: 'glide' }), 'flaps')).toHaveLength(1);
    // Horns come from the look (a body-plan locus), on adults; no more horns for every large herbivore.
    expect(tagged(adult({ size: 3 }), 'horn')).toHaveLength(0);
    expect(tagged(adult({ horns: 2 }), 'horn').length).toBeGreaterThan(0);
    expect(tagged({ look: look({ horns: 2 }), juvenile: true }, 'horn')).toHaveLength(0);
    expect(differs({ horns: 2, horn_form: 'branched' }, { horns: 2 })).toBe(true);
    expect(differs({ horns: 2, horn_form: 'straight' }, { horns: 2 })).toBe(true);
    // Coat patterns draw markings; wing shape changes a flier's outline.
    for (const pattern of ['spots', 'stripes', 'mottled'] as const) expect(tagged(adult({ pattern }), 'pattern').length, pattern).toBeGreaterThan(0);
    expect(tagged(adult(), 'pattern')).toHaveLength(0);
    expect(differs({ locomotion: 'fly', wing_aspect: 2 }, { locomotion: 'fly' })).toBe(true);
  });

  it('colors by coloring class', () => {
    expect(palette(look({ coloring: 'warning' })).accent).toBe(0x1b1b1b);
    expect(palette(look({ coloring: 'bright' })).base).not.toBe(palette(look({ coloring: 'drab' })).base);
    expect(palette(look({ warm: false })).base).not.toBe(palette(look({ warm: true })).base);
    // The sprite's body tone follows the class palette.
    const body = (s: CreatureSpec) => draw(s).drawing.groups.find((g) => g.part === 'body')!.tones.base;
    expect(body(adult({ coloring: 'warning' }))).not.toBe(body(adult()));
  });

  it('draws one outline width on screen at every size, bolder than the portrait relative to the creature', () => {
    for (const size of [0, 1, 2, 3] as const) {
      const sd = draw(adult({ size }));
      expect(sd.outline * sd.k).toBeCloseTo(SPRITE_OUTLINE_PX);
      // The portrait's 1.9 px outline on a ~250 px creature; here over a 1.15 px on a 15-40 px one.
      const b = pixelBounds(sd);
      expect(sd.outline * sd.k / (b.maxX - b.minX)).toBeGreaterThan(1.9 / 250);
    }
  });
});

/** Looks covering every morphology value on a spread of body plans. */
function morphLooks(): Appearance[] {
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

describe('morphology', () => {
  it('fills defaults for older servers and clamps values from the wire', () => {
    expect(morphOf(DEFAULT_LOOK)).toEqual({
      eyes: 2, eyeSize: 1, eyeSet: 1, ears: 2, earShape: 'round', earSize: 1, legs: 4, tail: 'thin', tailSize: 1, snout: 1, fangs: 0,
      horns: 0, hornForm: 'curved', wingSet: 1, pattern: 'plain',
    });
    expect(morphOf(look({ diet: 'carnivore' })).fangs).toBe(1);
    const odd = morphOf(look({ eyes: 40, ears: 3, legs: 5, tail: 'spiky' as never, ear_shape: 'x' as never, snout: 9 as never, fangs: -2 as never, eye_size: 1.4 as never, eye_azimuth: 7 as never }));
    expect(odd).toMatchObject({ eyeSet: 2, eyes: 6, ears: 4, legs: 4, tail: 'thin', earShape: 'round', snout: 2, fangs: 0, eyeSize: 1 });
    expect(morphOf(look({ ears: 1 })).ears).toBe(2);
    expect(morphOf(look({ legs: 7 })).legs).toBe(6);
  });

  it('draws the number of legs and ears it is given', () => {
    // Every leg is drawn (the far one of each pair behind, in the shadow ramp), and so is every ear.
    for (const legs of [0, 2, 4, 6, 8]) expect(pieces(adult({ legs }), 'leg'), String(legs)).toHaveLength(legs);
    for (const ears of [0, 2, 4]) expect(pieces(adult({ ears }), 'ear'), String(ears)).toHaveLength(ears);
    // Swimmers show fins, not legs.
    expect(pieces(adult({ legs: 8, locomotion: 'swim' }), 'leg')).toHaveLength(0);
  });

  it('shows the near-side eyes: one per pair and the midline one', () => {
    expect([1, 2, 3, 4, 5, 6].map((eyes) => glints(adult({ eyes })).length)).toEqual([1, 1, 2, 2, 3, 3]);
    expect(differs({ eyes: 1 }, { eyes: 2 })).toBe(true);
    expect(differs({ eyes: 3 }, { eyes: 4 })).toBe(true);
    expect(differs({ eyes: 5 }, { eyes: 6 })).toBe(true);
    const eye = (eye_size: 0 | 1 | 2) => glints(adult({ eye_size }))[0].ry;
    expect(eye(0)).toBeLessThan(eye(1));
    expect(eye(2)).toBeGreaterThan(eye(1));
  });

  it('places eyes forward (near the snout, both visible) or on the side (high and back)', () => {
    expect(EYE_AZIMUTH[0]).toBeLessThan(35);
    for (const body of [{}, { diet: 'carnivore' as const }, { warm: false, cover: 'scales' as const }, { locomotion: 'swim' as const }]) {
      for (const n of [2, 3, 4, 5, 6]) {
        const tag = `${JSON.stringify(body)} ${n} eyes`;
        const def = glints(adult({ ...body, eyes: n }));
        // Forward: the far eye of the front pair shows too; side-set, the same eyes elsewhere.
        expect(glints(adult({ ...body, eyes: n, eye_azimuth: 0 })).length, tag).toBe(def.length + 1);
        expect(glints(adult({ ...body, eyes: n, eye_azimuth: 2 })).length, tag).toBe(def.length);
        expect(differs({ eyes: n, eye_azimuth: 0 }, { eyes: n }, body), tag).toBe(true);
        expect(differs({ eyes: n, eye_azimuth: 2 }, { eyes: n }, body), tag).toBe(true);
      }
      // Two eyes: the far eye (drawn first) ahead of the near one; forward-set
      // further forward than the default, side-set higher and further back.
      const tag = JSON.stringify(body);
      const [d] = glints(adult(body));
      const [far, near] = glints(adult({ ...body, eye_azimuth: 0 }));
      const [side] = glints(adult({ ...body, eye_azimuth: 2 }));
      expect(far.x, tag).toBeGreaterThan(near.x);
      expect(far.ry, tag).toBeLessThan(near.ry);
      expect(near.x, tag).toBeGreaterThan(d.x);
      expect(side.x, tag).toBeLessThan(d.x);
      expect(side.y, tag).toBeLessThan(d.y);
    }
    // The default bucket as sent draws (and keys) as when absent; one eye has no placement.
    expect(differs({ eye_azimuth: 1 }, {})).toBe(false);
    expect(creatureKey(adult({ eye_azimuth: 1 }))).toBe(creatureKey(adult()));
    expect(differs({ eyes: 1, eye_azimuth: 0 }, { eyes: 1 })).toBe(false);
    expect(creatureKey(adult({ eyes: 1, eye_azimuth: 2 }))).toBe(creatureKey(adult({ eyes: 1 })));
    // Placement gets its own key, and far detail ignores it.
    const keys = new Set([0, 1, 2].map((eye_azimuth) => creatureKey(adult({ eye_azimuth: eye_azimuth as 0 | 1 | 2 }))));
    expect(keys.size).toBe(3);
    expect(creatureKey({ ...adult({ eye_azimuth: 0 }), far: true })).toBe(creatureKey(adult()));
  });

  it('changes the drawing for every ear shape, tail shape and size bucket', () => {
    for (const a of EAR_SHAPES) for (const b of EAR_SHAPES) if (a !== b) expect(differs({ ear_shape: a }, { ear_shape: b }), `${a} ${b}`).toBe(true);
    for (const a of TAIL_LOOKS) for (const b of TAIL_LOOKS) if (a !== b) expect(differs({ tail: a }, { tail: b }), `${a} ${b}`).toBe(true);
    expect(pieces(adult({ tail: 'none' }), 'tail')).toHaveLength(0);
    for (const f of ['ear_size', 'tail_size', 'snout', 'eye_size', 'fangs'] as const) {
      expect(differs({ [f]: 0 }, { [f]: 1 }, { diet: 'carnivore' }), f).toBe(true);
      expect(differs({ [f]: 1 }, { [f]: 2 }, { diet: 'carnivore' }), f).toBe(true);
    }
    // Snouts grow longer, for every diet.
    for (const diet of ['herbivore', 'omnivore', 'carnivore'] as const) {
      const reach = (snout: 0 | 1 | 2) => draw(adult({ diet, snout })).drawing.anchors.snoutTip[0];
      expect(reach(0), diet).toBeLessThan(reach(1));
      expect(reach(1), diet).toBeLessThan(reach(2));
    }
    // Fangs (tusks on herbivores): none, small, large (longer).
    expect(tagged(adult({ fangs: 0, diet: 'carnivore' }), 'fang')).toHaveLength(0);
    expect(tagged(adult({ fangs: 1, diet: 'carnivore' }), 'fang')).toHaveLength(1);
    expect(tagged(adult({ fangs: 1 }), 'tusk')).toHaveLength(1);
    const fangTip = (fangs: 1 | 2) => draw(adult({ fangs, diet: 'carnivore' })).drawing.anchors.fangTip[1];
    expect(fangTip(2)).toBeGreaterThan(fangTip(1));
  });

  it('keeps round ears flush on cold-blooded and swimming bodies, but shows other shapes', () => {
    expect(pieces(adult({ warm: false }), 'ear')).toHaveLength(0);
    expect(pieces(adult({ locomotion: 'swim' }), 'ear')).toHaveLength(0);
    // At every size (no ear opening in their place).
    for (const ear_size of [0, 1, 2] as const) expect(pieces(adult({ warm: false, ear_shape: 'pointed', ear_size }), 'ear'), String(ear_size)).toHaveLength(2);
  });

  it('gives one key per distinct drawing, and shares keys for what is not drawn', () => {
    const byKey = new Map<string, string>();
    const byDrawing = new Map<string, string>();
    for (const l of morphLooks()) {
      for (const juvenile of [false, true]) {
        const s = { look: l, juvenile };
        const key = creatureKey(s);
        const d = drawn(s);
        expect(byKey.get(key) ?? d, key).toBe(d);
        expect(byDrawing.get(d) ?? key, key).toBe(key);
        byKey.set(key, d);
        byDrawing.set(d, key);
      }
    }
    expect(creatureKey(adult({ locomotion: 'swim', legs: 8 }))).toBe(creatureKey(adult({ locomotion: 'swim', legs: 2 })));
    // A swimmer's thin tail is its tail fin.
    expect(creatureKey(adult({ locomotion: 'swim', tail: 'thin' }))).toBe(creatureKey(adult({ locomotion: 'swim', tail: 'fin' })));
    expect(creatureKey(adult({ ears: 0, ear_shape: 'fan', ear_size: 2 }))).toBe(creatureKey(adult({ ears: 0 })));
    expect(creatureKey(adult({ warm: false, ear_size: 2 }))).toBe(creatureKey(adult({ warm: false, ear_size: 0 })));
    expect(creatureKey(adult({ tail: 'none', tail_size: 2 }))).toBe(creatureKey(adult({ tail: 'none', tail_size: 0 })));
    expect(creatureKey({ ...adult({ legs: 8 }), far: true })).toBe(creatureKey(adult()));
    expect(visibleMorph(look({ legs: 8 }), true).legs).toBe(4);
  }, EXHAUSTIVE_MS);

  it('bounds the number of keys', () => {
    // Every field quantized: per class and stage at most
    // (1 + 5 placed × 3 placements) eye counts × 3 sizes × (1 + 2 counts × 4 shapes × 3 sizes) ears × 5 legs × (1 + 5 × 3) tails × 3 snouts × 3 fangs.
    const perClass = (1 + 5 * 3) * 3 * (1 + 2 * 4 * 3) * 5 * (1 + 5 * 3) * 3 * 3;
    expect(perClass).toBe(864000);
    // Any wire value maps into those values.
    for (const v of [-5, 0.4, 2.6, 100, NaN]) {
      const m = morphOf(look({ eyes: v, ears: v, legs: v, eye_size: v as never, eye_azimuth: v as never, snout: v as never }));
      expect([0, 1, 2]).toContain(m.eyeSet);
      expect([1, 2, 3, 4, 5, 6]).toContain(m.eyes);
      expect([0, 2, 4]).toContain(m.ears);
      expect([0, 2, 4, 6, 8]).toContain(m.legs);
      expect([0, 1, 2]).toContain(m.eyeSize);
      expect([0, 1, 2]).toContain(m.snout);
    }
  });

  it('keeps every morphology inside its body texture', () => {
    for (const l of morphLooks()) {
      for (const juvenile of [false, true]) {
        const s = { look: l, juvenile };
        const sd = draw(s);
        const b = pixelBounds(sd);
        expect(Math.max(-b.minX, b.maxX), creatureKey(s)).toBeLessThanOrEqual(sd.half.w);
        expect(Math.max(-b.minY, b.maxY), creatureKey(s)).toBeLessThanOrEqual(sd.half.h);
        // Long features grow the frame a little at most.
        expect(Math.max(sd.half.w, sd.half.h), creatureKey(s)).toBeLessThanOrEqual(spriteExtent(s) * 1.6);
      }
    }
  }, EXHAUSTIVE_MS);
});

describe('describeLook', () => {
  it('puts the body plan in words', () => {
    expect(describeLook(look({ size: 3, diet: 'carnivore', warm: false, cover: 'scales', coloring: 'camo', locomotion: 'swim' })))
      .toBe('large cold-blooded carnivore · scales · camouflaged · swims');
  });
});

describe('action hints', () => {
  it('maps actions to icons', () => {
    expect(actionHint('rest')).toBe('sleep');
    expect(actionHint('dormant')).toBe('sleep');
    expect(actionHint('flee')).toBe('flee');
    expect(actionHint('hunt')).toBe('hunt');
    expect(actionHint('seek_mate')).toBe('mate');
    expect(actionHint('graze')).toBeNull();
    expect(actionHint('something_new')).toBeNull();
  });

  it('has shapes for every icon', () => {
    for (const h of ['sleep', 'flee', 'hunt', 'mate'] as const) expect(hintShapes(h).length).toBeGreaterThan(0);
  });
});
