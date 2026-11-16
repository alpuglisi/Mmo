import { createHash } from 'node:crypto';
import { describe, expect, it } from 'vitest';
import type { Appearance, NearbyOrganism, OwnOrganism } from '../protocol';
import {
  actionHint, bodyExtent, bodyRadius, describeLook, creatureKey, creatureShapes, DEFAULT_LOOK, EAR_SHAPES, hintShapes, JUVENILE_SCALE, morphOf, palette, specOf,
  spriteExtent, TAIL_LOOKS, visibleMorph, type CreatureSpec, type Part, type Shape,
} from './creature';

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

const parts = (s: Shape[], part: Part) => s.filter((x) => x.part === part).length;

function bounds(shapes: Shape[]): { minX: number; maxX: number; minY: number; maxY: number } {
  let minX = Infinity, maxX = -Infinity, minY = Infinity, maxY = -Infinity;
  const add = (x: number, y: number) => {
    minX = Math.min(minX, x); maxX = Math.max(maxX, x); minY = Math.min(minY, y); maxY = Math.max(maxY, y);
  };
  for (const s of shapes) {
    if (s.kind === 'ellipse') {
      add(s.x - s.rx, s.y - s.ry);
      add(s.x + s.rx, s.y + s.ry);
    } else for (const [x, y] of s.points) add(x, y);
  }
  return { minX, maxX, minY, maxY };
}

describe('appearance classes', () => {
  it('gives every distinct appearance and life stage its own cache key', () => {
    const keys = new Set<string>();
    const looks = allLooks();
    for (const l of looks) for (const juvenile of [false, true]) keys.add(creatureKey({ look: l, juvenile }));
    expect(keys.size).toBe(looks.length * 2);
  });

  it('draws every class exactly as before morphology, without the fields and zoomed out', () => {
    // sha256 of the shapes of all 1,536 classes × stage from the sprite code
    // before morphology (numbers to 1e-6, `part` tags left out).
    const BEFORE = 'bc67e1fe8a8b2c03adc56c29f0bb3db63d1b382ce6401846ededcac24a03fc35';
    const hash = (far: boolean, fields: (l: Appearance) => Partial<Appearance>) => {
      const h = createHash('sha256');
      const replacer = (k: string, v: unknown) => (k === 'part' ? undefined : typeof v === 'number' ? Math.round(v * 1e6) / 1e6 : v);
      for (const l of allLooks()) for (const juvenile of [false, true]) h.update(JSON.stringify(creatureShapes({ look: { ...l, ...fields(l) }, juvenile, far }), replacer));
      return h.digest('hex');
    };
    expect(hash(false, () => ({}))).toBe(BEFORE);
    // The contract defaults as a server sends them draw the same.
    const defaults = (l: Appearance): Partial<Appearance> => ({
      eyes: 2, eye_size: 1, ears: 2, ear_shape: 'round', ear_size: 1, legs: 4, tail: 'thin', tail_size: 1, snout: 1, fangs: l.diet === 'carnivore' ? 1 : 0,
    });
    expect(hash(false, defaults)).toBe(BEFORE);
    // Far detail ignores the morphology: the class sprite.
    expect(hash(true, () => ({ eyes: 6, ears: 4, ear_shape: 'long', legs: 8, tail: 'club', tail_size: 2, snout: 2, fangs: 2 }))).toBe(BEFORE);
  });

  it('reads the look from an organism, with a default for older servers and juveniles from `matured`', () => {
    const own = { id: 1, matured: false, look: look({ diet: 'carnivore' }) } as unknown as OwnOrganism;
    expect(specOf(own)).toEqual({ look: look({ diet: 'carnivore' }), juvenile: true });
    const old = { id: 2, owner: 0, matured: true } as unknown as NearbyOrganism;
    expect(specOf(old)).toEqual({ look: DEFAULT_LOOK, juvenile: false });
  });

  it('sizes bodies by size class and draws juveniles smaller', () => {
    const r = [0, 1, 2, 3].map((size) => bodyRadius(adult({ size: size as Appearance['size'] })));
    for (let i = 1; i < r.length; i++) expect(r[i]).toBeGreaterThan(r[i - 1]);
    const j = bodyRadius({ look: look({ size: 2 }), juvenile: true });
    expect(j).toBeCloseTo(r[2] * JUVENILE_SCALE);
  });

  it('keeps every shape inside the texture frame', () => {
    for (const l of allLooks()) {
      for (const juvenile of [false, true]) {
        const s = { look: l, juvenile };
        const e = spriteExtent(s);
        const b = bounds(creatureShapes(s));
        expect(Math.max(-b.minX, b.maxX, -b.minY, b.maxY), creatureKey(s)).toBeLessThanOrEqual(e);
      }
    }
  });

  it('draws visibly different body plans', () => {
    const base = creatureShapes(adult());
    const differs = (over: Partial<Appearance>) => JSON.stringify(creatureShapes(adult(over))) !== JSON.stringify(base);
    expect(differs({ diet: 'carnivore' })).toBe(true);
    expect(differs({ warm: false })).toBe(true);
    expect(differs({ locomotion: 'swim' })).toBe(true);
    expect(differs({ locomotion: 'fly' })).toBe(true);
    expect(differs({ cover: 'scales' })).toBe(true);
    expect(differs({ coloring: 'warning' })).toBe(true);
    // Fliers reach well above the body (the raised wing); swimmers have no legs.
    expect(bounds(creatureShapes(adult({ locomotion: 'fly' }))).minY).toBeLessThan(bounds(base).minY - 3);
    expect(parts(base, 'leg')).toBe(4);
    expect(parts(creatureShapes(adult({ locomotion: 'swim' })), 'leg')).toBe(0);
  });

  it('colors by coloring class', () => {
    expect(palette(look({ coloring: 'warning' })).accent).toBe(0x1b1b1b);
    expect(palette(look({ coloring: 'bright' })).base).not.toBe(palette(look({ coloring: 'drab' })).base);
    expect(palette(look({ warm: false })).base).not.toBe(palette(look({ warm: true })).base);
  });
});

/** Looks covering every morphology value on a spread of body plans. */
function morphLooks(): Appearance[] {
  const bodies: Partial<Appearance>[] = [
    {}, { warm: false, cover: 'scales', diet: 'carnivore' }, { locomotion: 'swim', warm: false }, { locomotion: 'fly', size: 0 }, { size: 3, diet: 'omnivore', cover: 'armor' },
  ];
  const morphs: Partial<Appearance>[] = [];
  for (const eyes of [1, 2, 3, 4, 5, 6]) for (const eye_size of [0, 1, 2] as const) morphs.push({ eyes, eye_size });
  for (const ears of [0, 2, 4]) for (const ear_shape of EAR_SHAPES) for (const ear_size of [0, 1, 2] as const) morphs.push({ ears, ear_shape, ear_size });
  for (const legs of [0, 2, 4, 6, 8]) morphs.push({ legs });
  for (const tail of TAIL_LOOKS) for (const tail_size of [0, 1, 2] as const) morphs.push({ tail, tail_size });
  for (const snout of [0, 1, 2] as const) for (const fangs of [0, 1, 2] as const) morphs.push({ snout, fangs });
  const out: Appearance[] = [];
  for (const size of [0, 1, 2, 3] as const) for (const b of bodies) for (const m of morphs) out.push(look({ size, ...b, ...m }));
  return out;
}

describe('morphology', () => {
  const differs = (a: Partial<Appearance>, b: Partial<Appearance>, body: Partial<Appearance> = {}) =>
    JSON.stringify(creatureShapes(adult({ ...body, ...a }))) !== JSON.stringify(creatureShapes(adult({ ...body, ...b })));

  it('fills defaults for older servers and clamps values from the wire', () => {
    expect(morphOf(DEFAULT_LOOK)).toEqual({ eyes: 2, eyeSize: 1, ears: 2, earShape: 'round', earSize: 1, legs: 4, tail: 'thin', tailSize: 1, snout: 1, fangs: 0 });
    expect(morphOf(look({ diet: 'carnivore' })).fangs).toBe(1);
    const odd = morphOf(look({ eyes: 40, ears: 3, legs: 5, tail: 'spiky' as never, ear_shape: 'x' as never, snout: 9 as never, fangs: -2 as never, eye_size: 1.4 as never }));
    expect(odd).toMatchObject({ eyes: 6, ears: 4, legs: 4, tail: 'thin', earShape: 'round', snout: 2, fangs: 0, eyeSize: 1 });
    expect(morphOf(look({ ears: 1 })).ears).toBe(2);
    expect(morphOf(look({ legs: 7 })).legs).toBe(6);
  });

  it('draws the number of legs and ears it is given', () => {
    for (const legs of [0, 2, 4, 6, 8]) expect(parts(creatureShapes(adult({ legs })), 'leg')).toBe(legs);
    // A pair shows its near ear.
    for (const [ears, seen] of [[0, 0], [2, 1], [4, 2]]) expect(parts(creatureShapes(adult({ ears })), 'ear')).toBe(seen);
    // Swimmers show fins, not legs.
    expect(parts(creatureShapes(adult({ legs: 8, locomotion: 'swim' })), 'leg')).toBe(0);
  });

  it('shows the near-side eyes: one per pair and the midline one', () => {
    const seen = [1, 2, 3, 4, 5, 6].map((eyes) => parts(creatureShapes(adult({ eyes })), 'eye'));
    expect(seen).toEqual([1, 1, 2, 2, 3, 3]);
    expect(differs({ eyes: 1 }, { eyes: 2 })).toBe(true);
    expect(differs({ eyes: 3 }, { eyes: 4 })).toBe(true);
    expect(differs({ eyes: 5 }, { eyes: 6 })).toBe(true);
    const eye = (eye_size: 0 | 1 | 2) => creatureShapes(adult({ eye_size })).find((x) => x.part === 'eye') as { rx: number };
    expect(eye(0).rx).toBeLessThan(eye(1).rx);
    expect(eye(2).rx).toBeGreaterThan(eye(1).rx);
  });

  it('changes the drawing for every ear shape, tail shape and size bucket', () => {
    for (const a of EAR_SHAPES) for (const b of EAR_SHAPES) if (a !== b) expect(differs({ ear_shape: a }, { ear_shape: b }), `${a} ${b}`).toBe(true);
    for (const a of TAIL_LOOKS) for (const b of TAIL_LOOKS) if (a !== b) expect(differs({ tail: a }, { tail: b }), `${a} ${b}`).toBe(true);
    expect(parts(creatureShapes(adult({ tail: 'none' })), 'tail')).toBe(0);
    for (const f of ['ear_size', 'tail_size', 'snout', 'eye_size', 'fangs'] as const) {
      expect(differs({ [f]: 0 }, { [f]: 1 }, { diet: 'carnivore' }), f).toBe(true);
      expect(differs({ [f]: 1 }, { [f]: 2 }, { diet: 'carnivore' }), f).toBe(true);
    }
    // Snouts grow longer, for every diet.
    for (const diet of ['herbivore', 'omnivore', 'carnivore'] as const) {
      const reach = (snout: 0 | 1 | 2) => bounds(creatureShapes(adult({ diet, snout }))).maxX;
      expect(reach(0), diet).toBeLessThan(reach(1));
      expect(reach(1), diet).toBeLessThan(reach(2));
    }
    // Fangs: none, small, large (longer).
    const fangDepth = (fangs: 0 | 1 | 2) =>
      Math.max(...creatureShapes(adult({ fangs })).filter((x) => x.part === 'fang').flatMap((x) => (x.kind === 'poly' ? x.points.map((p) => p[1]) : [])));
    expect(parts(creatureShapes(adult({ fangs: 0, diet: 'carnivore' })), 'fang')).toBe(0);
    expect(parts(creatureShapes(adult({ fangs: 1 })), 'fang')).toBe(2);
    expect(fangDepth(2)).toBeGreaterThan(fangDepth(1));
  });

  it('keeps round ears flush on cold-blooded and swimming bodies, as today, but shows other shapes', () => {
    expect(parts(creatureShapes(adult({ warm: false })), 'ear')).toBe(0);
    expect(parts(creatureShapes(adult({ locomotion: 'swim' })), 'ear')).toBe(0);
    expect(parts(creatureShapes(adult({ warm: false, ear_shape: 'pointed' })), 'ear')).toBe(1);
  });

  it('gives one key per distinct drawing, and shares keys for what is not drawn', () => {
    const byKey = new Map<string, string>();
    const byDrawing = new Map<string, string>();
    for (const l of morphLooks()) {
      for (const juvenile of [false, true]) {
        const s = { look: l, juvenile };
        const key = creatureKey(s);
        const drawn = JSON.stringify(creatureShapes(s));
        expect(byKey.get(key) ?? drawn, key).toBe(drawn);
        expect(byDrawing.get(drawn) ?? key, key).toBe(key);
        byKey.set(key, drawn);
        byDrawing.set(drawn, key);
      }
    }
    expect(creatureKey(adult({ locomotion: 'swim', legs: 8 }))).toBe(creatureKey(adult({ locomotion: 'swim', legs: 2 })));
    expect(creatureKey(adult({ ears: 0, ear_shape: 'fan', ear_size: 2 }))).toBe(creatureKey(adult({ ears: 0 })));
    expect(creatureKey(adult({ warm: false, ear_size: 2 }))).toBe(creatureKey(adult({ warm: false, ear_size: 0 })));
    expect(creatureKey(adult({ tail: 'none', tail_size: 2 }))).toBe(creatureKey(adult({ tail: 'none', tail_size: 0 })));
    expect(creatureKey({ ...adult({ legs: 8 }), far: true })).toBe(creatureKey(adult()));
    expect(visibleMorph(look({ legs: 8 }), true).legs).toBe(4);
  });

  it('bounds the number of keys', () => {
    // Every field quantized: per class and stage at most
    // 6 eyes × 3 sizes × (1 + 2 counts × 4 shapes × 3 sizes) ears × 5 legs × (1 + 5 × 3) tails × 3 snouts × 3 fangs.
    const perClass = 6 * 3 * (1 + 2 * 4 * 3) * 5 * (1 + 5 * 3) * 3 * 3;
    expect(perClass).toBe(324000);
    // Any wire value maps into those values.
    for (const v of [-5, 0.4, 2.6, 100, NaN]) {
      const m = morphOf(look({ eyes: v, ears: v, legs: v, eye_size: v as never, snout: v as never }));
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
        const shapes = creatureShapes(s);
        const b = bounds(shapes);
        const e = bodyExtent(s, shapes);
        expect(Math.max(-b.minX, b.maxX, -b.minY, b.maxY), creatureKey(s)).toBeLessThanOrEqual(e);
        // Long features grow the frame a little at most.
        expect(e, creatureKey(s)).toBeLessThanOrEqual(spriteExtent(s) * 1.5);
      }
    }
  });
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
