import { describe, expect, it } from 'vitest';
import { creatureKey, DEFAULT_LOOK, markerScaleFor, type CreatureSpec } from './creature';
import { TextureCache } from './textureCache';

describe('TextureCache', () => {
  it('makes each key once and shares it', () => {
    let n = 0;
    const c = new TextureCache<{ id: number }>();
    const a = c.get('a', () => ({ id: n++ }));
    expect(c.get('a', () => ({ id: n++ }))).toBe(a);
    c.get('b', () => ({ id: n++ }));
    expect(n).toBe(2);
    expect(c.made).toBe(2);
    expect(c.size).toBe(2);
  });

  it('disposes every value on clear', () => {
    const disposed: string[] = [];
    const c = new TextureCache<string>((v) => disposed.push(v));
    c.get('x', () => 'X');
    c.get('y', () => 'Y');
    c.clear();
    expect(disposed.sort()).toEqual(['X', 'Y']);
    expect(c.size).toBe(0);
    c.get('x', () => 'X2');
    expect(c.made).toBe(3);
  });

  it('keeps textures few for hundreds of markers', () => {
    // 600 organisms drawn from a handful of body plans: one texture per class.
    const plans: CreatureSpec[] = [
      { look: DEFAULT_LOOK, juvenile: false },
      { look: DEFAULT_LOOK, juvenile: true },
      { look: { ...DEFAULT_LOOK, diet: 'carnivore', size: 3 }, juvenile: false },
      { look: { ...DEFAULT_LOOK, locomotion: 'fly', size: 0 }, juvenile: false },
      { look: { ...DEFAULT_LOOK, warm: false, cover: 'scales', locomotion: 'swim' }, juvenile: false },
    ];
    const c = new TextureCache<object>();
    for (let i = 0; i < 600; i++) c.get(creatureKey(plans[i % plans.length]), () => ({}));
    expect(c.made).toBe(plans.length);
  });
});

describe('marker scale', () => {
  it('grows with zoom within bounds', () => {
    expect(markerScaleFor(8)).toBeCloseTo(1, 1);
    expect(markerScaleFor(1)).toBe(0.75);
    expect(markerScaleFor(48)).toBe(1.9);
    expect(markerScaleFor(16)).toBeGreaterThan(markerScaleFor(8));
  });
});
