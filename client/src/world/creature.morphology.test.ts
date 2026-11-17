import { describe, expect, it } from 'vitest';
import { creatureKey, DEFAULT_LOOK, EAR_SHAPES, morphOf, spriteExtent, TAIL_LOOKS, visibleMorph } from './creature';
import { EYE_AZIMUTH } from './spriteArt';
import { EXHAUSTIVE_MS, look, adult, draw, drawn, pieces, tagged, glints, differs, pixelBounds, morphLooks } from './creature.testkit';

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
