import { createHash } from 'node:crypto';
import { describe, expect, it } from 'vitest';
import type { Appearance } from '../protocol';
import { EXHAUSTIVE_MS, allLooks, drawn } from './creature.testkit';

describe('appearance classes', () => {
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
});
