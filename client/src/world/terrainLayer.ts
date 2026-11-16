// The painted map on screen (`terrainArt.ts` paints, this shows): a base
// texture of the whole map at a few texels per tile, always there, and over
// it detail chunks (16×16 tiles at 8 or 16 texels per tile, by how many
// device pixels a tile covers) for the part on screen. Chunks are painted
// a few rows at a time within a per-frame budget, nearest the screen center
// first, kept in a bounded LRU cache, and repainted only when their tiles
// change. Until a chunk is ready the base shows through, so zooming in
// sharpens progressively instead of hitching.
//
// The light of the hour (palette.ts `lightAt`) tints the whole layer, eased.

import { BufferImageSource, Container, Sprite, Texture } from 'pixi.js';
import { baseTpxFor, buildFields, detailFor, paintRegion, regionHash, type TerrainFields } from './terrainArt';
import type { TerrainGrid } from './terrain';
import { TextureCache } from './textureCache';
import { lightAt, toHex, type Rgb } from './palette';

/** Tiles per chunk side. */
export const CHUNK = 16;
/** Painting time per frame (ms) for chunks and base repaints; at least one slice is painted per frame. */
export const TERRAIN_BUDGET_MS = 3;
/** Texels of chunk painted per slice (a slice is whole rows). */
const SLICE_TEXELS = 4096;
/** GPU and CPU memory the chunks may hold (bytes, RGBA). */
const CHUNK_BYTES = 24 * 1024 * 1024;
/** Easing of the hour's light (ms). */
const LIGHT_TAU_MS = 1500;

interface Chunk {
  key: string;
  cx: number;
  cy: number;
  tpx: number;
  tw: number;
  th: number;
  buf: Uint8Array;
  source: BufferImageSource;
  texture: Texture;
  sprite: Sprite;
  /** Fields checksum the chunk was (or is being) painted from, and the terrain generation it was checked at. */
  hash: number;
  gen: number;
  /** Next texel row to paint; `th * tpx` when done. */
  row: number;
  /** Shown at least once (its texture holds a whole painting). */
  shown: boolean;
  /** Repainted, waiting to be shown together with the other visible chunks being repainted. */
  held: boolean;
}

export interface TerrainStats {
  width: number;
  height: number;
  /** Base texture texels per tile and size. */
  baseTpx: number;
  baseSize: [number, number];
  /** Current detail level (texels per tile, 0: base only). */
  detail: number;
  /** Chunks cached, shown this frame, waiting to be painted. */
  chunks: number;
  shown: number;
  pending: number;
  /** Paint timings (ms): base (the quick first painting of a new map), slowest frame's painting, total, fields (last). */
  baseMs: number;
  maxFrameMs: number;
  paintMs: number;
  fieldsMs: number;
  /** Texels painted in all. */
  texels: number;
  /** Frames that painted, those over twice the budget, and the slowest single chunk slice (ms). */
  frames: number;
  slowFrames: number;
  maxSliceMs: number;
}

export class TerrainLayer {
  readonly root = new Container();
  private baseSprite = new Sprite();
  private chunkLayer = new Container();
  private fields: TerrainFields | null = null;
  private baseBuf: Uint8Array | null = null;
  private baseSource: BufferImageSource | null = null;
  private baseTex: Texture | null = null;
  private baseTpx = 4;
  /** Next base row to repaint (null: none pending). */
  private baseRow: number | null = null;
  /** The fields changed during a base repaint: repaint once more after it. */
  private baseAgain = false;
  private chunks: TextureCache<Chunk>;
  private visible = new Set<Chunk>();
  private detail = 0;
  /** Bumped on every terrain update (chunks re-check their tiles once per generation). */
  private gen = 0;
  private light: Rgb = [255, 255, 255];
  private stats = { baseMs: 0, maxFrameMs: 0, paintMs: 0, fieldsMs: 0, texels: 0, pending: 0, frames: 0, slowFrames: 0, maxSliceMs: 0 };
  /** Light the map as at this hour instead of the state's (tests, screenshots, debugging). */
  lightOverride: number | null = null;
  /** The grid painted (for tests and debugging). */
  grid: TerrainGrid | null = null;

  constructor() {
    this.chunks = new TextureCache<Chunk>({
      cost: (c) => c.buf.length,
      maxCost: CHUNK_BYTES,
      inUse: () => this.visible,
      dispose: (c) => {
        c.sprite.destroy();
        c.texture.destroy(true);
      },
    });
    this.root.addChild(this.baseSprite, this.chunkLayer);
  }

  /** A new grid, or new dynamic layers of the same one. */
  setTerrain(g: TerrainGrid): void {
    const t0 = performance.now();
    const sameSize = this.fields && this.fields.width === g.width && this.fields.height === g.height;
    this.grid = g;
    this.gen++;
    this.fields = buildFields(g, sameSize ? this.fields! : undefined);
    this.stats.fieldsMs = performance.now() - t0;
    if (!sameSize || !this.baseSource) {
      this.chunks.clear();
      this.makeBase(g.width, g.height);
      return;
    }
    // Same map, new weather: repaint the base progressively (a repaint under
    // way carries on with the new fields and then runs once more); chunks
    // repaint when their tiles changed (showing the old painting meanwhile).
    if (this.baseRow === null) this.baseRow = 0;
    else this.baseAgain = true;
  }

  private makeBase(w: number, h: number): void {
    const tpx = baseTpxFor(w, h);
    const t0 = performance.now();
    // A quick first painting at one texel per tile, spread over the texture
    // (the linear filter smooths it); the full painting follows a slice per
    // frame, so a new map costs a few ms instead of a long first frame.
    const W = w * tpx;
    const buf = new Uint8Array(W * h * tpx * 4);
    const quick = new Uint8Array(w * h * 4);
    paintRegion(this.fields!, 0, 0, w, h, 1, quick);
    const q32 = new Uint32Array(quick.buffer), b32 = new Uint32Array(buf.buffer);
    for (let y = 0; y < h * tpx; y++) {
      const qr = ((y / tpx) | 0) * w, br = y * W;
      for (let x = 0; x < W; x++) b32[br + x] = q32[qr + ((x / tpx) | 0)];
    }
    this.stats.baseMs = performance.now() - t0;
    this.stats.texels += w * h;
    const pot = (n: number) => (n & (n - 1)) === 0;
    const source = new BufferImageSource({
      resource: buf, width: w * tpx, height: h * tpx, scaleMode: 'linear',
      autoGenerateMipmaps: pot(w * tpx) && pot(h * tpx),
    });
    const tex = new Texture({ source });
    this.baseSprite.texture = tex;
    this.baseSprite.scale.set(1 / tpx);
    this.baseTex?.destroy(true);
    this.baseTex = tex;
    this.baseSource = source;
    this.baseBuf = buf;
    this.baseTpx = tpx;
    this.baseRow = tpx > 1 ? 0 : null;
    this.baseAgain = false;
  }

  /**
   * Per frame: show the chunks covering `view` (tiles) at the detail for
   * `devicePxPerTile`, paint within the budget, ease the light toward `hour`.
   */
  update(view: { x0: number; y0: number; x1: number; y1: number }, devicePxPerTile: number, hour: number | null, dtMs: number): void {
    if (this.lightOverride !== null) hour = this.lightOverride;
    if (hour !== null) {
      const target = lightAt(hour);
      const k = 1 - Math.exp(-dtMs / LIGHT_TAU_MS);
      this.light = this.light.map((v, i) => v + (target[i] - v) * k) as Rgb;
      this.root.tint = toHex(this.light);
    }
    const f = this.fields;
    if (!f) return;
    const start = performance.now();
    const detail = detailFor(devicePxPerTile, this.baseTpx);
    this.detail = detail;
    for (const c of this.visible) c.sprite.visible = false;
    this.visible.clear();
    // Visible chunks, nearest the center first.
    const want: { cx: number; cy: number; d: number }[] = [];
    if (detail > 0) {
      const cx0 = Math.max(0, Math.floor(view.x0 / CHUNK)), cx1 = Math.min(Math.ceil(f.width / CHUNK) - 1, Math.floor(view.x1 / CHUNK));
      const cy0 = Math.max(0, Math.floor(view.y0 / CHUNK)), cy1 = Math.min(Math.ceil(f.height / CHUNK) - 1, Math.floor(view.y1 / CHUNK));
      const mx = (view.x0 + view.x1) / 2, my = (view.y0 + view.y1) / 2;
      for (let cy = cy0; cy <= cy1; cy++) {
        for (let cx = cx0; cx <= cx1; cx++) want.push({ cx, cy, d: Math.hypot((cx + 0.5) * CHUNK - mx, (cy + 0.5) * CHUNK - my) });
      }
      want.sort((a, b) => a.d - b.d);
    }
    let painted = false;
    let pending = 0;
    // The base shows wherever chunks are not ready: one slice of its repaint every frame.
    if (this.baseRow !== null) {
      this.paintBaseSlice();
      painted = true;
    }
    const budgetLeft = () => !painted || performance.now() - start < TERRAIN_BUDGET_MS;
    for (const { cx, cy } of want) {
      // Making a chunk (buffer, texture, checksum) counts against the budget too.
      const fresh = !this.chunks.has(`${detail}:${cx}:${cy}`);
      if (fresh && !budgetLeft()) { pending++; this.standIn(cx, cy, detail); continue; }
      const c = this.chunk(cx, cy, detail);
      if (fresh) painted = true;
      this.visible.add(c);
      if (c.row >= c.th * c.tpx) {
        // Painted: repaint if its tiles changed (keep showing the old one meanwhile).
        if (c.gen !== this.gen) {
          c.gen = this.gen;
          const h = regionHash(f, c.cx * CHUNK, c.cy * CHUNK, c.tw, c.th);
          if (h !== c.hash) { c.hash = h; c.row = 0; }
        }
      }
      while (c.row < c.th * c.tpx && budgetLeft()) {
        this.paintSlice(c);
        painted = true;
      }
      if (c.row < c.th * c.tpx) pending++;
      c.sprite.visible = c.shown;
      if (!c.shown) this.standIn(cx, cy, detail);
    }
    // Then the base repaint, a slice at a time.
    while (this.baseRow !== null && budgetLeft()) {
      this.paintBaseSlice();
      painted = true;
    }
    // Repainted chunks show at once, when no visible one is still repainting.
    let repainting = false;
    for (const c of this.visible) if (c.shown && c.row < c.th * c.tpx) repainting = true;
    if (!repainting) {
      for (const c of this.visible) {
        if (!c.held) continue;
        c.held = false;
        c.source.update();
      }
    }
    if (this.baseRow !== null) pending++;
    const ms = performance.now() - start;
    this.stats.paintMs += ms;
    this.stats.maxFrameMs = Math.max(this.stats.maxFrameMs, ms);
    if (painted) this.stats.frames++;
    if (ms > 2 * TERRAIN_BUDGET_MS) this.stats.slowFrames++;
    this.stats.pending = pending;
  }

  /** Until the chunk at `detail` is painted, show the same chunk at the other detail level if it is cached. */
  private standIn(cx: number, cy: number, detail: number): void {
    const other = detail === 8 ? 16 : 8;
    const key = `${other}:${cx}:${cy}`;
    if (!this.chunks.has(key)) return;
    const o = this.chunks.get(key, () => { throw new Error('unreachable'); });
    if (!o.shown) return;
    o.sprite.visible = true;
    this.visible.add(o);
  }

  private chunk(cx: number, cy: number, tpx: number): Chunk {
    const key = `${tpx}:${cx}:${cy}`;
    return this.chunks.get(key, () => {
      const f = this.fields!;
      const tw = Math.min(CHUNK, f.width - cx * CHUNK), th = Math.min(CHUNK, f.height - cy * CHUNK);
      const buf = new Uint8Array(tw * tpx * th * tpx * 4);
      const source = new BufferImageSource({ resource: buf, width: tw * tpx, height: th * tpx, scaleMode: 'linear' });
      const texture = new Texture({ source });
      const sprite = new Sprite(texture);
      // A hair larger than the chunk so neighbours overlap instead of leaving a seam.
      sprite.position.set(cx * CHUNK, cy * CHUNK);
      sprite.scale.set((tw + 0.02) / (tw * tpx), (th + 0.02) / (th * tpx));
      sprite.visible = false;
      this.chunkLayer.addChild(sprite);
      return { key, cx, cy, tpx, tw, th, buf, source, texture, sprite, hash: regionHash(f, cx * CHUNK, cy * CHUNK, tw, th), gen: this.gen, row: 0, shown: false, held: false };
    });
  }

  private paintBaseSlice(): void {
    const f = this.fields!;
    const w = f.width * this.baseTpx;
    const rows = Math.max(1, Math.floor(SLICE_TEXELS / w));
    const from = this.baseRow!;
    const to = Math.min(f.height * this.baseTpx, from + rows);
    paintRegion(f, 0, 0, f.width, f.height, this.baseTpx, this.baseBuf!, from, to);
    this.stats.texels += (to - from) * w;
    this.baseRow = to;
    if (to >= f.height * this.baseTpx) {
      this.baseSource!.update();
      this.baseRow = this.baseAgain ? 0 : null;
      this.baseAgain = false;
    }
  }

  private paintSlice(c: Chunk): void {
    const t0 = performance.now();
    const w = c.tw * c.tpx;
    const rows = Math.max(1, Math.floor(SLICE_TEXELS / w));
    const to = Math.min(c.th * c.tpx, c.row + rows);
    paintRegion(this.fields!, c.cx * CHUNK, c.cy * CHUNK, c.tw, c.th, c.tpx, c.buf, c.row, to);
    this.stats.maxSliceMs = Math.max(this.stats.maxSliceMs, performance.now() - t0);
    this.stats.texels += (to - c.row) * w;
    c.row = to;
    if (to >= c.th * c.tpx) {
      if (c.shown) c.held = true; // a repaint: shown with the others (no patchwork of old and new)
      else {
        c.source.update();
        c.shown = true;
      }
    }
  }

  /** The base painting, RGBA (for tests and debugging). */
  baseBuffer(): Uint8Array | null {
    return this.baseBuf;
  }

  terrainStats(): TerrainStats {
    const f = this.fields;
    const tpx = this.baseTpx;
    return {
      width: f?.width ?? 0, height: f?.height ?? 0, baseTpx: tpx, baseSize: [this.baseTex?.width ?? 0, this.baseTex?.height ?? 0],
      detail: this.detail, chunks: this.chunks.size, shown: [...this.visible].filter((c) => c.sprite.visible).length,
      pending: this.stats.pending, baseMs: this.stats.baseMs, maxFrameMs: this.stats.maxFrameMs, paintMs: this.stats.paintMs,
      fieldsMs: this.stats.fieldsMs, texels: this.stats.texels, frames: this.stats.frames, slowFrames: this.stats.slowFrames, maxSliceMs: this.stats.maxSliceMs,
    };
  }

  destroy(): void {
    this.chunks.clear();
    this.baseTex?.destroy(true);
    this.root.destroy({ children: true });
  }
}
