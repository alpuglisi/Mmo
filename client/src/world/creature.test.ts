import { describe, expect, it } from 'vitest';
import type { Appearance, NearbyOrganism, OwnOrganism } from '../protocol';
import {
  actionHint, bodyRadius, describeLook, creatureKey, creatureShapes, DEFAULT_LOOK, hintShapes, JUVENILE_SCALE, palette, specOf, spriteExtent,
  type CreatureSpec, type Shape,
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
    const legs = (s: Shape[]) => s.filter((x) => x.kind === 'line' && x.stroke === 0x15181c && x.points[1][1] > x.points[0][1]).length;
    expect(legs(base)).toBe(4);
    expect(legs(creatureShapes(adult({ locomotion: 'swim' })))).toBe(0);
  });

  it('colors by coloring class', () => {
    expect(palette(look({ coloring: 'warning' })).accent).toBe(0x1b1b1b);
    expect(palette(look({ coloring: 'bright' })).base).not.toBe(palette(look({ coloring: 'drab' })).base);
    expect(palette(look({ warm: false })).base).not.toBe(palette(look({ warm: true })).base);
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
