// The map and organisms, drawn with PixiJS.
//
// Two layers: the map (the tile grid from /api/terrain painted one pixel per
// tile into a nearest-neighbor texture, one world unit per tile, plus
// overlays in world units) moves with the camera; the organism markers live
// in screen space so they stay readable at any zoom (growing a little as you
// zoom in). Each marker is a few sprites sharing textures cached per
// appearance class (`sprites.ts`): owner ring, body, status ring, action
// icon. Markers glide between server positions and the possessed organism
// is predicted toward its order (`motion.ts`), so movement is continuous
// rather than hopping at each `state`.
//
// Input: mouse drag or one-finger drag pans, wheel or two-finger pinch
// zooms, a tap/click selects, and a long press (touch or mouse held still)
// is reported separately (the game screen uses it to possess).

import { Application, BufferImageSource, Container, Graphics, Sprite, Texture } from 'pixi.js';
import { actionHint, bodyRadius, creatureKey, markerScaleFor, specOf, type CreatureSpec, type Hint } from './creature';
import { CreatureSprites, type MarkerParts } from './sprites';
import type { NearbyOrganism, OrgId, OwnOrganism, StateView } from '../protocol';
import { OWN_COLOR, ownerColor } from './colors';
import { groupColor, groupsOf, type TerritoryDraft } from '../groups';
import { paintTerrain, type TerrainGrid } from './terrain';
import { isOpenGround, newTrack, Predictor, RECONCILE_SNAP, RECONCILE_TAU_MS, retarget, smoothToward, TickClock, trackAt, type Point, type Track } from './motion';

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
  parts: MarkerParts;
  /** Displayed position (world). */
  x: number;
  y: number;
  /** The glide between server positions. */
  track: Track;
  style: string;
  seen: boolean;
  own: boolean;
  /** Body radius in screen pixels at marker scale 1. */
  r: number;
  /** 1 facing right, -1 facing left (the last horizontal heading). */
  facing: 1 | -1;
  hint: Hint | null;
}

/** What a marker shows, for tests and debugging. */
export interface MarkerInfo {
  key: string;
  hint: Hint | null;
  facing: 1 | -1;
  own: boolean;
  scale: number;
}

const MIN_ZOOM = 1;
const MAX_ZOOM = 48;
/** Minimum tap target radius in screen pixels (fingers need more than a mouse). */
const HIT_PX = 12;
const TOUCH_HIT_PX = 20;
/** A press held this long without moving is a long press. */
// Shorter than the browser's own long-press gesture (~500 ms in Chromium),
// which would otherwise cancel the pointer first; the `contextmenu` that
// gesture raises also counts as a long press.
const LONG_PRESS_MS = 450;
/** Movement (px) that turns a press into a drag. */
const DRAG_PX = 6;
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
  private sprites: CreatureSprites | null = null;
  /** Current marker scale (grows with zoom). */
  private markerScale = 1;
  private texture: Texture | null = null;
  private terrainBuf: Uint8Array | null = null;
  private terrainSource: BufferImageSource | null = null;
  /** The tile grid on screen (null until the first terrain arrives). */
  terrain: TerrainGrid | null = null;
  private worldW = 64;
  private worldH = 64;
  private state: StateView | null = null;
  private moveTarget: { x: number; y: number } | null = null;
  /** Prediction for the possessed organism, and the measured tick length. */
  readonly predictor = new Predictor();
  readonly tickClock = new TickClock();
  private readonly passable = (x: number, y: number) => isOpenGround(this.terrain, x, y);
  selected: OrgId | null = null;
  /** The group picked in the group menu (its territory is drawn bolder), or null for all. */
  selectedGroup: number | null = null;
  /** A territory being drawn (center tapped, edge not yet). */
  territoryDraft: TerritoryDraft | null = null;
  cam: Camera = { x: 32, y: 32, zoom: 8 };
  follow = true;
  onClick: (c: WorldClick) => void = () => {};
  /** A press held still (touch or mouse); defaults to nothing. */
  onLongPress: (c: WorldClick) => void = () => {};
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
    this.sprites = new CreatureSprites(this.app.renderer);
    this.app.canvas.id = 'world-canvas';
    this.mapLayer.addChild(this.mapSprite, this.overlay);
    this.app.stage.addChild(this.mapLayer, this.markerLayer, this.highlight);
    this.bindInput(this.app.canvas);
    this.app.ticker.add((t) => this.frame(t.deltaMS));
  }

  destroy(): void {
    this.sprites?.destroy();
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
    const now = performance.now();
    this.state = s;
    this.tickClock.observe(s.clock.tick, now);
    for (const m of this.markers.values()) m.seen = false;
    for (const o of s.organisms) this.upsert(o, true, now);
    for (const o of s.nearby) this.upsert(o, false, now);
    for (const [id, m] of this.markers) {
      if (!m.seen) {
        m.parts.root.destroy({ children: true });
        this.markers.delete(id);
      }
    }
    const possessed = s.organisms.find((o) => o.possessed);
    this.predictor.onState(possessed, now);
    if (!possessed) this.moveTarget = null;
    this.mapSprite.tint = s.clock.daytime ? 0xffffff : 0x8a96b8;
  }

  /** Show a move order for the possessed organism (and predict the walk toward it), or clear it. */
  setMoveTarget(p: { x: number; y: number } | null): void {
    this.moveTarget = p;
    if (p) this.predictor.order(p.x, p.y, performance.now());
    else this.predictor.reject();
  }

  /** The server refused the last move order: stop predicting toward it. */
  rejectMove(): void {
    this.predictor.reject();
    this.moveTarget = null;
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

  /** What an organism's marker shows (for tests and debugging). */
  markerInfo(id: OrgId): MarkerInfo | null {
    const m = this.markers.get(id);
    return m ? { key: m.style, hint: m.hint, facing: m.facing, own: m.own, scale: this.markerScale } : null;
  }

  /** Markers on screen and distinct textures made for them (the cache size). */
  spriteStats(): { markers: number; textures: number; made: number } {
    return { markers: this.markers.size, textures: this.sprites?.cache.size ?? 0, made: this.sprites?.cache.made ?? 0 };
  }

  /** Drawn world position of an organism marker (glided or predicted; for tests). */
  markerWorldPos(id: OrgId): { x: number; y: number } | null {
    const m = this.markers.get(id);
    return m ? { x: m.x, y: m.y } : null;
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

  /** What the overlay shows of groups, for tests and debugging. */
  overlayInfo(): {
    territories: { group: number; x: number; y: number; radius: number }[];
    rallies: { group: number; x: number; y: number; radius: number }[];
    draft: TerritoryDraft | null;
  } {
    const groups = groupsOf(this.state);
    const territories = groups.flatMap((g) => (g.territory ? [{ group: g.id, x: g.territory.x, y: g.territory.y, radius: g.territory.radius }] : []));
    const rallies = groups.flatMap((g) => (g.rally ? [{ group: g.id, x: g.rally.x, y: g.rally.y, radius: g.rally.radius }] : []));
    return { territories, rallies, draft: this.territoryDraft };
  }

  private upsert(o: OwnOrganism | NearbyOrganism, own: boolean, now: number): void {
    const sprites = this.sprites;
    if (!sprites) return;
    let m = this.markers.get(o.id);
    if (!m) {
      const parts = sprites.marker();
      this.markerLayer.addChild(parts.root);
      m = { parts, x: o.x, y: o.y, track: newTrack(o.x, o.y, now), style: '', seen: true, own, r: 0, facing: 1, hint: null };
      this.markers.set(o.id, m);
    }
    const dx = o.x - m.track.tx;
    if (Math.abs(dx) > 0.02) m.facing = dx > 0 ? 1 : -1;
    // Glide from where it is drawn; a long jump (e.g. a reconnect) snaps.
    retarget(m.track, m, o.x, o.y, now);
    m.seen = true;
    m.own = own;
    const spec = specOf(o);
    const status = own ? statusColor(o as OwnOrganism) : null;
    const hint = actionHint(o.action);
    const tint = own ? OWN_COLOR : ownerColor((o as NearbyOrganism).owner);
    const key = `${creatureKey(spec)}|${tint}|${status ?? ''}|${hint ?? ''}|${own}`;
    if (key !== m.style) {
      m.style = key;
      m.hint = hint;
      m.r = bodyRadius(spec);
      applyMarker(sprites, m.parts, spec, own, tint, status, hint);
    }
    m.parts.body.scale.x = m.facing;
    m.parts.root.zIndex = own ? 2 : 1;
  }

  private frame(dtMs: number): void {
    const now = performance.now();
    const k = Math.min(1, dtMs / 120);
    this.moveMarkers(now, dtMs);
    if (this.follow && this.state) {
      // The view window follows the possessed organism: follow its drawn
      // (predicted) position so the camera moves as smoothly as it does.
      // (Not when the view is pinned elsewhere.)
      const sv = this.state.view;
      const pm = this.predictor.id !== null ? this.markers.get(this.predictor.id) : undefined;
      const v: Point = pm && Math.abs(pm.x - sv.x) + Math.abs(pm.y - sv.y) < 3 ? pm : sv;
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
    const sc = markerScaleFor(this.cam.zoom);
    this.markerScale = sc;
    for (const m of this.markers.values()) {
      const p = this.worldToScreen(m.x, m.y);
      const root = m.parts.root;
      root.position.set(p.x, p.y);
      root.scale.set(sc);
      const pad = 40 * sc;
      root.visible = p.x > -pad && p.y > -pad && p.x < width + pad && p.y < height + pad;
    }
    this.highlight.clear();
    const sel = this.selected !== null ? this.markers.get(this.selected) : undefined;
    if (sel) {
      const p = this.worldToScreen(sel.x, sel.y);
      const r = Math.max(9, sel.r * 2.3 * sc);
      this.highlight.circle(p.x, p.y, r).stroke({ width: 3.5, color: 0x0d1117, alpha: 0.6 });
      this.highlight.circle(p.x, p.y, r).stroke({ width: 2, color: 0xffffff });
    }
  }

  /**
   * Drawn positions for this frame: every marker on its glide between server
   * positions; the possessed one eased toward its predicted position when
   * there is one (else toward its glide), so corrections slide.
   */
  private moveMarkers(now: number, dtMs: number): void {
    const pid = this.predictor.id;
    for (const [id, m] of this.markers) {
      const g = trackAt(m.track, now);
      if (id !== pid) {
        m.x = g.x;
        m.y = g.y;
        continue;
      }
      const goal = this.predictor.goal(now, this.tickClock.msPerTick, this.passable) ?? g;
      const p = smoothToward(m, goal, dtMs, RECONCILE_TAU_MS, RECONCILE_SNAP);
      m.x = p.x;
      m.y = p.y;
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
    // Each group's territory (a tinted disc, the picked group's outline
    // bolder) and migration target (a cross in a ring; pink for a lone group).
    const groups = groupsOf(s);
    for (const grp of groups) {
      const color = groups.length > 1 ? groupColor(grp.id) : 0xf778ba;
      const t = grp.territory;
      if (t) {
        const picked = this.selectedGroup === grp.id;
        const tint = groupColor(grp.id);
        g.circle(t.x, t.y, t.radius).fill({ color: tint, alpha: picked ? 0.2 : 0.12 });
        g.circle(t.x, t.y, t.radius).stroke({ width: (picked ? 3 : 1.5) * px, color: tint, alpha: 0.9 });
      }
      const r = grp.rally;
      if (r) {
        // A rally: the circle the group gathers in (outline only) and a
        // filled flag point at its center.
        const picked = this.selectedGroup === grp.id;
        const tint = groupColor(grp.id);
        g.circle(r.x, r.y, r.radius).stroke({ width: (picked ? 2.5 : 1.5) * px, color: tint, alpha: 0.7 });
        g.circle(r.x, r.y, 5 * px).fill({ color: tint, alpha: 0.95 });
        g.circle(r.x, r.y, 5 * px).stroke({ width: 1.5 * px, color: 0xffffff, alpha: 0.9 });
      }
      if (grp.migration) {
        const [mx, my] = grp.migration;
        g.circle(mx, my, 8 * px).stroke({ width: 2 * px, color });
        g.moveTo(mx - 5 * px, my).lineTo(mx + 5 * px, my).moveTo(mx, my - 5 * px).lineTo(mx, my + 5 * px).stroke({ width: 2 * px, color });
      }
    }
    const d = this.territoryDraft;
    if (d) {
      // The largest circle the group may claim, and its center.
      const tint = groupColor(d.group);
      g.circle(d.x, d.y, d.radius).fill({ color: tint, alpha: 0.08 });
      g.circle(d.x, d.y, d.radius).stroke({ width: 1.5 * px, color: tint, alpha: 0.6 });
      g.circle(d.x, d.y, 3 * px).fill({ color: tint });
    }
    const possessed = s.organisms.find((o) => o.possessed);
    // The order the player gave, else the server's (e.g. after a reload).
    const t = this.moveTarget ?? (possessed?.order ? { x: possessed.order[0], y: possessed.order[1] } : null);
    if (t && possessed) {
      const m = this.markers.get(possessed.id);
      if (m) g.moveTo(m.x, m.y).lineTo(t.x, t.y).stroke({ width: 1.5 * px, color: 0xffd33d, alpha: 0.8 });
      g.circle(t.x, t.y, 4 * px).fill({ color: 0xffd33d, alpha: 0.9 });
    }
  }

  /** The organism under a screen point: the nearest marker within its tap radius. */
  hitTest(sx: number, sy: number, touch = false): OrgId | null {
    let best: OrgId | null = null;
    let bestD = Infinity;
    const min = touch ? TOUCH_HIT_PX : HIT_PX;
    for (const [id, m] of this.markers) {
      const p = this.worldToScreen(m.x, m.y);
      const d = (p.x - sx) ** 2 + (p.y - sy) ** 2;
      const reach = Math.max(min, m.r * 1.6 * this.markerScale);
      if (d > reach * reach) continue;
      // Prefer own organisms on ties (they're drawn on top).
      if (d < bestD || (d === bestD && m.own)) {
        bestD = d;
        best = id;
      }
    }
    return best;
  }

  private bindInput(canvas: HTMLCanvasElement): void {
    // Active pointers (mouse, pen or fingers) by id, at their latest position.
    const pointers = new Map<number, { x: number; y: number }>();
    // The single-pointer gesture in progress: a tap, a long press or a drag.
    let press: { id: number; x: number; y: number; camX: number; camY: number; touch: boolean; dragging: boolean; timer: number | null; done: boolean; hit: OrgId | null } | null = null;
    // The two-finger gesture in progress: distance and midpoint at the last move.
    let pinch: { d: number; mx: number; my: number } | null = null;
    const local = (e: PointerEvent | WheelEvent) => {
      const r = canvas.getBoundingClientRect();
      return { x: e.clientX - r.left, y: e.clientY - r.top };
    };
    const cancelPress = () => {
      if (press?.timer != null) clearTimeout(press.timer);
      press = null;
    };
    const pinchOf = () => {
      const [a, b] = [...pointers.values()];
      return { d: Math.hypot(a.x - b.x, a.y - b.y), mx: (a.x + b.x) / 2, my: (a.y + b.y) / 2 };
    };
    const click = (x: number, y: number, touch: boolean): WorldClick => {
      const w = this.screenToWorld(x, y);
      return { x: w.x, y: w.y, organism: this.hitTest(x, y, touch) };
    };

    const longPress = (pr: NonNullable<typeof press>) => {
      if (press !== pr || pr.dragging || pr.done) return;
      pr.done = true;
      if (pr.timer != null) clearTimeout(pr.timer);
      pr.timer = null;
      const c = click(pr.x, pr.y, pr.touch);
      this.onLongPress(pr.hit !== null && this.markers.has(pr.hit) ? { ...c, organism: pr.hit } : c);
    };

    canvas.addEventListener('pointerdown', (e) => {
      const p = local(e);
      pointers.set(e.pointerId, p);
      try {
        canvas.setPointerCapture(e.pointerId);
      } catch {
        /* synthetic events may not be capturable */
      }
      if (pointers.size === 2) {
        // A second finger turns the gesture into a pinch.
        cancelPress();
        pinch = pinchOf();
        this.setFollow(false);
        return;
      }
      if (pointers.size > 2) return;
      cancelPress();
      const touch = e.pointerType === 'touch' || e.pointerType === 'pen';
      // The organism under the finger when it went down: a long press acts on
      // it even if its marker glides away during the hold.
      const pr = { id: e.pointerId, x: p.x, y: p.y, camX: this.cam.x, camY: this.cam.y, touch, dragging: false, timer: null as number | null, done: false,
        hit: this.hitTest(p.x, p.y, touch) };
      if (e.button === 0 || touch) pr.timer = window.setTimeout(() => longPress(pr), LONG_PRESS_MS);
      press = pr;
    });
    canvas.addEventListener('pointermove', (e) => {
      if (!pointers.has(e.pointerId)) return;
      const p = local(e);
      pointers.set(e.pointerId, p);
      if (pinch && pointers.size >= 2) {
        const now = pinchOf();
        if (pinch.d > 0 && now.d > 0) this.zoomBy(now.d / pinch.d, now.mx, now.my);
        this.cam.x -= (now.mx - pinch.mx) / this.cam.zoom;
        this.cam.y -= (now.my - pinch.my) / this.cam.zoom;
        pinch = now;
        return;
      }
      if (!press || e.pointerId !== press.id || press.done) return;
      const dx = p.x - press.x;
      const dy = p.y - press.y;
      if (!press.dragging && dx * dx + dy * dy > DRAG_PX * DRAG_PX) {
        press.dragging = true;
        if (press.timer != null) clearTimeout(press.timer);
        press.timer = null;
        this.setFollow(false);
      }
      if (press.dragging) {
        this.cam.x = press.camX - dx / this.cam.zoom;
        this.cam.y = press.camY - dy / this.cam.zoom;
      }
    });
    const up = (e: PointerEvent, cancelled: boolean) => {
      if (!pointers.delete(e.pointerId)) return;
      if (pinch) {
        // Lifting one finger of a pinch ends it; the other doesn't start a drag or tap.
        if (pointers.size < 2) pinch = null;
        return;
      }
      if (!press || e.pointerId !== press.id) return;
      const pr = press;
      cancelPress();
      if (!cancelled && !pr.dragging && !pr.done) this.onClick(click(pr.x, pr.y, pr.touch));
    };
    canvas.addEventListener('pointerup', (e) => up(e, false));
    canvas.addEventListener('pointercancel', (e) => {
      // A touch held still that the browser cancels is its long-press gesture.
      if (press && e.pointerId === press.id && press.touch && !press.dragging && !press.done && !pinch) longPress(press);
      up(e, true);
    });
    canvas.addEventListener('wheel', (e) => {
      e.preventDefault();
      const p = local(e);
      this.zoomBy(Math.exp(-e.deltaY * 0.0015), p.x, p.y);
    }, { passive: false });
    canvas.addEventListener('contextmenu', (e) => {
      e.preventDefault();
      if (press && press.touch && !press.dragging) longPress(press);
    });
  }
}

function statusColor(o: OwnOrganism): number | null {
  return o.possessed ? 0xff7b72 : o.pregnant ? 0x7ee787 : null;
}

function applyMarker(sprites: CreatureSprites, parts: MarkerParts, spec: CreatureSpec, own: boolean, tint: number, status: number | null, hint: Hint | null): void {
  parts.ring.texture = sprites.ownerRing(spec, own);
  parts.ring.tint = tint;
  parts.body.texture = sprites.body(spec);
  parts.status.visible = status !== null;
  if (status !== null) {
    parts.status.texture = sprites.statusRing(spec);
    parts.status.tint = status;
  }
  parts.icon.visible = hint !== null;
  if (hint !== null) {
    parts.icon.texture = sprites.hint(hint);
    const r = bodyRadius(spec);
    parts.icon.position.set(r * 1.2, -r * 1.9 - 4);
  }
}
