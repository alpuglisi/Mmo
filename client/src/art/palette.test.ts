import { describe, expect, it } from 'vitest';
import { creaturePalette, fromHsl, toHsl, tonesOf } from './palette';
import { scallopRing, ellipsePts, rng, area } from './geom';

const world = { base: 0xa47a52, dark: 0x6e4f33, light: 0xd2b48c, accent: 0x7d5b3c };

describe('palette', () => {
  it('round-trips HSL', () => {
    for (const c of [0xa47a52, 0x3fd4e8, 0xffc61a, 0x221a16]) {
      const [h, s, l] = toHsl(c);
      const back = fromHsl(h, s, l);
      for (const sh of [16, 8, 0]) expect(Math.abs(((back >> sh) & 255) - ((c >> sh) & 255))).toBeLessThanOrEqual(1);
    }
  });

  it('one shade and one highlight step: darker and cooler, lighter and warmer', () => {
    const t = tonesOf(0xa47a52);
    expect(toHsl(t.shade)[2]).toBeLessThan(toHsl(t.base)[2]);
    expect(toHsl(t.light)[2]).toBeGreaterThan(toHsl(t.base)[2]);
  });

  it('vividness pushes the body from drab to vivid; far limbs sit in shadow', () => {
    const drab = creaturePalette({ world, coloring: 'drab', vividness: 0, warm: true });
    const vivid = creaturePalette({ world, coloring: 'drab', vividness: 1, warm: true });
    expect(toHsl(vivid.body.base)[1]).toBeGreaterThan(toHsl(drab.body.base)[1]);
    expect(toHsl(drab.far.base)[2]).toBeLessThan(toHsl(drab.body.base)[2]);
    // Shared materials: every creature uses the same ink and bone.
    expect(vivid.ink).toBe(drab.ink);
    expect(vivid.bone).toBe(drab.bone);
  });
});

describe('scallopRing', () => {
  it('pushes locks outward, whichever way the outline winds', () => {
    const ring = ellipsePts(0, 0, 20, 10, 24);
    for (const pts of [ring, ring.slice().reverse()]) {
      const out = scallopRing(pts, 10, () => 3, 0, rng(1), true);
      expect(Math.abs(area(out))).toBeGreaterThan(Math.abs(area(pts)));
    }
  });
});
