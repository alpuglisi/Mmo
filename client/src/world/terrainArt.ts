// The painted map: turns the tile grid into soft, textured ground at any
// resolution (texels per tile), PixiJS-free so it is unit-tested and timed
// without a renderer. See STYLE.md for the look.
//
// Two steps:
//  1. `buildFields`: per tile, a handful of floats (ground and water colour
//     with hill shading from the top-left, wetness, snow, ice and how much
//     grain, canopy and rock texture the biome has). Cheap: once per terrain
//     update, O(tiles).
//  2. `paintRegion`: per texel, the fields sampled bilinearly at a slightly
//     warped position (so blends and shorelines follow organic curves rather
//     than the tile grid), then water or ground decided by the smoothed
//     wetness, then procedural detail from two small tileable noise tables:
//     grain on grass and sand, embossed canopy blobs in forests, strata on
//     rock, ripples and a foam line on water, cracks on ice, drifts of snow.
//     A texel is a pure function of its world position, so chunks painted
//     separately (and at different resolutions) meet without seams.

import { B, type TerrainGrid } from './terrain';
import { groundColor, waterColor } from './terrain';
import { WATER, SNOW, SNOW_SHADE, type Rgb } from './palette';

/** Floats per tile in `TerrainFields.data`. */
const F = 14;
const enum K { R, G, Bl, WR, WG, WB, Wet, Snow, Ice, Grain, Canopy, Rock, Chan, Dune }

export interface TerrainFields {
  width: number;
  height: number;
  /** `F` floats per tile, row-major. */
  data: Float32Array;
}

const NOISE = 256;
const NMASK = NOISE - 1;
let noiseA: Float32Array | null = null;
let noiseB: Float32Array | null = null;

/** Deterministic hash → [0, 1). */
function hash(x: number, y: number, seed: number): number {
  let h = (x * 374761393 + y * 668265263 + seed * 2147483647) | 0;
  h = Math.imul(h ^ (h >>> 13), 1274126177);
  h ^= h >>> 16;
  return (h >>> 0) / 4294967296;
}

/** Tileable value noise of `NOISE`² summed over octaves of the given periods (pixels), normalized to 0..1. */
function fbm(periods: number[], seed: number): Float32Array {
  const out = new Float32Array(NOISE * NOISE);
  let total = 0;
  periods.forEach((p, o) => {
    const amp = 1 / (o + 1) ** 0.9;
    total += amp;
    const cells = NOISE / p;
    // The lattice once per octave, then smooth interpolation.
    const lat = new Float32Array(cells * cells);
    for (let j = 0; j < cells; j++) for (let i = 0; i < cells; i++) lat[j * cells + i] = hash(i, j, seed + o);
    for (let y = 0; y < NOISE; y++) {
      const fy = y / p, y0 = Math.floor(fy), ty = fy - y0, sy = ty * ty * (3 - 2 * ty);
      const r0 = (y0 % cells) * cells, r1 = ((y0 + 1) % cells) * cells;
      for (let x = 0; x < NOISE; x++) {
        const fx = x / p, x0 = Math.floor(fx), tx = fx - x0, sx = tx * tx * (3 - 2 * tx);
        const c0 = x0 % cells, c1 = (x0 + 1) % cells;
        const a = lat[r0 + c0], b = lat[r0 + c1], c = lat[r1 + c0], d = lat[r1 + c1];
        out[y * NOISE + x] += amp * (a + (b - a) * sx + (c - a + (a - b + d - c) * sx) * sy);
      }
    }
  });
  let lo = Infinity, hi = -Infinity;
  for (let i = 0; i < out.length; i++) { out[i] /= total; lo = Math.min(lo, out[i]); hi = Math.max(hi, out[i]); }
  for (let i = 0; i < out.length; i++) out[i] = (out[i] - lo) / (hi - lo);
  return out;
}

function noises(): [Float32Array, Float32Array] {
  // Macro (warp, patches, snow drifts) and fine (grain, canopy, ripples).
  noiseA ??= fbm([64, 32, 16, 8], 11);
  noiseB ??= fbm([8, 4, 2], 23);
  return [noiseA, noiseB];
}

/** Noise pixels per tile: macro noise repeats every 16 tiles, fine noise every 256/24 ≈ 10.7. */
const A_PER_TILE = 16;
const B_PER_TILE = 24;

/** Texture per biome: [grain, canopy, rock], indexed by biome id. */
const BIOME_TEXTURE: readonly (readonly [number, number, number])[] = (() => {
  const t: [number, number, number][] = [];
  for (let b = 0; b < 8; b++) {
    t[b] = b === B.grass ? [0.16, 0, 0] : b === B.forest ? [0.06, 1, 0] : b === B.marsh ? [0.14, 0.35, 0]
      : b === B.desert ? [0.1, 0, 0] : b === B.tundra ? [0.12, 0, 0.25] : b === B.mountain ? [0.08, 0, 1] : [0.08, 0, 0];
  }
  return t;
})();
const NO_TEXTURE = [0.08, 0, 0] as const;

/** A 3×3 binomial blur ([1 2 1] each way, separable; edges renormalized). */
function blur(src: Float32Array, w: number, h: number): Float32Array {
  const tmp = new Float32Array(src.length);
  const out = new Float32Array(src.length);
  for (let y = 0; y < h; y++) {
    const r = y * w;
    for (let x = 0; x < w; x++) {
      const l = x > 0, rr = x < w - 1;
      tmp[r + x] = (2 * src[r + x] + (l ? src[r + x - 1] : 0) + (rr ? src[r + x + 1] : 0)) / (2 + (l ? 1 : 0) + (rr ? 1 : 0));
    }
  }
  for (let y = 0; y < h; y++) {
    const u = y > 0, dn = y < h - 1, k = 1 / (2 + (u ? 1 : 0) + (dn ? 1 : 0));
    const r = y * w;
    for (let x = 0; x < w; x++) out[r + x] = (2 * tmp[r + x] + (u ? tmp[r + x - w] : 0) + (dn ? tmp[r + x + w] : 0)) * k;
  }
  return out;
}

/** Per-tile inputs of the painter (see the file comment). */
export function buildFields(g: TerrainGrid, prev?: TerrainFields): TerrainFields {
  const { width: w, height: h } = g;
  const n = w * h;
  const data = prev && prev.width === w && prev.height === h ? prev.data : new Float32Array(n * F);
  const shore = WATER.shore;
  for (let y = 0; y < h; y++) {
    for (let x = 0; x < w; x++) {
      const i = y * w + x;
      const o = i * F;
      const ground = groundColor(g, i);
      // Hill shading: the slope toward the light (top-left), central differences.
      const e = g.elevation;
      const dx = e[y * w + Math.min(w - 1, x + 1)] - e[y * w + Math.max(0, x - 1)];
      const dy = e[Math.min(h - 1, y + 1) * w + x] - e[Math.max(0, y - 1) * w + x];
      const mountain = g.biome[i] === B.mountain;
      let light = 1 + Math.max(-0.22, Math.min(0.22, (dx + dy) / (mountain ? 90 : 160)));
      if (mountain) light *= 0.94 + 0.16 * (e[i] / 255);
      data[o + K.R] = ground[0] * light;
      data[o + K.G] = ground[1] * light;
      data[o + K.Bl] = ground[2] * light;
      const wc: Rgb = waterColor(g, i) ?? shore;
      data[o + K.WR] = wc[0];
      data[o + K.WG] = wc[1];
      data[o + K.WB] = wc[2];
      data[o + K.Snow] = Math.min(1, g.snow[i] / 200);
      data[o + K.Ice] = g.water[i] > 0 && g.ice[i] > 77 ? Math.min(1, g.ice[i] / 200) : 0;
      const [grain, canopy, rock] = BIOME_TEXTURE[g.biome[i]] ?? NO_TEXTURE;
      const veg = g.veg[i] / 255;
      data[o + K.Grain] = grain;
      // Thinned forests show fewer, flatter crowns.
      data[o + K.Canopy] = canopy * (0.35 + 0.65 * veg);
      data[o + K.Rock] = rock;
      data[o + K.Dune] = g.biome[i] === B.desert ? 1 : 0;
    }
  }
  // Water: lakes as a blurred mask (rounded shores, no staircase along
  // diagonals); rivers and ponds too small for the blur as a sharp channel mask, linked
  // across diagonal steps so a river reads as one line, not beads.
  const raw = new Float32Array(n);
  for (let i = 0; i < n; i++) raw[i] = g.water[i] > 0 ? 1 : 0;
  const blurred = blur(blur(raw, w, h), w, h);
  const chan = (x: number, y: number) => x >= 0 && y >= 0 && x < w && y < h && g.water[y * w + x] > 0 && g.river[y * w + x] > 0;
  for (let y = 0; y < h; y++) {
    for (let x = 0; x < w; x++) {
      const i = y * w + x;
      data[i * F + K.Wet] = blurred[i];
      let c = 0;
      if (g.water[i] > 0) {
        c = g.river[i] > 0 ? 1 : blurred[i] < 0.6 ? 0.8 : 0; // a river, or a pond the blur would erase
      } else {
        // A land tile in the inner corner of a diagonal step of a river carries a little of the channel.
        for (let k = 0; k < 4; k++) {
          const dx = k & 1 ? 1 : -1, dy = k & 2 ? 1 : -1;
          if (chan(x + dx, y) && chan(x, y + dy) && !chan(x + dx, y + dy)) c = 0.36;
        }
      }
      data[i * F + K.Chan] = c;
    }
  }
  // Water colour: one blur pass, so depth changes as soft gradients, not blotches.
  const ch = new Float32Array(n);
  for (const k of [K.WR, K.WG, K.WB]) {
    for (let i = 0; i < n; i++) ch[i] = data[i * F + k];
    const b = blur(ch, w, h);
    for (let i = 0; i < n; i++) data[i * F + k] = b[i];
  }
  return { width: w, height: h, data };
}

/**
 * A checksum of the fields of a rectangle of tiles (to find chunks that
 * changed), quantized so that slow drift (a little grazing, a little
 * regrowth) does not repaint every chunk at every terrain refresh: colours
 * to steps of 4 of 255, the other fields to 1/24.
 */
export function regionHash(f: TerrainFields, x0: number, y0: number, tw: number, th: number): number {
  let hsh = 2166136261;
  const d = f.data;
  for (let y = y0; y < Math.min(f.height, y0 + th); y++) {
    for (let x = x0; x < Math.min(f.width, x0 + tw); x++) {
      const o = (y * f.width + x) * F;
      for (let k = 0; k < F; k++) hsh = Math.imul(hsh ^ Math.round(k <= K.WB ? d[o + k] / 4 : d[o + k] * 24), 16777619);
    }
  }
  return hsh >>> 0;
}

const smooth = (a: number, b: number, t: number) => {
  const x = t <= a ? 0 : t >= b ? 1 : (t - a) / (b - a);
  return x * x * (3 - 2 * x);
};

/**
 * Paint the tiles `[x0, x0 + tw) × [y0, y0 + th)` at `tpx` texels per tile
 * into `out` (RGBA, `(tw * tpx) × (th * tpx)`), rows `rowFrom` to `rowTo`
 * (texel rows, for painting a region over several frames).
 */
export function paintRegion(f: TerrainFields, x0: number, y0: number, tw: number, th: number, tpx: number, out: Uint8Array,
  rowFrom = 0, rowTo = th * tpx): void {
  const [na, nb] = noises();
  const W = tw * tpx;
  if (out.length < W * th * tpx * 4) throw new Error('terrain chunk buffer too small');
  const { width: fw, height: fh, data: d } = f;
  // Fine detail fades out at coarse resolutions (it would only alias).
  const detail = Math.min(1, tpx / 12);
  const inv = 1 / tpx;
  const xStep = fw > 1 ? F : 0, yStep = fh > 1 ? fw * F : 0;
  const wetAt = (u: number, w: number): number => {
    // (Lakes only: river banks cast no shadow worth drawing.)
    let fx = u - 0.5, fy = w - 0.5;
    if (fx < 0) fx = 0; else if (fx > fw - 1) fx = fw - 1;
    if (fy < 0) fy = 0; else if (fy > fh - 1) fy = fh - 1;
    const ix = Math.min(fw - 2, Math.floor(fx)), iy = Math.min(fh - 2, Math.floor(fy));
    const ax = fw > 1 ? fx - ix : 0, ay = fh > 1 ? fy - iy : 0;
    const o = (Math.max(0, iy) * fw + Math.max(0, ix)) * F + K.Wet;
    const x1 = fw > 1 ? F : 0, y1 = fh > 1 ? fw * F : 0;
    return (d[o] * (1 - ax) + d[o + x1] * ax) * (1 - ay) + (d[o + y1] * (1 - ax) + d[o + y1 + x1] * ax) * ay;
  };
  for (let ty = rowFrom; ty < rowTo; ty++) {
    const wy = y0 + (ty + 0.5) * inv;
    for (let tx = 0; tx < W; tx++) {
      const wx = x0 + (tx + 0.5) * inv;
      // Noise lookups (nearest; the tables are smooth at these scales).
      const ax = (wx * A_PER_TILE) | 0, ay = (wy * A_PER_TILE) | 0;
      const nA = na[(ay & NMASK) * NOISE + (ax & NMASK)];
      const nA2 = na[((ay + 97) & NMASK) * NOISE + ((ax + 151) & NMASK)];
      const bx = (wx * B_PER_TILE) | 0, by = (wy * B_PER_TILE) | 0;
      const nB = nb[(by & NMASK) * NOISE + (bx & NMASK)];
      // Domain warp: blends and shores meander instead of following the grid.
      const su = wx + (nA - 0.5) * 0.9, sv = wy + (nA2 - 0.5) * 0.9;
      // Bilinear footprint between tile centers (clamped at the map edge), inline for speed.
      let fx = su - 0.5, fy = sv - 0.5;
      if (fx < 0) fx = 0; else if (fx > fw - 1) fx = fw - 1;
      if (fy < 0) fy = 0; else if (fy > fh - 1) fy = fh - 1;
      const ix = fx >= fw - 1 ? Math.max(0, fw - 2) : fx | 0, iy = fy >= fh - 1 ? Math.max(0, fh - 2) : fy | 0;
      const fax = fw > 1 ? fx - ix : 0, fay = fh > 1 ? fy - iy : 0;
      const o00 = (iy * fw + ix) * F, o10 = o00 + xStep, o01 = o00 + yStep, o11 = o01 + xStep;
      const w00 = (1 - fax) * (1 - fay), w10 = fax * (1 - fay), w01 = (1 - fax) * fay, w11 = fax * fay;
      // The tile whose texture shows: the nearest one to a more strongly
      // warped point (the macro noise, about ±0.8 tile), so biome
      // borders wander instead of tracing the grid; colours blend.
      const tu = fx + (na[((ay + 17) & NMASK) * NOISE + ((ax + 201) & NMASK)] - 0.5) * 1.6 + 0.5;
      const tv = fy + (na[((ay + 133) & NMASK) * NOISE + ((ax + 9) & NMASK)] - 0.5) * 1.6 + 0.5;
      const nx = tu < 0 ? 0 : tu >= fw ? fw - 1 : tu | 0, ny = tv < 0 ? 0 : tv >= fh ? fh - 1 : tv | 0;
      const near = (ny * fw + nx) * F;
      const wet = Math.max((d[o00 + K.Wet] * w00 + d[o10 + K.Wet] * w10 + d[o01 + K.Wet] * w01 + d[o11 + K.Wet] * w11) + (nB - 0.5) * 0.12 * detail + (nA - 0.5) * 0.1, (d[o00 + K.Chan] * w00 + d[o10 + K.Chan] * w10 + d[o01 + K.Chan] * w01 + d[o11 + K.Chan] * w11) + (nA - 0.5) * 0.06);
      let r: number, g: number, b: number;
      // Ground.
      // Half the blended colour, half the textured tile's, so colour and texture borders agree.
      r = 0.5 * (d[o00 + K.R] * w00 + d[o10 + K.R] * w10 + d[o01 + K.R] * w01 + d[o11 + K.R] * w11) + 0.5 * d[near + K.R];
      g = 0.5 * (d[o00 + K.G] * w00 + d[o10 + K.G] * w10 + d[o01 + K.G] * w01 + d[o11 + K.G] * w11) + 0.5 * d[near + K.G];
      b = 0.5 * (d[o00 + K.Bl] * w00 + d[o10 + K.Bl] * w10 + d[o01 + K.Bl] * w01 + d[o11 + K.Bl] * w11) + 0.5 * d[near + K.Bl];
      let shade = 1 + d[near + K.Grain] * (nB - 0.5) * 2 * detail + (nA - 0.5) * 0.08;
      const canopy = d[near + K.Canopy], rock = d[near + K.Rock], dune = d[near + K.Dune];
      if (canopy > 0.02) {
        // Tree crowns: blobs of the fine noise, lit from the top-left (emboss).
        // Crowns about half a tile across (the fine noise at half its scale).
        const cx = (wx * 13) | 0, cy = (wy * 13) | 0;
        const c0 = nb[((cy + 64) & NMASK) * NOISE + ((cx + 64) & NMASK)], c1 = nb[((cy + 63) & NMASK) * NOISE + ((cx + 63) & NMASK)];
        const crown = smooth(0.3, 0.7, c0);
        shade += canopy * ((c0 - c1) * 1.5 * detail + (crown - 0.5) * 0.3 + (nB - 0.5) * 0.08 * detail);
      }
      if (rock > 0.02) {
        // Relief: the macro noise embossed (lit from the top-left) into ridges.
        // Ridged: 1 - |2n - 1| folds the noise into crests.
        const rx = (wx * 8) | 0, ry = (wy * 8) | 0;
        const r0 = 1 - Math.abs(2 * na[((ry + 40) & NMASK) * NOISE + ((rx + 40) & NMASK)] - 1);
        const r1 = 1 - Math.abs(2 * na[((ry + 39) & NMASK) * NOISE + ((rx + 39) & NMASK)] - 1);
        const nA3 = na[(((ay - 1) & NMASK) * NOISE) + ((ax - 1) & NMASK)];
        shade += rock * ((r0 - r1) * 2.4 + (r0 - 0.6) * 0.3 + (nA - nA3) * 1.6 * detail + (nB - 0.5) * 0.06 * detail);
      }
      if (dune > 0.02) {
        // Dunes: soft wind ripples, wavering with the macro noise.
        const s = Math.sin((wx * 0.7 + wy * 2.4 + nA * 5) * 3.2);
        shade += dune * s * 0.045 * Math.min(1, detail * 1.5);
      }
      r *= shade; g *= shade; b *= shade;
      // Damp ground along the water's edge.
      if (wet > 0.32) {
        const damp = smooth(0.32, 0.5, wet) * 0.22;
        r *= 1 - damp; g *= 1 - damp * 0.8; b *= 1 - damp * 0.6;
      }
      // Snow drifts.
      const snow = (d[o00 + K.Snow] * w00 + d[o10 + K.Snow] * w10 + d[o01 + K.Snow] * w01 + d[o11 + K.Snow] * w11) + (nA - 0.5) * 0.45 + (nB - 0.5) * 0.05 * detail;
      if (snow > 0.42 && wet < 0.55) {
        const k = smooth(0.42, 0.56, snow) * 0.96;
        const lit = smooth(-0.3, 0.3, (nA - na[(((ay - 3) & NMASK) * NOISE) + ((ax - 3) & NMASK)]) * 6);
        const sr = SNOW_SHADE[0] + (SNOW[0] - SNOW_SHADE[0]) * lit, sg = SNOW_SHADE[1] + (SNOW[1] - SNOW_SHADE[1]) * lit, sb = SNOW_SHADE[2] + (SNOW[2] - SNOW_SHADE[2]) * lit;
        r += (sr - r) * k; g += (sg - g) * k; b += (sb - b) * k;
      }
      if (wet > 0.46) {
        // Water: the colour deepens away from the shore; ripples; a foam line.
        let wr = (d[o00 + K.WR] * w00 + d[o10 + K.WR] * w10 + d[o01 + K.WR] * w01 + d[o11 + K.WR] * w11), wg = (d[o00 + K.WG] * w00 + d[o10 + K.WG] * w10 + d[o01 + K.WG] * w01 + d[o11 + K.WG] * w11), wb = (d[o00 + K.WB] * w00 + d[o10 + K.WB] * w10 + d[o01 + K.WB] * w01 + d[o11 + K.WB] * w11);
        const inner = smooth(0.5, 0.95, wet);
        const sh = WATER.shore;
        wr = sh[0] + (wr - sh[0]) * (0.35 + 0.65 * inner);
        wg = sh[1] + (wg - sh[1]) * (0.35 + 0.65 * inner);
        wb = sh[2] + (wb - sh[2]) * (0.35 + 0.65 * inner);
        const rip = 1 + (nB - 0.5) * 0.07 * detail + (nA - 0.5) * 0.05;
        wr *= rip; wg *= rip; wb *= rip;
        // The bank's shadow on the water (light from the top-left).
        if (wetAt(su - 0.22, sv - 0.22) < 0.5) { wr *= 0.86; wg *= 0.88; wb *= 0.9; }
        const ice = (d[o00 + K.Ice] * w00 + d[o10 + K.Ice] * w10 + d[o01 + K.Ice] * w01 + d[o11 + K.Ice] * w11) + (nA - 0.5) * 0.3;
        if (ice > 0.45) {
          const k = smooth(0.45, 0.6, ice);
          const crack = Math.abs(nB - 0.5) < 0.025 * detail ? 0.82 : 1;
          wr += (232 * crack - wr) * k; wg += (240 * crack - wg) * k; wb += (246 * crack - wb) * k;
        } else {
          const foam = smooth(0.5, 0.52, wet) * (1 - smooth(0.52, 0.56, wet)) * 0.5;
          const fo = WATER.foam;
          wr += (fo[0] - wr) * foam; wg += (fo[1] - wg) * foam; wb += (fo[2] - wb) * foam;
        }
        const k = smooth(0.46, 0.5, wet);
        r += (wr - r) * k; g += (wg - g) * k; b += (wb - b) * k;
      }
      const o = (ty * W + tx) * 4;
      out[o] = r < 0 ? 0 : r > 255 ? 255 : r;
      out[o + 1] = g < 0 ? 0 : g > 255 ? 255 : g;
      out[o + 2] = b < 0 ? 0 : b > 255 ? 255 : b;
      out[o + 3] = 255;
    }
  }
}

/** Detail level for a zoom: texels per tile of the chunks, or 0 for the base only. */
export function detailFor(devicePxPerTile: number, baseTpx: number): number {
  if (devicePxPerTile <= baseTpx * 1.6) return 0;
  return devicePxPerTile <= 11 ? 8 : 16;
}

/** Texels per tile of the base texture: up to 4, within a 2048-texel side. */
export function baseTpxFor(w: number, h: number): number {
  return Math.max(1, Math.min(4, Math.floor(2048 / Math.max(w, h))));
}
