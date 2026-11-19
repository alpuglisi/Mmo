// The map and organisms, drawn with PixiJS.
//
// Two layers: the map (the tile grid from /api/terrain painted into soft,
// textured ground by `terrainLayer.ts`, one world unit per tile, plus
// overlays in world units) moves with the camera; the organism markers live
// in screen space so they stay readable at any zoom (growing a little as you
// zoom in). Each marker is a few sprites sharing textures cached per
// appearance class (`sprites.ts`): contact shadow and owner ring at the
// feet, body, status mark, action icon (STYLE.md). Markers glide between
// server positions and the possessed organism is predicted toward its order
// (`motion.ts`), so movement is continuous rather than hopping at each
// `state`.
//
// Vision cones (`visionCones.ts`): around the player's own selected
// organism (else the possessed one), when the server sends its eyes and
// heading (own organisms only), drawn in screen space under the markers and
// turned smoothly toward each new heading.
//
// Input: mouse drag or one-finger drag pans, wheel or two-finger pinch
// zooms, a tap/click selects, and a long press (touch or mouse held still)
// is reported separately (the game screen uses it to possess).
//
// GPU trouble is contained (`renderGuard.ts`): a drawing error is logged and
// never escapes into the state handler or stops the ticker; a lost WebGL
// context pauses drawing and drops the generated marker textures, which are
// rebuilt from the current state on restore, or on a new renderer if the
// context does not come back within a few seconds.

import { clampCam, followAxis } from './camera';
import { Application, BufferImageSource, Container, Graphics, Sprite, Texture, UPDATE_PRIORITY, type Renderer } from 'pixi.js';
import { actionHint, bodyRadius, creatureKey, markerScaleFor, NEAR_SCALE, specOf, type CreatureSpec, type Hint } from './creature';
import { CreatureSprites, type MarkerParts, type StatusKind } from './sprites';
import { ContextGuard, ErrorLog, type GuardState } from './renderGuard';
import type { NearbyOrganism, OrgId, OwnOrganism, StateView } from '../protocol';
import { OWN_COLOR, ownerColor } from './colors';
import { groupColor, groupsOf, homeRangeGroup, type TerritoryDraft } from '../groups';
import { foodPixels, type TerrainGrid } from './terrain';
import { TerrainLayer, type TerrainStats } from './terrainLayer';
import { MARK } from './palette';
import { isOpenGround, newTrack, Predictor, RECONCILE_SNAP, RECONCILE_TAU_MS, retarget, smoothToward, TickClock, trackAt, type Point, type Track } from './motion';
import { arcPoints, coneLayout, dotGrid, easeAngle, facingOf, hatchSegments, headingDeg, VISION_COLORS, wedgePolygon, type ConeLayout } from './visionCones';

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
  /** Every organism within reach of the point, the player's own first, then nearest (a tap in a herd). */
  candidates?: OrgId[];
  /** Shift, Ctrl, Alt or ⌘ held (select instead of walking while possessing). */
  modifier?: boolean;
  /** A finger or pen, not a mouse. */
  touch?: boolean;
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
  /** 1 facing right, -1 facing left (the server heading's side for own organisms, else the last horizontal move). */
  facing: 1 | -1;
  hint: Hint | null;
  /** What the marker should show (its textures are made from this). */
  look: MarkerLook | null;
  /** The textures do not match `look` yet (not made, dropped, or making them failed). */
  dirty: boolean;
}

interface MarkerLook {
  spec: CreatureSpec;
  own: boolean;
  tint: number;
  status: StatusKind | null;
  hint: Hint | null;
}

/** The vision cones on screen, for tests and debugging. */
export interface VisionInfo {
  organism: OrgId;
  /** The server heading (screen degrees) and the drawn one (eased toward it). */
  heading: number;
  drawn: number;
  /** Cone radius, screen pixels. */
  radius: number;
  covered: number;
  blind: number;
  overlap: number;
  eyes: number;
  /** Hatch strokes in the blind spots and dots in the depth zone. */
  hatch: number;
  dots: number;
  /** Screen position of the cones' center. */
  x: number;
  y: number;
}

/** Cone colours: the builder's eye view and the legend use the same (`VISION_COLORS`). */
const CONE_SEEN = VISION_COLORS.seen;
const CONE_DEPTH = VISION_COLORS.depth;
const CONE_BLIND = VISION_COLORS.blind;
/** Easing time constant of the drawn heading toward the server's (ms). */
const TURN_TAU_MS = 140;

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
/** Follow's dead zone, as a share of half the screen's shorter side: the herd, and a possessed animal (`followAxis`). */
const FOLLOW_DEAD = 0.25;
const FOLLOW_DEAD_POSSESSED = 0.12;
/**
 * Time a frame may spend making marker textures (ms). Markers whose textures
 * are all cached are always shown; the rest wait for a later frame once this
 * is spent (at least one is made per frame), so a frame that brings many new
 * looks (a zoom in, a crowd arriving) spreads the drawing instead of hitching.
 */
export const TEXTURE_BUDGET_MS = 4;

export class WorldView {
  /** The PixiJS app; replaced by a new one if a lost WebGL context never comes back. */
  app = new Application();
  private parent: HTMLElement | null = null;
  /** Drawing errors, logged and throttled; never thrown at callers. */
  readonly errors = new ErrorLog();
  readonly guard = new ContextGuard({
    onLost: () => this.dropTextures(),
    onRestored: () => {
      this.dropTextures();
      this.flushMarkers();
    },
    recreate: () => this.recreate(),
  }, { log: this.errors });
  private mapLayer = new Container();
  /** The painted terrain (world units, under the overlay). */
  readonly terrainLayer = new TerrainLayer();
  /** The Food layer (`setFoodOverlay`): one texel per tile, over the painted map. */
  private foodSprite = new Sprite();
  private foodSource: BufferImageSource | null = null;
  private overlay = new Graphics();
  private markerLayer = new Container();
  private markers = new Map<OrgId, Marker>();
  private highlight = new Graphics();
  /** The vision cones (screen space, under the markers). */
  private visionLayer = new Graphics();
  /** Draw vision cones for the selected (or possessed) own organism. */
  showVision = true;
  /** Last drawn cones (null: none), and the heading as drawn. */
  private vision: VisionInfo | null = null;
  private drawnHeading: { id: OrgId; deg: number } | null = null;
  private sprites: CreatureSprites | null = null;
  /** Current marker scale (grows with zoom). */
  private markerScale = 1;
  /** Markers use the far (class) sprites: drawn smaller than `NEAR_SCALE`. */
  private far = false;
  /** Markers left for a later frame by the texture budget, in all. */
  private deferred = 0;
  /** Hour of the day (the map's light), from the state. */
  private hour: number | null = null;
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
  /** A pointer is down on the map (Follow holds the camera still meanwhile). */
  private pressing = false;
  /** Screen pixels the camera may show past each map edge: what floats over the map there (the toolbar). */
  edgePad = { top: 0, right: 0, bottom: 0, left: 0 };
  /** The first frame with a size picks the starting zoom (about 40 tiles across the shorter side). */
  private zoomFitted = false;
  follow = true;
  onClick: (c: WorldClick) => void = () => {};
  /** A press held still (touch or mouse); defaults to nothing. */
  onLongPress: (c: WorldClick) => void = () => {};
  onFollowChange: (follow: boolean) => void = () => {};

  async init(parent: HTMLElement): Promise<void> {
    this.parent = parent;
    this.mapLayer.addChild(this.terrainLayer.root, this.foodSprite, this.overlay);
    this.foodSprite.visible = false;
    await this.startApp(this.app);
    this.adopt(this.app);
  }

  private async startApp(app: Application): Promise<void> {
    await app.init({
      resizeTo: this.parent!,
      background: 0x0d1117,
      antialias: true,
      autoDensity: true,
      resolution: window.devicePixelRatio || 1,
      preference: ['webgl', 'canvas'],
    });
  }

  /** Make `app` the one drawing: its canvas on the page, our layers on its stage, input and the frame loop. */
  private adopt(app: Application): void {
    this.app = app;
    this.parent!.appendChild(app.canvas);
    app.canvas.id = 'world-canvas';
    this.sprites = new CreatureSprites(app.renderer, () => this.texturesInUse());
    app.stage.addChild(this.mapLayer, this.visionLayer, this.markerLayer, this.highlight);
    this.bindInput(app.canvas);
    // PixiJS draws from a ticker callback, and a throw there stops the ticker
    // for good: draw through a guard instead (paused while the context is lost).
    app.ticker.remove(app.render, app);
    app.ticker.add((t) => {
      if (this.app !== app || this.guard.paused) return;
      this.errors.run('frame', () => this.frame(t.deltaMS));
    });
    app.ticker.add(() => {
      if (this.app !== app || this.guard.paused) return;
      if (!this.errors.run('render', () => app.render())) this.checkContext();
    }, undefined, UPDATE_PRIORITY.LOW);
    // After PixiJS's own listeners, so its systems are re-initialized before we rebuild.
    this.guard.attach(app.canvas);
  }

  /** After a drawing error: if the context is lost (the event may not have come, or came first), say so. */
  private checkContext(): void {
    const r = this.app.renderer as Renderer & { gl?: WebGLRenderingContext };
    if (r?.gl?.isContextLost?.()) this.guard.lost('drawing failed on a lost context');
  }

  /**
   * Drop the generated marker textures (their pixels live only on the GPU):
   * markers show nothing until rebuilt by `flushMarkers`.
   */
  private dropTextures(): void {
    for (const m of this.markers.values()) {
      for (const s of [m.parts.ring, m.parts.body, m.parts.status, m.parts.icon]) s.texture = Texture.EMPTY;
      m.dirty = true;
    }
    this.errors.run('drop textures', () => this.sprites?.cache.clear());
  }

  /** The textures markers show now (the cache must not drop them). */
  private texturesInUse(): Set<Texture> {
    const out = new Set<Texture>();
    for (const m of this.markers.values()) for (const s of [m.parts.ring, m.parts.body, m.parts.status, m.parts.icon]) out.add(s.texture);
    return out;
  }

  /**
   * Make the textures of markers whose look changed (or were dropped), yours
   * first, within the frame's texture budget. Stops at the first failure.
   */
  private flushMarkers(): void {
    const sprites = this.sprites;
    if (!sprites || this.guard.paused) return;
    const start = performance.now();
    let made = false;
    for (const own of [true, false]) {
      for (const m of this.markers.values()) {
        if (!m.dirty || !m.look || m.own !== own) continue;
        const l = m.look;
        if (made && performance.now() - start > TEXTURE_BUDGET_MS && !sprites.ready(l.spec, l.own, l.status, l.hint)) {
          this.deferred++;
          continue; // next frame
        }
        const before = sprites.cache.made;
        if (!this.errors.run('build marker textures', () => applyMarker(sprites, m.parts, l.spec, l.own, l.tint, l.status, l.hint))) {
          this.checkContext();
          return; // try again next frame
        }
        if (sprites.cache.made !== before) made = true;
        m.dirty = false;
      }
    }
  }

  /** The context did not come back: draw on a new app (and canvas), from the current state. */
  private async recreate(): Promise<void> {
    const old = this.app;
    const app = new Application();
    await this.startApp(app);
    old.stage?.removeChildren();
    this.errors.run('destroy old sprites', () => this.sprites?.destroy());
    for (const m of this.markers.values()) m.dirty = true;
    this.adopt(app);
    this.errors.run('destroy old renderer', () => old.destroy({ removeView: true }, { children: false }));
    // The guard turns live when this resolves; the next frame rebuilds the markers.
  }

  /** WebGL context handling, for tests and debugging. */
  contextStats(): { state: GuardState; lost: number; restored: number; recreated: number; dirty: number; errors: Record<string, number> } {
    let dirty = 0;
    for (const m of this.markers.values()) if (m.dirty) dirty++;
    return { state: this.guard.state, ...this.guard.stats, dirty, errors: Object.fromEntries(this.errors.counts) };
  }

  destroy(): void {
    this.guard.dispose();
    this.sprites?.destroy();
    this.terrainLayer.destroy();
    this.app.destroy(true, { children: true, texture: true });
  }

  setWorldSize(w: number, h: number): void {
    this.worldW = w;
    this.worldH = h;
  }

  /**
   * Show a tile grid. A grid of the same size (the dynamic layers change
   * every few seconds) repaints what changed, progressively.
   */
  setTerrain(g: TerrainGrid): void {
    this.terrain = g;
    this.setWorldSize(g.width, g.height);
    this.terrainLayer.setTerrain(g);
    if (this.foodSprite.visible) this.paintFood();
  }

  /** The Food layer: where plants (and so grazers) are, bare to rich, over the map. */
  setFoodOverlay(on: boolean): void {
    this.foodSprite.visible = on;
    if (on) this.paintFood();
  }

  get foodOverlay(): boolean {
    return this.foodSprite.visible;
  }

  private paintFood(): void {
    const g = this.terrain;
    if (!g) return;
    const px = foodPixels(g);
    const same = this.foodSource && this.foodSource.width === g.width && this.foodSource.height === g.height;
    if (same) {
      (this.foodSource!.resource as Uint8Array).set(px);
      this.foodSource!.update();
      return;
    }
    this.foodSprite.texture?.destroy(true);
    this.foodSource = new BufferImageSource({ resource: px, width: g.width, height: g.height, scaleMode: 'nearest' });
    this.foodSprite.texture = new Texture({ source: this.foodSource });
    this.foodSprite.position.set(0, 0);
    this.foodSprite.scale.set(1);
  }

  /** The painted map: sizes, detail level, chunks and paint timings (tests and debugging). */
  terrainStats(): TerrainStats {
    return this.terrainLayer.terrainStats();
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
        this.markers.delete(id);
        this.errors.run('remove marker', () => m.parts.root.destroy({ children: true }));
      }
    }
    // Textures last, and contained: the state above is in place even if the GPU is not.
    this.flushMarkers();
    const possessed = s.organisms.find((o) => o.possessed);
    this.predictor.onState(possessed, now);
    if (!possessed) this.moveTarget = null;
    this.hour = s.clock.hour;
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

  /**
   * Markers on screen and the texture cache: distinct textures held, made
   * (misses), evicted, their estimated bytes, time making them (total and
   * slowest, ms), markers deferred by the frame budget, and whether markers
   * use the far (class) sprites.
   */
  spriteStats(): { markers: number; textures: number; made: number; evicted: number; bytes: number; makeMs: number; maxMakeMs: number; deferred: number; far: boolean } {
    const c = this.sprites?.cache;
    return {
      markers: this.markers.size, textures: c?.size ?? 0, made: c?.made ?? 0, evicted: c?.evicted ?? 0, bytes: c?.cost ?? 0,
      makeMs: c?.makeMs ?? 0, maxMakeMs: c?.maxMakeMs ?? 0, deferred: this.deferred, far: this.far,
    };
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
    /** The group whose territory is drawn as the home range, or null. */
    home: number | null;
  } {
    const groups = drawnGroups(this.state);
    // A territory kept while its group migrates is still drawn (playtest 8: hidden from its owner).
    const territories = groups.flatMap((g) => {
      const t = g.territory ?? g.territory_kept;
      return t ? [{ group: g.id, x: t.x, y: t.y, radius: t.radius }] : [];
    });
    const rallies = groups.flatMap((g) => (g.rally ? [{ group: g.id, x: g.rally.x, y: g.rally.y, radius: g.rally.radius }] : []));
    return { territories, rallies, draft: this.territoryDraft, home: homeRangeGroup(groups)?.id ?? null };
  }

  private upsert(o: OwnOrganism | NearbyOrganism, own: boolean, now: number): void {
    const sprites = this.sprites;
    if (!sprites) return;
    let m = this.markers.get(o.id);
    if (!m) {
      const parts = sprites.marker();
      this.markerLayer.addChild(parts.root);
      m = { parts, x: o.x, y: o.y, track: newTrack(o.x, o.y, now), style: '', seen: true, own, r: 0, facing: 1, hint: null, look: null, dirty: false };
      this.markers.set(o.id, m);
    }
    const heading = own ? (o as OwnOrganism).heading : undefined;
    if (heading !== undefined) m.facing = facingOf(headingDeg(heading), m.facing);
    else {
      const dx = o.x - m.track.tx;
      if (Math.abs(dx) > 0.02) m.facing = dx > 0 ? 1 : -1;
    }
    // Glide from where it is drawn; a long jump (e.g. a reconnect) snaps.
    retarget(m.track, m, o.x, o.y, now);
    m.seen = true;
    m.own = own;
    const spec = specOf(o, this.far);
    const status = own ? statusOf(o as OwnOrganism) : null;
    // Action icons only where they tell something: danger (fleeing, hunting)
    // on anyone, the quieter ones (resting, courting) on your own.
    const action = actionHint(o.action);
    const hint = action === 'flee' || action === 'hunt' || own ? action : null;
    const tint = own ? OWN_COLOR : ownerColor((o as NearbyOrganism).owner);
    const key = `${creatureKey(spec)}|${tint}|${status ?? ''}|${hint ?? ''}|${own}`;
    if (key !== m.style) {
      m.style = key;
      m.hint = hint;
      m.r = bodyRadius(spec);
      m.look = { spec, own, tint, status, hint };
      m.dirty = true;
    }
    m.parts.body.scale.x = m.facing;
    m.parts.root.zIndex = own ? 2 : 1;
  }

  private frame(dtMs: number): void {
    const now = performance.now();
    const k = Math.min(1, dtMs / 120);
    this.flushMarkers();
    this.moveMarkers(now, dtMs);
    if (this.follow && this.state) {
      // The view window follows the possessed organism: follow its drawn
      // (predicted) position so the camera moves as smoothly as it does.
      // (Not when the view is pinned elsewhere.)
      const sv = this.state.view;
      const pm = this.predictor.id !== null ? this.markers.get(this.predictor.id) : undefined;
      const v: Point = pm && Math.abs(pm.x - sv.x) + Math.abs(pm.y - sv.y) < 3 ? pm : sv;
      // A dead zone round the centre (a smaller one for a possessed animal,
      // which the player steers), and still while a finger is down: taps
      // land where they were aimed (`followAxis`).
      if (!this.pressing) {
        const { width: sw, height: sh } = this.app.screen;
        const dead = (pm ? FOLLOW_DEAD_POSSESSED : FOLLOW_DEAD) * Math.min(sw, sh) / 2 / this.cam.zoom;
        this.cam.x = followAxis(this.cam.x, v.x, dead, k * 0.5);
        this.cam.y = followAxis(this.cam.y, v.y, dead, k * 0.5);
      }
    }
    const { width, height } = this.app.screen;
    if (!this.zoomFitted && width > 0 && height > 0) {
      // A big window starts closer in, so the creatures read and the map fills
      // it; a phone keeps the old default (8 px a tile).
      this.zoomFitted = true;
      this.cam.zoom = Math.min(18, Math.max(8, Math.min(width, height) / 40));
    }
    const pad = this.edgePad, z = this.cam.zoom;
    this.cam.x = clampCam(this.cam.x, width / 2 / z, this.worldW, pad.left / z, pad.right / z);
    this.cam.y = clampCam(this.cam.y, height / 2 / z, this.worldH, pad.top / z, pad.bottom / z);
    this.mapLayer.scale.set(this.cam.zoom);
    this.mapLayer.position.set(width / 2 - this.cam.x * this.cam.zoom, height / 2 - this.cam.y * this.cam.zoom);
    const tl = this.screenToWorld(0, 0), br = this.screenToWorld(width, height);
    this.terrainLayer.update({ x0: tl.x, y0: tl.y, x1: br.x, y1: br.y }, this.cam.zoom * (this.app.renderer.resolution || 1), this.hour, dtMs);
    this.drawOverlay();
    this.markerLayer.sortableChildren = true;
    const sc = markerScaleFor(this.cam.zoom);
    this.markerScale = sc;
    const far = sc < NEAR_SCALE;
    if (far !== this.far) {
      // Detail level changed: every marker's body texture changes.
      this.far = far;
      for (const m of this.markers.values()) {
        if (!m.look) continue;
        const spec: CreatureSpec = { look: m.look.spec.look, juvenile: m.look.spec.juvenile };
        if (far) spec.far = true;
        m.look = { ...m.look, spec };
        m.style = [creatureKey(spec), ...m.style.split('|').slice(1)].join('|');
        m.dirty = true;
      }
    }
    for (const m of this.markers.values()) {
      const p = this.worldToScreen(m.x, m.y);
      const root = m.parts.root;
      root.position.set(p.x, p.y);
      root.scale.set(sc);
      const pad = 40 * sc;
      root.visible = p.x > -pad && p.y > -pad && p.x < width + pad && p.y < height + pad;
      // Far away, only danger icons stay (the rest would be noise at that size).
      m.parts.icon.visible = m.hint !== null && (!far || m.hint === 'flee');
    }
    this.highlight.clear();
    const sel = this.selected !== null ? this.markers.get(this.selected) : undefined;
    if (sel) {
      // A white ellipse at the feet, a little wider than the owner ring,
      // breathing slowly so it is found at a glance.
      const p = this.worldToScreen(sel.x, sel.y);
      const pulse = 1 + 0.06 * Math.sin(now / 320);
      const rx = Math.max(10, sel.r * 2.05 * sc) * pulse;
      const ry = rx * 0.42;
      const y = p.y + sel.r * 0.95 * sc;
      this.highlight.ellipse(p.x, y, rx, ry).stroke({ width: 3.5, color: 0x0d1117, alpha: 0.45 });
      this.highlight.ellipse(p.x, y, rx, ry).stroke({ width: 1.75, color: MARK.select, alpha: 0.95 });
    }
    this.drawVision(dtMs);
  }

  /** The own organism whose cones show: the selected one if it is yours, else the possessed one. */
  private visionTarget(): OwnOrganism | null {
    const s = this.state;
    if (!s || !this.showVision) return null;
    const has = (o: OwnOrganism | undefined): o is OwnOrganism => !!o && !!o.vision && o.heading !== undefined && this.markers.has(o.id);
    const sel = this.selected !== null ? s.organisms.find((o) => o.id === this.selected) : undefined;
    if (has(sel)) return sel;
    const pos = s.organisms.find((o) => o.possessed);
    return has(pos) ? pos : null;
  }

  /** Vision cones on screen (null when none are drawn). */
  visionInfo(): VisionInfo | null {
    return this.vision;
  }

  private drawVision(dtMs: number): void {
    const g = this.visionLayer;
    g.clear();
    this.vision = null;
    const o = this.visionTarget();
    const m = o ? this.markers.get(o.id) : undefined;
    if (!o || !m || !o.vision || o.heading === undefined) {
      this.drawnHeading = null;
      return;
    }
    const target = headingDeg(o.heading);
    const prev = this.drawnHeading?.id === o.id ? this.drawnHeading.deg : target;
    const deg = easeAngle(prev, target, 1 - Math.exp(-dtMs / TURN_TAU_MS));
    this.drawnHeading = { id: o.id, deg };
    const p = this.worldToScreen(m.x, m.y);
    // A few body lengths, growing a little with zoom; readable on a phone.
    const r = Math.round(Math.min(150, Math.max(64, this.cam.zoom * 5, m.r * this.markerScale * 5)));
    const layout = coneLayout(o.vision, deg);
    const counts = drawCones(g, p.x, p.y, r, layout);
    this.vision = {
      organism: o.id, heading: target, drawn: deg, radius: r, x: p.x, y: p.y,
      covered: layout.covered.length, blind: layout.blind.length, overlap: layout.overlap.length, eyes: layout.eyes.length, ...counts,
    };
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
    // The world's edge: a soft dark falloff outside it.
    for (let k = 1; k <= 3; k++) g.rect(-k * 3 * px, -k * 3 * px, this.worldW + k * 6 * px, this.worldH + k * 6 * px).stroke({ width: 3 * px, color: 0x000000, alpha: 0.16 / k });
    const s = this.state;
    if (!s) return;
    // The view window: corner brackets and a faint hairline, not a box.
    const v = s.view;
    const vx0 = v.x - v.radius, vy0 = v.y - v.radius, vx1 = v.x + v.radius, vy1 = v.y + v.radius;
    g.rect(vx0, vy0, vx1 - vx0, vy1 - vy0).stroke({ width: px, color: MARK.view, alpha: 0.14 });
    const arm = Math.min(v.radius * 0.5, 18 * px);
    for (const [cx, cy, sx, sy] of [[vx0, vy0, 1, 1], [vx1, vy0, -1, 1], [vx0, vy1, 1, -1], [vx1, vy1, -1, -1]]) {
      g.moveTo(cx + sx * arm, cy).lineTo(cx, cy).lineTo(cx, cy + sy * arm);
    }
    g.stroke({ width: 3.5 * px, color: 0x0d1117, alpha: 0.35, cap: 'round', join: 'round' });
    for (const [cx, cy, sx, sy] of [[vx0, vy0, 1, 1], [vx1, vy0, -1, 1], [vx0, vy1, 1, -1], [vx1, vy1, -1, -1]]) {
      g.moveTo(cx + sx * arm, cy).lineTo(cx, cy).lineTo(cx, cy + sy * arm);
    }
    g.stroke({ width: 1.75 * px, color: MARK.view, alpha: 0.7, cap: 'round', join: 'round' });
    // Each group's territory (a tinted disc, the picked group's outline
    // bolder) and migration target (a cross in a ring; pink for a lone group).
    // Other players' territories: public ground, a grey outline.
    for (const t of s.territories ?? []) {
      g.circle(t.x, t.y, t.radius).stroke({ width: 1.5 * px, color: 0x9aa4ad, alpha: 0.7 });
    }
    const groups = drawnGroups(s);
    for (const grp of groups) {
      const color = groups.length > 1 ? groupColor(grp.id) : 0xf778ba;
      const t = grp.territory;
      if (t) {
        const picked = this.selectedGroup === grp.id;
        const tint = groupColor(grp.id);
        g.circle(t.x, t.y, t.radius).fill({ color: tint, alpha: picked ? 0.2 : 0.12 });
        g.circle(t.x, t.y, t.radius).stroke({ width: (picked ? 3 : 1.5) * px, color: tint, alpha: 0.9 });
        if (grp.home_range) {
          // The home range (shielded while the player is offline): a gold
          // second ring just outside and a small house at the center.
          const gold = 0xe3b341;
          g.circle(t.x, t.y, t.radius + 4 * px).stroke({ width: 1.5 * px, color: gold, alpha: 0.9 });
          const hs = 5 * px;
          g.rect(t.x - hs, t.y - hs * 0.2, 2 * hs, 1.2 * hs).fill({ color: gold, alpha: 0.95 });
          g.poly([t.x - 1.4 * hs, t.y - 0.2 * hs, t.x, t.y - 1.4 * hs, t.x + 1.4 * hs, t.y - 0.2 * hs]).fill({ color: gold, alpha: 0.95 });
        }
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
      const st = grp.home;
      if (st) {
        // Settling after a migration: a faint circle, no flag.
        g.circle(st.x, st.y, st.radius).stroke({ width: 1 * px, color: groupColor(grp.id), alpha: 0.35 });
      }
      if (grp.migration) {
        const [mx, my] = grp.migration;
        // The planned way there (around water and thin ice).
        const route = grp.route ?? [];
        if (route.length >= 2) {
          g.moveTo(route[0][0], route[0][1]);
          for (const [rx, ry] of route.slice(1)) g.lineTo(rx, ry);
          g.stroke({ width: 1.5 * px, color, alpha: 0.55 });
        }
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

  /** Every organism within its tap radius of a screen point: own first, then nearest. */
  hitAll(sx: number, sy: number, touch = false): OrgId[] {
    const min = touch ? TOUCH_HIT_PX : HIT_PX;
    const hits: { id: OrgId; d: number; own: boolean }[] = [];
    for (const [id, m] of this.markers) {
      const p = this.worldToScreen(m.x, m.y);
      const d = (p.x - sx) ** 2 + (p.y - sy) ** 2;
      const reach = Math.max(min, m.r * 1.6 * this.markerScale);
      if (d <= reach * reach) hits.push({ id, d, own: m.own });
    }
    return hits.sort((a, b) => Number(b.own) - Number(a.own) || a.d - b.d).map((h) => h.id);
  }

  /** A tap's click as of when the finger went down (`hit`, `near`), for the organisms still on the map; else as of when it lifted. */
  private tapOn(c: WorldClick, hit: OrgId | null, near: OrgId[]): WorldClick {
    if (hit === null || !this.markers.has(hit)) return c;
    return { ...c, organism: hit, candidates: near.filter((id) => this.markers.has(id)) };
  }

  private bindInput(canvas: HTMLCanvasElement): void {
    // Active pointers (mouse, pen or fingers) by id, at their latest position.
    const pointers = new Map<number, { x: number; y: number }>();
    // The single-pointer gesture in progress: a tap, a long press or a drag.
    let press: { id: number; x: number; y: number; camX: number; camY: number; touch: boolean; dragging: boolean; timer: number | null; done: boolean; hit: OrgId | null; near: OrgId[]; modifier: boolean } | null = null;
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
    const click = (x: number, y: number, touch: boolean, modifier = false): WorldClick => {
      const w = this.screenToWorld(x, y);
      return { x: w.x, y: w.y, organism: this.hitTest(x, y, touch), candidates: this.hitAll(x, y, touch), modifier, touch };
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
      this.pressing = true;
      const touch = e.pointerType === 'touch' || e.pointerType === 'pen';
      // The organisms under the finger when it went down: a tap or a long
      // press acts on them even if their markers glide away before it lifts
      // (animals move every frame; on a slow device a tap spans several).
      const pr = { id: e.pointerId, x: p.x, y: p.y, camX: this.cam.x, camY: this.cam.y, touch, dragging: false, timer: null as number | null, done: false,
        hit: this.hitTest(p.x, p.y, touch), near: this.hitAll(p.x, p.y, touch), modifier: e.shiftKey || e.ctrlKey || e.altKey || e.metaKey };
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
      if (pointers.size === 0) this.pressing = false;
      if (pinch) {
        // Lifting one finger of a pinch ends it; the other doesn't start a drag or tap.
        if (pointers.size < 2) pinch = null;
        return;
      }
      if (!press || e.pointerId !== press.id) return;
      const pr = press;
      cancelPress();
      if (!cancelled && !pr.dragging && !pr.done) this.onClick(this.tapOn(click(pr.x, pr.y, pr.touch, pr.modifier), pr.hit, pr.near));
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

/** Draw the cones of `l` around a screen point; returns the hatch strokes and dots drawn. */
function drawCones(g: Graphics, cx: number, cy: number, r: number, l: ConeLayout): { hatch: number; dots: number } {
  // Each eye's field: its own translucent blue wedge, so crossings read darker.
  for (const w of l.eyes) g.poly(wedgePolygon(cx, cy, r, w)).fill({ color: CONE_SEEN, alpha: 0.13 });
  // Blind spots: greyed out and hatched at 45° like the builder's pattern.
  let hatch = 0;
  for (const w of l.blind) {
    g.poly(wedgePolygon(cx, cy, r, w)).fill({ color: 0x0d1117, alpha: 0.3 });
    g.poly(wedgePolygon(cx, cy, r, w)).fill({ color: CONE_BLIND, alpha: 0.16 });
    for (const [x1, y1, x2, y2] of hatchSegments(cx, cy, r, w, 6)) {
      g.moveTo(x1, y1).lineTo(x2, y2);
      hatch++;
    }
    g.stroke({ width: 1.3, color: CONE_BLIND, alpha: 0.6 });
    g.poly(wedgePolygon(cx, cy, r, w)).stroke({ width: 1, color: CONE_BLIND, alpha: 0.75 });
  }
  // The depth zone in front: green and dotted.
  let dots = 0;
  for (const w of l.overlap) {
    g.poly(wedgePolygon(cx, cy, r, w)).fill({ color: CONE_DEPTH, alpha: 0.22 });
    for (const [x, y] of dotGrid(cx, cy, r - 2, w, 6, 6)) {
      g.circle(x, y, 1.3);
      dots++;
    }
    g.fill({ color: CONE_DEPTH, alpha: 0.9 });
    g.poly(wedgePolygon(cx, cy, r, w)).stroke({ width: 1, color: CONE_DEPTH, alpha: 0.9 });
  }
  if (l.occluded) g.poly(wedgePolygon(cx, cy, r * 0.8, l.occluded)).fill({ color: 0x0d1117, alpha: 0.35 });
  // Each eye's field: a dashed blue outline just inside the rim.
  l.eyes.forEach((w, i) => {
    const rr = r - 3 - 3 * i;
    const pts = arcPoints(cx, cy, rr, w, 3);
    for (let k = 0; k + 1 < pts.length; k += 2) g.moveTo(pts[k][0], pts[k][1]).lineTo(pts[k + 1][0], pts[k + 1][1]);
    for (const end of [pts[0], pts[pts.length - 1]]) {
      const dx = end[0] - cx, dy = end[1] - cy;
      for (let t = 0.25; t < 0.95; t += 0.2) g.moveTo(cx + dx * t, cy + dy * t).lineTo(cx + dx * (t + 0.1), cy + dy * (t + 0.1));
    }
  });
  g.stroke({ width: 1, color: CONE_SEEN, alpha: 0.75 });
  // Facing: a small arrowhead on the rim.
  const a = (l.heading * Math.PI) / 180;
  const tip = { x: cx + Math.cos(a) * (r + 7), y: cy + Math.sin(a) * (r + 7) };
  const side = (s: number) => ({ x: cx + Math.cos(a + s * 0.09) * (r - 2), y: cy + Math.sin(a + s * 0.09) * (r - 2) });
  const [l1, r1] = [side(-1), side(1)];
  g.poly([tip.x, tip.y, l1.x, l1.y, r1.x, r1.y]).fill({ color: 0xffffff, alpha: 0.9 }).stroke({ width: 1, color: 0x0d1117, alpha: 0.6 });
  return { hatch, dots };
}

function statusOf(o: OwnOrganism): StatusKind | null {
  return o.possessed ? 'possessed' : o.pregnant ? 'pregnant' : null;
}

function applyMarker(sprites: CreatureSprites, parts: MarkerParts, spec: CreatureSpec, own: boolean, tint: number, status: StatusKind | null, hint: Hint | null): void {
  parts.ring.texture = sprites.ownerRing(spec, own);
  parts.ring.tint = tint;
  parts.body.texture = sprites.body(spec);
  parts.status.visible = status !== null;
  if (status !== null) {
    parts.status.texture = sprites.statusMark(spec, status);
    parts.status.tint = status === 'possessed' ? MARK.possessed : MARK.pregnant;
  }
  parts.icon.visible = hint !== null;
  if (hint !== null) {
    parts.icon.texture = sprites.hint(hint);
    const r = bodyRadius(spec);
    // Small, just above the head on the facing side.
    parts.icon.scale.set(0.8);
    parts.icon.position.set(r * 1.1, -r * 1.75 - 3);
  }
}

/** The groups the map draws: an emptied group (every member dead) shows no territory, rally or target. */
function drawnGroups(s: Parameters<typeof groupsOf>[0]): ReturnType<typeof groupsOf> {
  return groupsOf(s).filter((g) => g.id === 0 || g.members > 0);
}

