import { describe, expect, it } from 'vitest';
import { B, decodeTerrain, type TerrainGrid } from './terrain';
import { baseTpxFor, buildFields, detailFor, paintRegion, regionDelta, regionSnapshot, upsample } from './terrainArt';
import { lightAt, LIGHT } from './palette';
import type { TerrainView } from '../protocol';

const b64 = (bytes: number[]) => btoa(String.fromCharCode(...bytes));

/** A 16×16 map: a lake in the west, grass in the middle, forest in the east, a river row, a mountain corner. */
function grid(over: { snow?: number; season?: string } = {}): TerrainGrid {
  const w = 16, h = 16, n = w * h;
  const biome: number[] = [], elev: number[] = [], river: number[] = [], water: number[] = [];
  for (let y = 0; y < h; y++) {
    for (let x = 0; x < w; x++) {
      const lake = x < 5;
      const riv = y === 8 && x >= 5;
      biome.push(lake ? B.deep : x > 11 && y < 4 ? B.mountain : x > 10 ? B.forest : B.grass);
      elev.push(lake ? 0 : Math.min(255, 40 + x * 10 + (y < 4 ? 60 : 0)));
      river.push(riv ? 150 : 0);
      water.push(lake ? 200 : riv ? 60 : 0);
    }
  }
  const view: TerrainView = {
    width: w, height: h, terrain_version: 1,
    biome: b64(biome), elevation: b64(elev), river: b64(river), base_water: b64(water),
    dynamic: {
      tick: 1, day: 0, hour: 12, season: over.season ?? 'spring',
      water: b64(water), ice: b64(new Array(n).fill(0)), snow: b64(new Array(n).fill(over.snow ?? 0)),
      veg: b64(new Array(n).fill(255)), wetness: b64(new Array(n).fill(128)),
    },
  };
  return decodeTerrain(view);
}

const px = (buf: Uint8Array, W: number, x: number, y: number) => [buf[(y * W + x) * 4], buf[(y * W + x) * 4 + 1], buf[(y * W + x) * 4 + 2], buf[(y * W + x) * 4 + 3]];

describe('painted terrain', () => {
  it('paints water blue, grass green and every texel opaque', () => {
    const f = buildFields(grid());
    const tpx = 4, W = 16 * tpx;
    const buf = new Uint8Array(W * W * 4);
    paintRegion(f, 0, 0, 16, 16, tpx, buf);
    for (let i = 3; i < buf.length; i += 4) expect(buf[i]).toBe(255);
    const lake = px(buf, W, 4, 8 * 4 - 12); // deep in the lake
    expect(lake[2]).toBeGreaterThan(lake[0] + 40);
    const grass = px(buf, W, 8 * tpx, 3 * tpx);
    expect(grass[1]).toBeGreaterThan(grass[2] + 30);
    expect(grass[1]).toBeGreaterThan(grass[0]);
  });

  it('is a function of world position: chunks and row slices meet without seams', () => {
    const f = buildFields(grid());
    const tpx = 8, W = 16 * tpx;
    const whole = new Uint8Array(W * W * 4);
    paintRegion(f, 0, 0, 16, 16, tpx, whole);
    // The east half as its own chunk, painted in two row slices.
    const half = new Uint8Array(8 * tpx * W * 4);
    paintRegion(f, 8, 0, 8, 16, tpx, half, 0, 40);
    paintRegion(f, 8, 0, 8, 16, tpx, half, 40, 16 * tpx);
    for (const [x, y] of [[0, 0], [3, 50], [63, 127], [20, 64]]) {
      expect(px(half, 8 * tpx, x, y)).toEqual(px(whole, W, 8 * tpx + x, y));
    }
  });

  it('draws the river as one continuous channel', () => {
    const f = buildFields(grid());
    const tpx = 8, W = 16 * tpx;
    const buf = new Uint8Array(W * W * 4);
    paintRegion(f, 0, 0, 16, 16, tpx, buf);
    // Along the river row's center line every texel is water (bluer than green).
    for (let x = 6 * tpx; x < 15 * tpx; x += 3) {
      const c = px(buf, W, x, 8 * tpx + tpx / 2);
      expect(c[2]).toBeGreaterThan(c[1] - 10);
    }
  });

  it('whitens land under snow and follows the season', () => {
    const tpx = 4, W = 16 * tpx;
    const paint = (g: TerrainGrid) => { const b = new Uint8Array(W * W * 4); paintRegion(buildFields(g), 0, 0, 16, 16, tpx, b); return b; };
    const plain = paint(grid()), snowy = paint(grid({ snow: 255 })), autumn = paint(grid({ season: 'autumn' }));
    const at = (b: Uint8Array) => px(b, W, 8 * tpx, 3 * tpx);
    const lum = (c: number[]) => c[0] + c[1] + c[2];
    expect(lum(at(snowy))).toBeGreaterThan(lum(at(plain)) + 200);
    expect(at(autumn)[0]).toBeGreaterThan(at(plain)[0] + 15);
  });

  it('measures how far a region moved from the painting, however slowly', () => {
    const g = grid();
    const f = buildFields(g);
    const a = regionSnapshot(f, 0, 0, 8, 8), b = regionSnapshot(f, 8, 8, 8, 8);
    expect(regionDelta(f, a, 0, 0, 8, 8)).toBe(0);
    // A slow melt: each step small, the sum large (a quantized checksum missed it).
    for (let s = 0; s < 6; s++) {
      g.snow[2 * 16 + 2] = 20 + s * 8;
      buildFields(g, f);
    }
    const f2 = buildFields(g, f);
    expect(f2.data).toBe(f.data); // reused in place
    expect(regionDelta(f2, a, 0, 0, 8, 8)).toBeCloseTo(60 / 200, 5);
    expect(regionDelta(f2, b, 8, 8, 8, 8)).toBe(0);
    // Snow coming or going counts in full; colours in 64ths of their range.
    g.snow[2 * 16 + 2] = 250;
    expect(regionDelta(buildFields(g, f), a, 0, 0, 8, 8)).toBe(1);
    // Past the map edge the snapshot clamps, as the painter does.
    const edge = regionSnapshot(f, -1, -1, 4, 4);
    expect(regionDelta(f, edge, -1, -1, 4, 4)).toBe(0);
    expect([...edge.slice(0, 14)]).toEqual([...edge.slice(14 * 5, 14 * 6)]);
  });

  it('picks detail levels and base sizes', () => {
    expect(baseTpxFor(128, 128)).toBe(4);
    expect(baseTpxFor(1024, 512)).toBe(2);
    expect(baseTpxFor(4096, 4096)).toBe(1);
    expect(detailFor(4, 4)).toBe(0);
    expect(detailFor(8, 4)).toBe(8);
    expect(detailFor(21, 4)).toBe(16);
  });
});

describe('the light of the hour', () => {
  it('is neutral at noon, blue at night, warm at dusk', () => {
    expect(lightAt(12)).toEqual(LIGHT.day);
    expect(lightAt(2)).toEqual(LIGHT.night);
    const night = lightAt(23);
    expect(night[2]).toBeGreaterThan(night[0] + 40);
    const dusk = lightAt(19);
    expect(dusk[0]).toBeGreaterThan(dusk[2] + 40);
    expect(lightAt(36)).toEqual(lightAt(12));
  });
});

describe('chunk aprons and the quick base', () => {
  it('a chunk painted with a border of neighbouring tiles matches the plain painting inside', () => {
    const f = buildFields(grid());
    const tpx = 4;
    const plain = new Uint8Array(8 * tpx * 8 * tpx * 4);
    paintRegion(f, 4, 4, 8, 8, tpx, plain);
    const pw = 10 * tpx;
    const apron = new Uint8Array(pw * 10 * tpx * 4);
    paintRegion(f, 3, 3, 10, 10, tpx, apron);
    for (let y = 0; y < 8 * tpx; y += 5) {
      for (let x = 0; x < 8 * tpx; x += 3) {
        const a = ((y + tpx) * pw + x + tpx) * 4, p = (y * 8 * tpx + x) * 4;
        expect([...apron.slice(a, a + 4)]).toEqual([...plain.slice(p, p + 4)]);
      }
    }
    // A border past the map edge clamps (no throw, no holes).
    const edge = new Uint8Array(10 * tpx * 10 * tpx * 4);
    paintRegion(f, -1, -1, 10, 10, tpx, edge);
    expect(edge[3]).toBe(255);
  });

  it('spreads the quick painting smoothly (no blocks)', () => {
    // Two tiles, black and white: the texels between their centres ramp.
    const src = new Uint8Array([0, 0, 0, 255, 255, 255, 255, 255]);
    const out = new Uint8Array(2 * 4 * 1 * 4 * 4);
    upsample(src, 2, 1, 4, out);
    const row = Array.from({ length: 8 }, (_, x) => out[x * 4]);
    expect(row[0]).toBe(0);
    expect(row[7]).toBe(255);
    for (let x = 1; x < 8; x++) expect(row[x]).toBeGreaterThanOrEqual(row[x - 1]);
    expect(new Set(row).size).toBeGreaterThan(4);
  });
});
