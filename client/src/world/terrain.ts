// The tile grid from `GET /api/terrain` (fixed layers plus the dynamic ones,
// refreshed from `GET /api/terrain/dynamic`) and its colors.
//
// Each layer arrives as base64, one byte per tile in row-major order. The
// colours per tile (biome and vegetation, the season's tint, drought,
// floods, ice and snow) are the input of the painted map (`terrainArt.ts`,
// which blends and textures them); `paintTerrain` keeps a one-pixel-per-tile
// summary of them for tests and debugging.

import type { TerrainDynamic, TerrainView } from '../protocol';
import { GROUND, ICE, mix, SEASON_GREEN, SNOW, WATER, type Rgb } from './palette';

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
  /** Most plant food per tile, 0..255 for none..the map's richest (absent from older servers). */
  vegMax?: Uint8Array;
  /** Current water depth, cm. */
  water: Uint8Array;
  ice: Uint8Array;
  snow: Uint8Array;
  veg: Uint8Array;
  wetness: Uint8Array;
  /** Ripe fruit, seeds and nuts, 0..255 (absent: none). */
  fruit?: Uint8Array;
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
    vegMax: t.veg_max ? layer(t.veg_max, n, 'veg_max') : undefined,
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
  g.fruit = d.fruit ? layer(d.fruit, n, 'fruit') : undefined;
  g.tick = d.tick;
  g.day = d.day;
  g.hour = d.hour;
  g.season = d.season;
  return g;
}

export type { Rgb } from './palette';

/** Ground colour per biome (spring, before vegetation and weather). */
const BASE: Record<number, Rgb> = {
  [B.deep]: GROUND.sand,
  [B.shallow]: GROUND.sand,
  [B.marsh]: GROUND.marsh,
  [B.grass]: GROUND.grass,
  [B.forest]: GROUND.forest,
  [B.desert]: GROUND.desert,
  [B.tundra]: GROUND.tundra,
  [B.mountain]: GROUND.mountain,
};

const clamp = (v: number) => (v < 0 ? 0 : v > 255 ? 255 : Math.round(v));

/** Share of the season tint per biome. */
function plantShare(biome: number): number {
  return biome === B.grass ? 0.6 : biome === B.forest ? 0.35 : biome === B.marsh || biome === B.tundra ? 0.25 : 0;
}

/** Whether a tile shows open water (liquid or iced). */
export function isWater(g: TerrainGrid, i: number): boolean {
  return g.water[i] > 0;
}

/** Whether a dry-land tile is flooded (water where there normally is none, or a river well above its bed). */
export function isFlooded(g: TerrainGrid, i: number): boolean {
  return g.water[i] > 0 && g.biome[i] > B.shallow && (g.baseWater[i] === 0 || g.water[i] > g.baseWater[i] + 60);
}

/**
 * The ground of tile `i` as if it were dry (no water, snow or lighting):
 * biome, season, grazing and drought. Under water it is the bed the shore
 * shows: sand under lakes, a bank along rivers, mud where a lake dried out.
 */
export function groundColor(g: TerrainGrid, i: number): Rgb {
  const biome = g.biome[i];
  if (g.river[i] > 0) return GROUND.bank;
  const land = biome > B.shallow;
  if (!land) return g.water[i] === 0 ? GROUND.mud : GROUND.sand;
  // Dried-out marsh or lake shore: cracked mud.
  if (g.baseWater[i] > 0 && g.water[i] === 0) return GROUND.mud;
  let c: Rgb = BASE[biome] ?? [255, 0, 255];
  const share = plantShare(biome);
  if (share > 0) {
    c = mix(c, SEASON_GREEN[g.season] ?? SEASON_GREEN.spring, share);
    // Grazed-out land is browner.
    c = mix(GROUND.bare, c, 0.45 + 0.55 * (g.veg[i] / 255));
    // Fruiting land (berries, mast, seed heads) takes a faint warm tint.
    const fruit = g.fruit?.[i] ?? 0;
    if (fruit > 0) c = mix(c, GROUND.fruit, Math.min(1, fruit / 200) * 0.22);
  }
  if (biome === B.mountain) c = mix(c, GROUND.peak, Math.max(0, (g.elevation[i] - 150) / 105) * 0.8);
  // Drought: parched land yellows toward straw.
  const w = g.wetness[i];
  if (w < DROUGHT_BYTE && biome !== B.mountain) c = mix(c, GROUND.straw, ((DROUGHT_BYTE - w) / DROUGHT_BYTE) * 0.75);
  return c;
}

/** The colour of the water on tile `i` (null when dry): rivers, lakes darkening with depth, flood water, ice. */
export function waterColor(g: TerrainGrid, i: number): Rgb | null {
  const water = g.water[i];
  if (water === 0) return null;
  let c: Rgb;
  if (isFlooded(g, i)) c = WATER.flood;
  else if (g.river[i] > 0) c = mix(WATER.river, WATER.deep, Math.min(1, water / 100) * 0.55);
  else {
    const deep = Math.min(1, water / 255);
    c = deep < 0.5 ? mix(WATER.shallow, WATER.deep, deep * 2) : mix(WATER.deep, WATER.abyss, (deep - 0.5) * 2);
  }
  if (g.ice[i] > 77) c = mix(c, ICE, Math.min(1, g.ice[i] / 200));
  return c;
}

/** The colour of tile `i` (no lighting). */
export function tileColor(g: TerrainGrid, i: number): Rgb {
  let c = waterColor(g, i) ?? groundColor(g, i);
  const snow = Math.min(1, g.snow[i] / 255) * 0.85;
  if (snow > 0) c = mix(c, SNOW, snow);
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

/**
 * Food on a tile now, 0..1 of the map's richest pasture: the plants left
 * (`veg`, a share of the tile's own maximum) times how much the tile can
 * hold (`vegMax`; without it, the share alone). 0 on water.
 */
export function foodAt(g: TerrainGrid, i: number): number {
  if (g.water[i] > 0) return 0;
  const share = g.veg[i] / 255;
  return g.vegMax ? share * (g.vegMax[i] / 255) : share;
}

/** Food bands of the map's Food layer (legend and colours). */
export const FOOD_BANDS: { upTo: number; label: string; rgb: Rgb; alpha: number }[] = [
  { upTo: 0.05, label: 'bare', rgb: [196, 74, 58], alpha: 0.55 },
  { upTo: 0.25, label: 'sparse', rgb: [214, 160, 64], alpha: 0.5 },
  { upTo: 0.55, label: 'fair', rgb: [168, 201, 88], alpha: 0.45 },
  { upTo: 1.01, label: 'rich', rgb: [64, 196, 92], alpha: 0.5 },
];

/**
 * The Food layer as one RGBA texel per tile: bands from bare (red) to rich
 * (green), transparent on water.
 */
export function foodPixels(g: TerrainGrid): Uint8Array {
  const n = g.width * g.height;
  const out = new Uint8Array(n * 4);
  for (let i = 0; i < n; i++) {
    if (g.water[i] > 0 || g.biome[i] <= B.shallow) continue;
    const f = foodAt(g, i);
    const band = FOOD_BANDS.find((b) => f < b.upTo) ?? FOOD_BANDS[FOOD_BANDS.length - 1];
    out[i * 4] = band.rgb[0];
    out[i * 4 + 1] = band.rgb[1];
    out[i * 4 + 2] = band.rgb[2];
    out[i * 4 + 3] = Math.round(band.alpha * 255);
  }
  return out;
}

