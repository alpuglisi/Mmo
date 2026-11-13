// The map and organisms, drawn with PixiJS.
//
// Two layers: the map (the tile grid from /api/terrain painted one pixel per
// tile into a nearest-neighbor texture, one world unit per tile, plus
// overlays in world units) moves with the camera; the organism markers live
// in screen space so they stay readable at any zoom. Positions glide toward
// each new `state` so 1 s state pushes don't jump.

import { Application, BufferImageSource, Container, Graphics, Sprite, Texture } from 'pixi.js';
import type { NearbyOrganism, OrgId, OwnOrganism, PlayerId, StateView } from '../protocol';
import { OWN_COLOR, ownerColor } from './colors';
import { paintTerrain, type TerrainGrid } from './terrain';

export interface Camera {
  /** World point (tiles) at the center of the screen. */
  x: number;
  y: number;
  /** Screen pixels per tile. */
  zoom: number;
}

export interface WorldClick {
  x: number;
  y: number;
  organism: OrgId | null;
}

interface Marker {
  g: Graphics;
  /** Displayed position (world), easing toward the target. */
  x: number;
  y: number;
  tx: number;
  ty: number;
  style: string;
  seen: boolean;
}

const MIN_ZOOM = 1;
const MAX_ZOOM = 48;
const HIT_PX = 12;
/** Zoom (pixels per tile) from which tile edges are outlined. */
const GRID_ZOOM = 18;

export class WorldView {
  readonly app = new Application();
  private mapLayer = new Container();
  private mapSprite = new Sprite();
  private overlay = new Graphics();
  private markerLayer = new Container();
  private markers = new Map<OrgId, Marker>();
  private highlight = new Graphics();
  private texture: Texture | null = null;
  private terrainBuf: Uint8Array | null = null;
  private terrainSource: BufferImageSource | null = null;
  /** The tile grid on screen (null until the first terrain arrives). */
  terrain: TerrainGrid | null = null;
  private worldW = 64;
  private worldH = 64;
  private state: StateView | null = null;
  private moveTarget: { x: number; y: number } | null = null;
  selected: OrgId | null = null;
  cam: Camera = { x: 32, y: 32, zoom: 8 };
  follow = true;
  onClick: (c: WorldClick) => void = () => {};
  onFollowChange: (follow: boolean) => void = () => {};

  async init(parent: HTMLElement): Promise<void> {
    await this.app.init({
      resizeTo: parent,
      background: 0x0d1117,
      antialias: true,
      autoDensity: true,
      resolution: window.devicePixelRatio || 1,
      preference: ['webgl', 'canvas'],
    });
    parent.appendChild(this.app.canvas);
    this.app.canvas.id = 'world-canvas';
    this.mapLayer.addChild(this.mapSprite, this.overlay);
    this.app.stage.addChild(this.mapLayer, this.markerLayer, this.highlight);
    this.bindInput(this.app.canvas);
    this.app.ticker.add((t) => this.frame(t.deltaMS));
  }

  destroy(): void {
    this.app.destroy(true, { children: true, texture: true });
    this.texture?.destroy(true);
  }

  setWorldSize(w: number, h: number): void {
    this.worldW = w;
    this.worldH = h;
  }

  /**
   * Show a tile grid: one texture pixel per tile. A grid of the same size
   * repaints the existing texture (the dynamic layers change every few
   * seconds); a new size makes a new one.
   */
  setTerrain(g: TerrainGrid): void {
    this.terrain = g;
    this.setWorldSize(g.width, g.height);
    const n = g.width * g.height * 4;
    const src = this.terrainSource;
    if (src && this.terrainBuf && src.width === g.width && src.height === g.height) {
      paintTerrain(g, this.terrainBuf);
      src.update();
      return;
    }
    const buf = new Uint8Array(n);
    paintTerrain(g, buf);
    const source = new BufferImageSource({ resource: buf, width: g.width, height: g.height, scaleMode: 'nearest' });
    const tex = new Texture({ source });
    this.mapSprite.texture = tex;
    this.mapSprite.scale.set(1);
    this.texture?.destroy(true);
    this.texture = tex;
    this.terrainBuf = buf;
    this.terrainSource = source;
  }

  setState(s: StateView): void {
    this.state = s;
    for (const m of this.markers.values()) m.seen = false;
    const me = s.player;
    for (const o of s.organisms) this.upsert(o.id, o.x, o.y, styleOwn(o));
    for (const o of s.nearby) this.upsert(o.id, o.x, o.y, styleNearby(o, me));
    for (const [id, m] of this.markers) {
      if (!m.seen) {
        m.g.destroy();
        this.markers.delete(id);
      }
    }
    const possessed = s.organisms.find((o) => o.possessed);
    if (!possessed) this.moveTarget = null;
    this.mapSprite.tint = s.clock.daytime ? 0xffffff : 0x8a96b8;
  }

  setMoveTarget(p: { x: number; y: number } | null): void {
    this.moveTarget = p;
  }

  setFollow(f: boolean): void {
    if (this.follow !== f) {
      this.follow = f;
      this.onFollowChange(f);
    }
  }

  centerOn(x: number, y: number): void {
    this.cam.x = x;
    this.cam.y = y;
  }

  zoomBy(factor: number, sx = this.app.screen.width / 2, sy = this.app.screen.height / 2): void {
    const before = this.screenToWorld(sx, sy);
    this.cam.zoom = Math.min(MAX_ZOOM, Math.max(MIN_ZOOM, this.cam.zoom * factor));
    const after = this.screenToWorld(sx, sy);
    this.cam.x += before.x - after.x;
    this.cam.y += before.y - after.y;
  }

  /** Screen position of an organism marker (for tests and tooltips). */
  markerScreenPos(id: OrgId): { x: number; y: number } | null {
    const m = this.markers.get(id);
    return m ? this.worldToScreen(m.x, m.y) : null;
  }

  worldToScreen(x: number, y: number): { x: number; y: number } {
    const { width, height } = this.app.screen;
    return { x: (x - this.cam.x) * this.cam.zoom + width / 2, y: (y - this.cam.y) * this.cam.zoom + height / 2 };
  }

  screenToWorld(sx: number, sy: number): { x: number; y: number } {
    const { width, height } = this.app.screen;
    return { x: (sx - width / 2) / this.cam.zoom + this.cam.x, y: (sy - height / 2) / this.cam.zoom + this.cam.y };
  }

  private upsert(id: OrgId, x: number, y: number, style: Style): void {
    let m = this.markers.get(id);
    if (!m) {
      const g = new Graphics();
      this.markerLayer.addChild(g);
      m = { g, x, y, tx: x, ty: y, style: '', seen: true };
      this.markers.set(id, m);
    }
    m.tx = x;
    m.ty = y;
    // A long jump (e.g. after a reconnect) snaps instead of sliding.
    if (Math.abs(m.x - x) + Math.abs(m.y - y) > 6) {
      m.x = x;
      m.y = y;
    }
    m.seen = true;
    const key = `${style.color}|${style.r}|${style.ring}|${style.own}`;
    if (key !== m.style) {
      m.style = key;
      drawMarker(m.g, style);
    }
    m.g.zIndex = style.own ? 2 : 1;
  }

  private frame(dtMs: number): void {
    const k = Math.min(1, dtMs / 120);
    if (this.follow && this.state) {
      const v = this.state.view;
      this.cam.x += (v.x - this.cam.x) * k * 0.5;
      this.cam.y += (v.y - this.cam.y) * k * 0.5;
    }
    this.cam.x = Math.min(this.worldW, Math.max(0, this.cam.x));
    this.cam.y = Math.min(this.worldH, Math.max(0, this.cam.y));
    const { width, height } = this.app.screen;
    this.mapLayer.scale.set(this.cam.zoom);
    this.mapLayer.position.set(width / 2 - this.cam.x * this.cam.zoom, height / 2 - this.cam.y * this.cam.zoom);
    this.drawOverlay();
    this.markerLayer.sortableChildren = true;
    for (const m of this.markers.values()) {
      m.x += (m.tx - m.x) * k;
      m.y += (m.ty - m.y) * k;
      const p = this.worldToScreen(m.x, m.y);
      m.g.position.set(p.x, p.y);
      m.g.visible = p.x > -20 && p.y > -20 && p.x < width + 20 && p.y < height + 20;
    }
    this.highlight.clear();
    const sel = this.selected !== null ? this.markers.get(this.selected) : undefined;
    if (sel) {
      const p = this.worldToScreen(sel.x, sel.y);
      this.highlight.circle(p.x, p.y, 11).stroke({ width: 2, color: 0xffffff });
    }
  }

  private drawOverlay(): void {
    const g = this.overlay;
    const px = 1 / this.cam.zoom; // one screen pixel in world units
    g.clear();
    if (this.cam.zoom >= GRID_ZOOM) {
      // Tile edges over the visible part of the map.
      const { width, height } = this.app.screen;
      const a = this.screenToWorld(0, 0);
      const b = this.screenToWorld(width, height);
      const x0 = Math.max(0, Math.floor(a.x)), x1 = Math.min(this.worldW, Math.ceil(b.x));
      const y0 = Math.max(0, Math.floor(a.y)), y1 = Math.min(this.worldH, Math.ceil(b.y));
      for (let x = x0; x <= x1; x++) g.moveTo(x, y0).lineTo(x, y1);
      for (let y = y0; y <= y1; y++) g.moveTo(x0, y).lineTo(x1, y);
      g.stroke({ width: px, color: 0x000000, alpha: 0.12 });
    }
    g.rect(0, 0, this.worldW, this.worldH).stroke({ width: 2 * px, color: 0x30363d });
    const s = this.state;
    if (!s) return;
    const v = s.view;
    g.rect(v.x - v.radius, v.y - v.radius, v.radius * 2, v.radius * 2).stroke({ width: 1.5 * px, color: 0x58a6ff, alpha: 0.6 });
    if (s.migration) {
      const [mx, my] = s.migration;
      g.circle(mx, my, 8 * px).stroke({ width: 2 * px, color: 0xf778ba });
      g.moveTo(mx - 5 * px, my).lineTo(mx + 5 * px, my).moveTo(mx, my - 5 * px).lineTo(mx, my + 5 * px).stroke({ width: 2 * px, color: 0xf778ba });
    }
    const t = this.moveTarget;
    const possessed = s.organisms.find((o) => o.possessed);
    if (t && possessed) {
      const m = this.markers.get(possessed.id);
      if (m) g.moveTo(m.x, m.y).lineTo(t.x, t.y).stroke({ width: 1.5 * px, color: 0xffd33d, alpha: 0.8 });
      g.circle(t.x, t.y, 4 * px).fill({ color: 0xffd33d, alpha: 0.9 });
    }
  }

  private hitTest(sx: number, sy: number): OrgId | null {
    let best: OrgId | null = null;
    let bestD = HIT_PX * HIT_PX;
    for (const [id, m] of this.markers) {
      const p = this.worldToScreen(m.x, m.y);
      const d = (p.x - sx) ** 2 + (p.y - sy) ** 2;
      // Prefer own organisms on ties (they're drawn on top).
      if (d < bestD || (d === bestD && m.g.zIndex > 1)) {
        bestD = d;
        best = id;
      }
    }
    return best;
  }

  private bindInput(canvas: HTMLCanvasElement): void {
    let down: { x: number; y: number; camX: number; camY: number; id: number } | null = null;
    let dragging = false;
    const local = (e: PointerEvent | WheelEvent) => {
      const r = canvas.getBoundingClientRect();
      return { x: e.clientX - r.left, y: e.clientY - r.top };
    };
    canvas.addEventListener('pointerdown', (e) => {
      const p = local(e);
      down = { x: p.x, y: p.y, camX: this.cam.x, camY: this.cam.y, id: e.pointerId };
      dragging = false;
      canvas.setPointerCapture(e.pointerId);
    });
    canvas.addEventListener('pointermove', (e) => {
      if (!down || e.pointerId !== down.id) return;
      const p = local(e);
      const dx = p.x - down.x;
      const dy = p.y - down.y;
      if (!dragging && dx * dx + dy * dy > 25) {
        dragging = true;
        this.setFollow(false);
      }
      if (dragging) {
        this.cam.x = down.camX - dx / this.cam.zoom;
        this.cam.y = down.camY - dy / this.cam.zoom;
      }
    });
    const up = (e: PointerEvent) => {
      if (!down || e.pointerId !== down.id) return;
      const p = local(e);
      if (!dragging) {
        const w = this.screenToWorld(p.x, p.y);
        this.onClick({ x: w.x, y: w.y, organism: this.hitTest(p.x, p.y) });
      }
      down = null;
      dragging = false;
    };
    canvas.addEventListener('pointerup', up);
    canvas.addEventListener('pointercancel', () => { down = null; dragging = false; });
    canvas.addEventListener('wheel', (e) => {
      e.preventDefault();
      const p = local(e);
      this.zoomBy(Math.exp(-e.deltaY * 0.0015), p.x, p.y);
    }, { passive: false });
    canvas.addEventListener('contextmenu', (e) => e.preventDefault());
  }
}

interface Style {
  color: number;
  r: number;
  ring: number | null;
  own: boolean;
}

function styleOwn(o: OwnOrganism): Style {
  return {
    color: OWN_COLOR,
    r: o.matured ? 5.5 : 3.5,
    ring: o.possessed ? 0xff7b72 : o.pregnant ? 0x7ee787 : null,
    own: true,
  };
}

function styleNearby(o: NearbyOrganism, _me: PlayerId): Style {
  return { color: ownerColor(o.owner), r: o.matured ? 4.5 : 3, ring: null, own: false };
}

function drawMarker(g: Graphics, s: Style): void {
  g.clear();
  if (s.ring !== null) g.circle(0, 0, s.r + 3.5).stroke({ width: 2.5, color: s.ring });
  g.circle(0, 0, s.r).fill(s.color).stroke({ width: s.own ? 1.5 : 1, color: s.own ? 0x1f2328 : 0x000000, alpha: 0.85 });
}
