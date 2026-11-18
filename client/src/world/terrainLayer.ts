// The painted map on screen (`terrainArt.ts` paints, this shows): a base
// texture of the whole map at a few texels per tile, always there, and over
// it detail chunks (16×16 tiles at 8 or 16 texels per tile, by how many
// device pixels a tile covers) for the part on screen. Chunks are painted
// a few rows at a time within a per-frame budget (`paintBudget`) and kept in
// a bounded LRU cache. Until a chunk is ready the base shows through, so
// zooming in sharpens progressively instead of hitching.
//
// Repaints (playtests 1 and 2, #15: stale snow blocks, seams and a blocky
// patchwork after long sessions): every chunk keeps a copy of the fields it
// was painted from, and how far the map has moved on from it
// (`regionDelta`) decides whether and how soon it is repainted
// (`workOrder`): chunks never painted first, then big changes (snow coming
// or going, floods), then the oldest paintings. The budget grows with the
// frame time, so a slow machine still catches up with a fast clock instead
// of repainting the chunks round the screen center forever while the rest
// keep last season's snow.
//
// The light of the hour (palette.ts `lightAt`) tints the whole layer, eased.

import { BufferImageSource, Container, Rectangle, Sprite, Texture } from 'pixi.js';
import { baseTpxFor, buildFields, detailFor, paintRegion, regionDelta, regionSnapshot, upsample, type TerrainFields } from './terrainArt';
import type { TerrainGrid } from './terrain';
import { TextureCache } from './textureCache';
import { lightAt, toHex, type Rgb } from './palette';

/** Tiles per chunk side. */
export const CHUNK = 16;
/** Painting time per frame (ms) for chunks and base repaints at 60 fps and faster; at least one slice is painted per frame. */
export const TERRAIN_BUDGET_MS = 3;
/** The most painting time per frame (ms), on slow frames. */
export const TERRAIN_BUDGET_MAX_MS = 32;
/**
 * Painting time for a frame that took `dtMs`: a fifth of the frame, within
 * [`TERRAIN_BUDGET_MS`, `TERRAIN_BUDGET_MAX_MS`]. A fixed 3 ms is 180 ms of
 * painting a second at 60 fps but 36 at 12 (software WebGL, a loaded
 * machine), too little to repaint a screen of chunks between two terrain
 * refreshes of a fast world.
 */
export function paintBudget(dtMs: number): number {
  return Math.min(TERRAIN_BUDGET_MAX_MS, Math.max(TERRAIN_BUDGET_MS, (Number.isFinite(dtMs) ? dtMs : 0) * 0.2));
}
/** A chunk whose fields moved this far from its painting (`regionDelta`) is repainted when its turn comes. */
export const STALE = 0.03;
/** One whose fields moved this far (snow came or went, a flood) is repainted before the merely old ones. */
export const URGENT = 0.12;
/** Texels of chunk painted per slice (a slice is whole rows). */
const SLICE_TEXELS = 4096;
/** GPU and CPU memory the chunks may hold (bytes, RGBA). */
const CHUNK_BYTES = 24 * 1024 * 1024;
/** Easing of the hour's light (ms). */
const LIGHT_TAU_MS = 1500;
/**
 * Tiles of border painted round every chunk (and cropped by its texture
 * frame): the linear filter at a chunk's edge then blends with the real
 * neighbouring ground instead of the chunk's own clamped edge, so there is
 * no seam between chunks.
 */
export const APRON = 1;
/**
 * A repainted chunk waits to be shown with the other visible chunks being
 * repainted (no patchwork of old and new); never longer than this (ms), so a
 * map that keeps changing (fast clocks) can't hold old snow on screen forever.
 */
export const HOLD_MAX_MS = 600;

interface Chunk {
  key: string;
  cx: number;
  cy: number;
  tpx: number;
  tw: number;
  th: number;
  /** Texel rows of its painting (the apron included). */
  rows: number;
  buf: Uint8Array;
  source: BufferImageSource;
  texture: Texture;
  sprite: Sprite;
  /** The fields it was (or is being) painted from (`regionSnapshot`, apron included). */
  snap: Float32Array;
  /** How far the fields moved from `snap` (`regionDelta`), and the terrain generation it was measured at. */
  stale: number;
  gen: number;
  /** When its painting started (ms, `performance.now`). */
  paintedAt: number;
  /** Distance of its center from the screen center (tiles, this frame). */
  d: number;
  /** Next texel row to paint; `rows` when done. */
  row: number;
  /** Shown at least once (its texture holds a whole painting). */
  shown: boolean;
  /** Repainted, waiting to be shown together with the other visible chunks being repainted. */
  held: boolean;
  /** When it started waiting (ms, `performance.now`). */
  heldSince: number;
  /** Frames in a row it has been on screen far from the terrain (`URGENT`). */
  staleFrames: number;
}

export interface TerrainStats {
  width: number;
  height: number;
  /** Base texture texels per tile and size. */
  baseTpx: number;
  baseSize: [number, number];
  /** Current detail level (texels per tile, 0: base only). */
  detail: number;
  /** Chunks cached, shown this frame, waiting to be painted (or repainted). */
  chunks: number;
  shown: number;
  pending: number;
  /** Visible chunks not painted yet (the base or the other detail level shows there). */
  unpainted: number;
  /** Visible chunks showing a painting far from the map (`URGENT`), and the largest such distance. */
  stale: number;
  maxStale: number;
  /** The most frames in a row a chunk on screen has now been far from the terrain (none stays so: `workOrder`). */
  maxStaleFrames: number;
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
  private stats = { baseMs: 0, maxFrameMs: 0, paintMs: 0, fieldsMs: 0, texels: 0, pending: 0, unpainted: 0, stale: 0, maxStale: 0, maxStaleFrames: 0, frames: 0, slowFrames: 0, maxSliceMs: 0 };
  /** Painting time per frame instead of `paintBudget` (tests: a large one paints everything at once). */
  budgetOverride: number | null = null;
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
    upsample(quick, w, h, tpx, buf);
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
    const budget = this.budgetOverride ?? paintBudget(dtMs);
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
    const budgetLeft = () => !painted || performance.now() - start < budget;
    const work: Chunk[] = [];
    for (const { cx, cy, d } of want) {
      // Making a chunk (buffer, texture, snapshot) counts against the budget too.
      const fresh = !this.chunks.has(`${detail}:${cx}:${cy}`);
      if (fresh && !budgetLeft()) { pending++; this.standIn(cx, cy, detail); continue; }
      const c = this.chunk(cx, cy, detail);
      if (fresh) painted = true;
      c.d = d;
      this.visible.add(c);
      // How far the map moved from its painting (once per terrain update).
      if (c.gen !== this.gen) {
        c.gen = this.gen;
        c.stale = regionDelta(f, c.snap, c.cx * CHUNK - APRON, c.cy * CHUNK - APRON, c.tw + 2 * APRON, c.th + 2 * APRON);
      }
      if (c.row < c.rows || c.stale >= STALE) work.push(c);
    }
    // Paint in order of need (`workOrder`): a repaint keeps showing the old painting meanwhile.
    for (const c of workOrder(work)) {
      if (!budgetLeft()) break;
      if (c.row >= c.rows) this.startPaint(c);
      while (c.row < c.rows && budgetLeft()) {
        this.paintSlice(c);
        painted = true;
      }
    }
    // Show what is painted (this frame's first paintings too), the other detail level's painting or the base elsewhere.
    for (const c of [...this.visible]) {
      if (c.tpx !== detail) continue;
      c.sprite.visible = c.shown;
      if (!c.shown) this.standIn(c.cx, c.cy, detail);
    }
    let stale = 0, maxStale = 0, unpainted = pending;
    for (const c of work) {
      if (!c.shown) unpainted++;
      if (c.row < c.rows || c.stale >= STALE) pending++;
      if (c.shown && c.stale >= URGENT) { stale++; maxStale = Math.max(maxStale, c.stale); }
    }
    this.stats.stale = stale;
    this.stats.unpainted = unpainted;
    this.stats.maxStale = maxStale;
    let maxStaleFrames = 0;
    for (const c of this.visible) {
      if (c.tpx !== detail) continue;
      c.staleFrames = c.shown && c.stale >= URGENT ? c.staleFrames + 1 : 0;
      maxStaleFrames = Math.max(maxStaleFrames, c.staleFrames);
    }
    this.stats.maxStaleFrames = maxStaleFrames;
    // Then the base repaint, a slice at a time.
    while (this.baseRow !== null && budgetLeft()) {
      this.paintBaseSlice();
      painted = true;
    }
    // Repainted chunks show at once, when no visible one is still repainting
    // (or when one has waited too long).
    let repainting = false;
    let overdue = false;
    const now = performance.now();
    for (const c of this.visible) {
      if (c.shown && c.row < c.rows) repainting = true;
      if (c.held && now - c.heldSince > HOLD_MAX_MS) overdue = true;
    }
    if (!repainting || overdue) {
      for (const c of this.visible) {
        if (!c.held || c.row < c.rows) continue;
        c.held = false;
        c.source.update();
      }
    }
    if (this.baseRow !== null) pending++;
    // A repaint waiting to be shown is not on screen yet either.
    for (const c of this.visible) if (c.held) pending++;
    const ms = performance.now() - start;
    this.stats.paintMs += ms;
    this.stats.maxFrameMs = Math.max(this.stats.maxFrameMs, ms);
    if (painted) this.stats.frames++;
    if (ms > 2 * budget) this.stats.slowFrames++;
    this.stats.pending = pending;
  }

  /** Start (re)painting a chunk from the fields as they are now. */
  private startPaint(c: Chunk): void {
    regionSnapshot(this.fields!, c.cx * CHUNK - APRON, c.cy * CHUNK - APRON, c.tw + 2 * APRON, c.th + 2 * APRON, c.snap);
    // (`stale` stays that of the painting on screen until the new one is done.)
    c.gen = this.gen;
    c.row = 0;
    c.paintedAt = performance.now();
  }

  /** Until the chunk at `detail` is painted, show the same chunk at the other detail level if it is cached. */
  private standIn(cx: number, cy: number, detail: number): void {
    const other = detail === 8 ? 16 : 8;
    const key = `${other}:${cx}:${cy}`;
    if (!this.chunks.has(key)) return;
    const o = this.chunks.get(key, () => { throw new Error('unreachable'); });
    if (!o.shown) return;
    // Not a painting of another season: the base is better than that.
    if (o.gen !== this.gen) {
      o.gen = this.gen;
      o.stale = regionDelta(this.fields!, o.snap, o.cx * CHUNK - APRON, o.cy * CHUNK - APRON, o.tw + 2 * APRON, o.th + 2 * APRON);
    }
    if (o.stale >= URGENT) return;
    o.sprite.visible = true;
    this.visible.add(o);
  }

  private chunk(cx: number, cy: number, tpx: number): Chunk {
    const key = `${tpx}:${cx}:${cy}`;
    return this.chunks.get(key, () => {
      const f = this.fields!;
      const tw = Math.min(CHUNK, f.width - cx * CHUNK), th = Math.min(CHUNK, f.height - cy * CHUNK);
      // Painted with an apron of neighbouring tiles; the frame shows the chunk itself.
      const pw = (tw + 2 * APRON) * tpx, ph = (th + 2 * APRON) * tpx;
      const buf = new Uint8Array(pw * ph * 4);
      const source = new BufferImageSource({ resource: buf, width: pw, height: ph, scaleMode: 'linear' });
      const texture = new Texture({ source, frame: new Rectangle(APRON * tpx, APRON * tpx, tw * tpx, th * tpx) });
      const sprite = new Sprite(texture);
      sprite.position.set(cx * CHUNK, cy * CHUNK);
      sprite.scale.set(1 / tpx);
      sprite.visible = false;
      this.chunkLayer.addChild(sprite);
      return {
        key, cx, cy, tpx, tw, th, rows: ph, buf, source, texture, sprite, gen: this.gen, row: 0,
        snap: regionSnapshot(f, cx * CHUNK - APRON, cy * CHUNK - APRON, tw + 2 * APRON, th + 2 * APRON), stale: 0, paintedAt: performance.now(), d: 0,
        shown: false, held: false, heldSince: 0, staleFrames: 0,
      };
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
    const w = (c.tw + 2 * APRON) * c.tpx;
    const rows = Math.max(1, Math.floor(SLICE_TEXELS / w));
    const to = Math.min(c.rows, c.row + rows);
    paintRegion(this.fields!, c.cx * CHUNK - APRON, c.cy * CHUNK - APRON, c.tw + 2 * APRON, c.th + 2 * APRON, c.tpx, c.buf, c.row, to);
    this.stats.maxSliceMs = Math.max(this.stats.maxSliceMs, performance.now() - t0);
    this.stats.texels += (to - c.row) * w;
    c.row = to;
    if (to >= c.rows) {
      // How far the terrain is now from what it started from: not 0 when a
      // terrain update came in while it was being painted (its last rows are
      // then of the new terrain, the first of the old: repainted in turn).
      c.stale = regionDelta(this.fields!, c.snap, c.cx * CHUNK - APRON, c.cy * CHUNK - APRON, c.tw + 2 * APRON, c.th + 2 * APRON);
      c.gen = this.gen;
      if (c.shown) {
        // A repaint: shown with the others (no patchwork of old and new), or after `HOLD_MAX_MS`.
        if (!c.held) c.heldSince = performance.now();
        c.held = true;
      } else {
        c.source.update();
        c.shown = true;
      }
    }
  }

  /** The state of the chunks at `"cx,cy"` (both detail levels; tests and debugging). */
  chunkInfo(at: string): object[] {
    const out: object[] = [];
    for (const tpx of [8, 16]) {
      const key = `${tpx}:${at.replace(",", ":")}`;
      if (!this.chunks.has(key)) continue;
      const c = this.chunks.get(key, () => { throw new Error('unreachable'); });
      out.push({ tpx, row: c.row, rows: c.rows, shown: c.shown, held: c.held, stale: +c.stale.toFixed(3), gen: c.gen === this.gen, visible: c.sprite.visible, ageMs: Math.round(performance.now() - c.paintedAt) });
    }
    return out;
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
      pending: this.stats.pending, unpainted: this.stats.unpainted, stale: this.stats.stale, maxStale: this.stats.maxStale, maxStaleFrames: this.stats.maxStaleFrames, baseMs: this.stats.baseMs, maxFrameMs: this.stats.maxFrameMs, paintMs: this.stats.paintMs,
      fieldsMs: this.stats.fieldsMs, texels: this.stats.texels, frames: this.stats.frames, slowFrames: this.stats.slowFrames, maxSliceMs: this.stats.maxSliceMs,
    };
  }

  destroy(): void {
    this.chunks.clear();
    this.baseTex?.destroy(true);
    this.root.destroy({ children: true });
  }
}

/** What `workOrder` ranks a chunk by. */
export interface ChunkWork {
  /** Texel rows painted so far, of `rows`. */
  row: number;
  rows: number;
  /** Shown at least once. */
  shown: boolean;
  /** How far the map moved from its painting (`regionDelta`). */
  stale: number;
  /** When its painting started (ms). */
  paintedAt: number;
  /** Distance from the screen center (tiles). */
  d: number;
}

function rank(c: ChunkWork): number {
  if (!c.shown) return c.row > 0 ? 0 : 1; // never painted: the base shows there (finish one first)
  if (c.row < c.rows) return 0; // finish a repaint that was started
  if (c.stale >= URGENT) return 2; // last season's snow, an old flood
  if (c.stale >= STALE) return 3;
  return 4;
}

/**
 * The chunks to paint, most needed first: one already being painted, then
 * those never painted (nearest the center first), then those far from the
 * map (most changed first), then the merely out of date (oldest painting
 * first, so none waits forever while the ones round the center change again
 * and again). Chunks close enough to the map are left out.
 */
export function workOrder<T extends ChunkWork>(chunks: readonly T[]): T[] {
  return chunks
    .filter((c) => rank(c) < 4)
    .sort((a, b) => {
      const ra = rank(a), rb = rank(b);
      if (ra !== rb) return ra - rb;
      if (ra === 2 && a.stale !== b.stale) return b.stale - a.stale;
      if (ra === 3 && a.paintedAt !== b.paintedAt) return a.paintedAt - b.paintedAt;
      return a.d - b.d;
    });
}
