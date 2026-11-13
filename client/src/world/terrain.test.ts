import { describe, expect, it } from 'vitest';
import type { TerrainDynamic, TerrainView } from '../protocol';
import { applyDynamic, B, decodeBase64, decodeTerrain, isFlooded, paintTerrain, terrainSummary, tileColor } from './terrain';

const b64 = (bytes: number[]) => btoa(String.fromCharCode(...bytes));

/** A 3×2 map: deep lake, shallow lake, river | grass, forest, mountain. */
function view(dyn: Partial<Record<'water' | 'ice' | 'snow' | 'veg' | 'wetness', number[]>> = {}, tick = 10): TerrainView {
  const d = {
    water: [255, 60, 40, 0, 0, 0],
    ice: [0, 0, 0, 0, 0, 0],
    snow: [0, 0, 0, 0, 0, 0],
    veg: [0, 0, 0, 255, 255, 0],
    wetness: [128, 128, 128, 128, 128, 128],
    ...dyn,
  };
  return {
    width: 3,
    height: 2,
    terrain_version: 1,
    biome: b64([B.deep, B.shallow, B.grass, B.grass, B.forest, B.mountain]),
    elevation: b64([0, 10, 60, 100, 150, 255]),
    river: b64([0, 0, 200, 0, 0, 0]),
    base_water: b64([255, 60, 40, 0, 0, 0]),
    dynamic: dynamic(d, tick),
  };
}

function dynamic(d: Record<'water' | 'ice' | 'snow' | 'veg' | 'wetness', number[]>, tick: number, season = 'spring'): TerrainDynamic {
  return { tick, day: 0, hour: 6, season, water: b64(d.water), ice: b64(d.ice), snow: b64(d.snow), veg: b64(d.veg), wetness: b64(d.wetness) };
}

describe('terrain decoding', () => {
  it('decodes base64 layers into one byte per tile', () => {
    expect([...decodeBase64(b64([0, 1, 254, 255]))]).toEqual([0, 1, 254, 255]);
    const g = decodeTerrain(view());
    expect(g.width * g.height).toBe(6);
    expect([...g.biome]).toEqual([0, 1, 3, 3, 4, 7]);
    expect(g.elevation[5]).toBe(255);
    expect(g.river[2]).toBe(200);
    expect(g.water[0]).toBe(255);
    expect(g.tick).toBe(10);
    expect(g.season).toBe('spring');
  });

  it('refuses layers of the wrong size', () => {
    const bad = { ...view(), biome: b64([1, 2, 3]) };
    expect(() => decodeTerrain(bad)).toThrow(/biome/);
    expect(() => decodeTerrain({ ...view(), width: 0 })).toThrow();
  });

  it('applies newer dynamic layers and ignores older ones', () => {
    const g = decodeTerrain(view());
    const all = (v: number) => [v, v, v, v, v, v];
    applyDynamic(g, dynamic({ water: all(0), ice: all(0), snow: all(200), veg: all(10), wetness: all(20) }, 20, 'winter'));
    expect(g.tick).toBe(20);
    expect(g.season).toBe('winter');
    expect(g.snow[3]).toBe(200);
    applyDynamic(g, dynamic({ water: all(9), ice: all(9), snow: all(9), veg: all(9), wetness: all(9) }, 15));
    expect(g.snow[3]).toBe(200);
  });
});

describe('tile colors', () => {
  const lum = (c: number[]) => c[0] + c[1] + c[2];

  it('shows snow, ice, drought, floods and dry river beds', () => {
    const base = decodeTerrain(view());
    const grass = tileColor(base, 3);
    expect(grass[1]).toBeGreaterThan(grass[2]); // green

    const snowy = decodeTerrain(view({ snow: [0, 0, 0, 255, 0, 0] }));
    expect(lum(tileColor(snowy, 3))).toBeGreaterThan(600);

    const iced = decodeTerrain(view({ ice: [0, 255, 0, 0, 0, 0] }));
    const ice = tileColor(iced, 1);
    expect(lum(ice)).toBeGreaterThan(lum(tileColor(base, 1)) + 200);

    const dry = decodeTerrain(view({ wetness: [128, 128, 128, 0, 128, 128] }));
    const parched = tileColor(dry, 3);
    expect(parched[0]).toBeGreaterThan(grass[0] + 40); // straw, not green

    const flooded = decodeTerrain(view({ water: [255, 60, 40, 80, 0, 0] }));
    expect(isFlooded(flooded, 3)).toBe(true);
    expect(isFlooded(base, 3)).toBe(false);
    expect(tileColor(flooded, 3)[2]).toBeGreaterThan(grass[2] + 50); // bluish

    const dryRiver = decodeTerrain(view({ water: [255, 60, 0, 0, 0, 0] }));
    expect(tileColor(dryRiver, 2)).toEqual([176, 160, 120]);
    const driedLake = decodeTerrain(view({ water: [255, 0, 40, 0, 0, 0] }));
    expect(tileColor(driedLake, 1)).toEqual([150, 130, 95]);
  });

  it('tints plant cover by season', () => {
    const spring = decodeTerrain(view());
    const autumn = decodeTerrain(view());
    autumn.season = 'autumn';
    expect(tileColor(autumn, 3)[0]).toBeGreaterThan(tileColor(spring, 3)[0]);
    // Desert-free mountain tiles don't take the tint.
    expect(tileColor(autumn, 5)).toEqual(tileColor(spring, 5));
  });

  it('paints every tile opaque and summarizes the map', () => {
    const g = decodeTerrain(view({ snow: [0, 0, 0, 255, 0, 0], wetness: [128, 128, 128, 128, 0, 128] }));
    const out = new Uint8Array(6 * 4);
    paintTerrain(g, out);
    for (let i = 0; i < 6; i++) expect(out[i * 4 + 3]).toBe(255);
    expect(out[3 * 4]).toBeGreaterThan(200); // the snowy tile
    expect(() => paintTerrain(g, new Uint8Array(4))).toThrow();
    const s = terrainSummary(g);
    expect(s.snow).toBeCloseTo(1 / 4);
    expect(s.drought).toBeCloseTo(1 / 4);
    expect(s.flood).toBe(0);
  });
});
