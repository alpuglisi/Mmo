import { describe, expect, it } from 'vitest';
import type { Appearance } from '../protocol';
import { creatureKey, DEFAULT_LOOK, EAR_SHAPES, markerScaleFor, TAIL_LOOKS, type CreatureSpec } from './creature';
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

  it('caches nothing when making fails (a lost context), and makes it again later', () => {
    const c = new TextureCache<string>();
    expect(() => c.get('x', () => { throw new Error('context lost'); })).toThrow('context lost');
    expect(c.size).toBe(0);
    expect(c.made).toBe(0);
    expect(c.get('x', () => 'X')).toBe('X');
    expect(c.made).toBe(1);
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

describe('TextureCache bounds', () => {
  it('drops the least recently used values past maxCount, down to the low water mark', () => {
    const disposed: string[] = [];
    const c = new TextureCache<string>({ dispose: (v) => disposed.push(v), maxCount: 4, lowWater: 0.5 });
    for (const k of ['a', 'b', 'c', 'd']) c.get(k, () => k.toUpperCase());
    c.get('a', () => 'A2'); // a is now the most recent
    expect(c.made).toBe(4);
    c.get('e', () => 'E'); // 5 > 4: drop to 2, oldest first (b, c, d)
    expect(disposed).toEqual(['B', 'C', 'D']);
    expect(c.size).toBe(2);
    expect(c.has('a') && c.has('e')).toBe(true);
    expect(c.evicted).toBe(3);
  });

  it('bounds the total cost, never drops what is in use, and keeps the new value', () => {
    type V = { k: string; n: number };
    const inUse = new Set<V>();
    const c = new TextureCache<V>({ cost: (v) => v.n, maxCost: 100, inUse: () => inUse });
    inUse.add(c.get('big', () => ({ k: 'big', n: 60 })));
    c.get('x', () => ({ k: 'x', n: 30 }));
    c.get('y', () => ({ k: 'y', n: 30 })); // 120 > 100: x goes, big stays (in use)
    expect(c.has('big')).toBe(true);
    expect(c.has('x')).toBe(false);
    expect(c.has('y')).toBe(true);
    expect(c.cost).toBe(90);
    // Everything in use: the bound is soft, nothing is dropped.
    inUse.add(c.get('y', () => ({ k: 'y', n: 0 })));
    c.get('z', () => ({ k: 'z', n: 50 }));
    expect(c.size).toBe(3);
    expect(c.cost).toBe(140);
    c.clear();
    expect(c.cost).toBe(0);
  });

  it('times each make', () => {
    let t = 0;
    const c = new TextureCache<number>({ now: () => t });
    c.get('a', () => { t += 2; return 1; });
    c.get('b', () => { t += 5; return 2; });
    c.get('a', () => { t += 100; return 3; });
    expect(c.makeMs).toBe(7);
    expect(c.maxMakeMs).toBe(5);
  });

  it('stays within its bound for a stream of many distinct looks', () => {
    const c = new TextureCache<number>({ maxCount: 256 });
    let peak = 0;
    for (let i = 0; i < 5000; i++) {
      c.get(`k${i % 1200}`, () => i);
      peak = Math.max(peak, c.size);
    }
    expect(peak).toBeLessThanOrEqual(256);
    expect(c.evicted).toBe(c.made - c.size);
  });
});

describe('creature keys at scale', () => {
  /** A deterministic generator (mulberry32). */
  function rng(seed: number): () => number {
    let a = seed;
    return () => {
      a = (a + 0x6d2b79f5) | 0;
      let t = Math.imul(a ^ (a >>> 15), 1 | a);
      t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
      return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
    };
  }

  it('keeps the distinct textures of a crowded view in the low hundreds', () => {
    // A crowded view: 300 organisms from 40 lineages (players' founders, each a
    // random body plan and morphology); each organism has each feature's
    // lineage value or, 10% of the time, a mutated neighbor value. (Seed 7:
    // 186 near keys, 77 far.)
    const r = rng(7);
    const pick = <T,>(xs: readonly T[]): T => xs[Math.floor(r() * xs.length)];
    const mutate = (v: number, lo: number, hi: number, by = 1) => (r() < 0.1 ? Math.min(hi, Math.max(lo, v + (r() < 0.5 ? -by : by))) : v);
    const b = (v: number) => v as 0 | 1 | 2;
    const lineages: Appearance[] = [];
    for (let i = 0; i < 40; i++) {
      lineages.push({
        size: pick([0, 1, 2, 3] as const), diet: pick(['herbivore', 'omnivore', 'carnivore'] as const), warm: r() < 0.6,
        locomotion: pick(['walk', 'walk', 'walk', 'swim', 'glide', 'fly'] as const), cover: pick(['skin', 'fur', 'scales', 'armor'] as const),
        coloring: pick(['drab', 'camo', 'bright', 'warning'] as const), eyes: pick([1, 2, 2, 2, 4, 6]), eye_size: pick([0, 1, 2] as const), ears: pick([0, 2, 2, 4]),
        ear_shape: pick(EAR_SHAPES), ear_size: pick([0, 1, 2] as const), legs: pick([2, 4, 4, 6, 8]), tail: pick(TAIL_LOOKS), tail_size: pick([0, 1, 2] as const),
        snout: pick([0, 1, 2] as const), fangs: pick([0, 1, 2] as const),
      });
    }
    const near = new Set<string>();
    const far = new Set<string>();
    for (let i = 0; i < 300; i++) {
      const l = lineages[i % lineages.length];
      const o: Appearance = {
        ...l, eyes: mutate(l.eyes!, 1, 6), eye_size: b(mutate(l.eye_size!, 0, 2)), ears: mutate(l.ears!, 0, 4, 2), legs: mutate(l.legs!, 0, 8, 2),
        tail_size: b(mutate(l.tail_size!, 0, 2)), snout: b(mutate(l.snout!, 0, 2)), fangs: b(mutate(l.fangs!, 0, 2)),
      };
      const juvenile = r() < 0.3;
      near.add(creatureKey({ look: o, juvenile }));
      far.add(creatureKey({ look: o, juvenile, far: true }));
    }
    expect(near.size).toBeLessThan(256);
    expect(far.size).toBeLessThanOrEqual(80);
    expect(far.size).toBeLessThan(near.size);
  });
});
