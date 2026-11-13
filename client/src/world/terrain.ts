// The tile grid from `GET /api/terrain` (fixed layers plus the dynamic ones,
// refreshed from `GET /api/terrain/dynamic`) and its colors.
//
// Each layer arrives as base64, one byte per tile in row-major order. The
// map is painted one pixel per tile into an RGBA buffer that the world view
// shows as a nearest-neighbor texture, so every tile is a crisp square:
// biome and vegetation, the season's tint, drought, floods, ice and snow.

import type { TerrainDynamic, TerrainView } from '../protocol';

export const BIOMES = ['deep_water', 'shallow_water', 'marsh', 'grassland', 'forest', 'desert', 'tundra', 'mountain'] as const;
export type Biome = (typeof BIOMES)[number];
export const B = { deep: 0, shallow: 1, marsh: 2, grass: 3, forest: 4, desert: 5, tundra: 6, mountain: 7 } as const;

/** Wetness byte below which land is in drought (0.3 of 1). */
export const DROUGHT_BYTE = 77;

export interface TerrainGrid {
  width: number;
  height: number;
  version: number;
  biome: Uint8Array;
  elevation: Uint8Array;
  river: Uint8Array;
  /** Water depth at normal wetness, cm. */
  baseWater: Uint8Array;
  /** Current water depth, cm. */
  water: Uint8Array;
  ice: Uint8Array;
  snow: Uint8Array;
  veg: Uint8Array;
  wetness: Uint8Array;
  tick: number;
  day: number;
  hour: number;
  season: string;
}

export function decodeBase64(s: string): Uint8Array {
  const bin = atob(s);
  const out = new Uint8Array(bin.length);
  for (let i = 0; i < bin.length; i++) out[i] = bin.charCodeAt(i);
  return out;
}

function layer(s: string, n: number, name: string): Uint8Array {
  const v = decodeBase64(s);
  if (v.length !== n) throw new Error(`terrain layer ${name} has ${v.length} tiles, expected ${n}`);
  return v;
}

export function decodeTerrain(t: TerrainView): TerrainGrid {
  const n = t.width * t.height;
  if (!(n > 0)) throw new Error(`bad terrain size ${t.width}×${t.height}`);
  const g: TerrainGrid = {
    width: t.width,
    height: t.height,
    version: t.terrain_version,
    biome: layer(t.biome, n, 'biome'),
    elevation: layer(t.elevation, n, 'elevation'),
    river: layer(t.river, n, 'river'),
    baseWater: layer(t.base_water, n, 'base_water'),
    water: new Uint8Array(n),
    ice: new Uint8Array(n),
    snow: new Uint8Array(n),
    veg: new Uint8Array(n),
    wetness: new Uint8Array(n),
    tick: 0,
    day: 0,
    hour: 0,
    season: 'spring',
  };
  return applyDynamic(g, t.dynamic);
}

/** Replace the dynamic layers (ignores an update older than what we have). */
export function applyDynamic(g: TerrainGrid, d: TerrainDynamic): TerrainGrid {
  if (d.tick < g.tick) return g;
  const n = g.width * g.height;
  g.water = layer(d.water, n, 'water');
  g.ice = layer(d.ice, n, 'ice');
  g.snow = layer(d.snow, n, 'snow');
  g.veg = layer(d.veg, n, 'veg');
  g.wetness = layer(d.wetness, n, 'wetness');
  g.tick = d.tick;
  g.day = d.day;
  g.hour = d.hour;
  g.season = d.season;
  return g;
}

export type Rgb = [number, number, number];

const BASE: Record<number, Rgb> = {
  [B.deep]: [20, 50, 120],
  [B.shallow]: [40, 92, 165],
  [B.marsh]: [62, 96, 72],
  [B.grass]: [112, 152, 62],
  [B.forest]: [34, 92, 44],
  [B.desert]: [212, 192, 134],
  [B.tundra]: [150, 160, 140],
  [B.mountain]: [124, 114, 104],
};

/** How plant-covered biomes look in each season (multiplied in by `plantShare`). */
const SEASON_TINT: Record<string, Rgb> = {
  spring: [100, 175, 70],
  summer: [140, 160, 60],
  autumn: [168, 122, 52],
  winter: [118, 116, 96],
};

const mix = (a: Rgb, b: Rgb, t: number): Rgb => [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t];
const clamp = (v: number) => (v < 0 ? 0 : v > 255 ? 255 : Math.round(v));

/** Share of the season tint per biome. */
function plantShare(biome: number): number {
  return biome === B.grass ? 0.55 : biome === B.forest ? 0.3 : biome === B.marsh || biome === B.tundra ? 0.25 : 0;
}

/** Whether a tile shows open water (liquid or iced). */
export function isWater(g: TerrainGrid, i: number): boolean {
  return g.water[i] > 0;
}

/** Whether a dry-land tile is flooded (water where there normally is none, or a river well above its bed). */
export function isFlooded(g: TerrainGrid, i: number): boolean {
  return g.water[i] > 0 && g.biome[i] > B.shallow && (g.baseWater[i] === 0 || g.water[i] > g.baseWater[i] + 60);
}

/** The color of tile `i` (no lighting). */
export function tileColor(g: TerrainGrid, i: number): Rgb {
  const biome = g.biome[i];
  const water = g.water[i];
  let c: Rgb = BASE[biome] ?? [255, 0, 255];
  const land = biome > B.shallow;
  if (land) {
    const share = plantShare(biome);
    if (share > 0) c = mix(c, SEASON_TINT[g.season] ?? SEASON_TINT.spring, share);
    // Grazed-out land is browner.
    const veg = share > 0 ? g.veg[i] / 255 : 1;
    c = c.map((v) => v * (0.62 + 0.38 * veg)) as Rgb;
    // Drought: parched land yellows toward straw.
    const w = g.wetness[i];
    if (w < DROUGHT_BYTE && biome !== B.mountain) c = mix(c, [196, 170, 104], ((DROUGHT_BYTE - w) / DROUGHT_BYTE) * 0.75);
  }
  if (g.river[i] > 0) {
    // A river: bright blue, deeper when swollen; a dry bed shows as sand.
    if (water > 0) {
      const deep = Math.min(1, water / 100);
      c = [70 - 40 * deep, 140 - 60 * deep, 225 - 40 * deep];
    } else {
      c = [176, 160, 120];
    }
  } else if (land && g.baseWater[i] > 0 && water === 0) {
    // Dried-out marsh or lake shore: cracked mud.
    c = [150, 130, 95];
  } else if (!land && water === 0) {
    c = [150, 130, 95];
  } else if (!land) {
    // Lakes darken with depth.
    const deep = Math.min(1, water / 255);
    c = mix(c, [16, 40, 104], deep * 0.5);
  }
  if (isFlooded(g, i)) c = mix(c, [72, 108, 150], 0.7);
  if (water > 0 && g.ice[i] > 77) c = mix(c, [196, 220, 238], Math.min(1, g.ice[i] / 200));
  const snow = Math.min(1, g.snow[i] / 255) * 0.85;
  if (snow > 0) c = mix(c, [245, 247, 250], snow);
  return c;
}

/**
 * Paint every tile into `out` (RGBA, `width * height * 4` bytes) with
 * hill shading from the north-west.
 */
export function paintTerrain(g: TerrainGrid, out: Uint8Array): void {
  const { width: w, height: h } = g;
  if (out.length < w * h * 4) throw new Error('terrain buffer too small');
  for (let y = 0; y < h; y++) {
    for (let x = 0; x < w; x++) {
      const i = y * w + x;
      const c = tileColor(g, i);
      let light = 1;
      if (g.biome[i] > B.shallow && g.snow[i] < 128) {
        const nw = g.elevation[Math.max(0, y - 1) * w + Math.max(0, x - 1)];
        light = 1 + Math.max(-0.18, Math.min(0.18, (g.elevation[i] - nw) / 60));
        if (g.biome[i] === B.mountain) light *= 0.9 + 0.25 * (g.elevation[i] / 255);
      }
      const k = i * 4;
      out[k] = clamp(c[0] * light);
      out[k + 1] = clamp(c[1] * light);
      out[k + 2] = clamp(c[2] * light);
      out[k + 3] = 255;
    }
  }
}

/** Map-wide shares for the legend: land in drought, flooded land, snow and ice cover. */
export function terrainSummary(g: TerrainGrid): { drought: number; flood: number; snow: number; ice: number } {
  let land = 0, drought = 0, flood = 0, snow = 0, ice = 0, water = 0;
  for (let i = 0; i < g.biome.length; i++) {
    if (g.biome[i] > B.shallow) {
      land++;
      if (g.wetness[i] < DROUGHT_BYTE) drought++;
      if (g.snow[i] > 25) snow++;
    }
    if (isFlooded(g, i)) flood++;
    if (g.water[i] > 0) {
      water++;
      if (g.ice[i] > 77) ice++;
    }
  }
  return { drought: drought / Math.max(1, land), flood: flood / Math.max(1, land), snow: snow / Math.max(1, land), ice: ice / Math.max(1, water) };
}
