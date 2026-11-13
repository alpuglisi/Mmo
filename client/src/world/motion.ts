// Smooth movement between state messages (pure math, no PixiJS).
//
// The server is authoritative and pushes `state` at its own pace: every
// state interval (1 s by default) for everyone, and after each possession
// sub-step (a quarter tick) to the owner of a walking possessed organism.
// Drawn as received, markers would hop at each message. Two remedies:
//
// - **Interpolation** (`Track`): every marker glides from where it is drawn
//   to each new server position over the time the previous move took, so an
//   organism that moves once a tick moves continuously, one message behind.
// - **Prediction** (`Predictor`): the player's possessed organism walks on
//   from its last server position toward its move order at its known speed
//   (`OwnOrganism.speed`, tiles per tick, over the measured tick length), so
//   a click answers at once and sub-step messages only nudge it. It never
//   leads the server by more than half a tick of movement, stops before a
//   tile that isn't plainly open ground in the client's terrain (the server
//   paths around water and cliffs, which straight-line prediction can't), and
//   stops when the server says the organism isn't following the order (an
//   urgent need or danger took over), detours, stalls or arrives. The drawn
//   position eases toward the predicted one (`smoothToward`), so a correction
//   slides rather than jumps unless it is beyond a snap distance.

import type { OwnOrganism } from '../protocol';
import { B, type TerrainGrid } from './terrain';

export interface Point {
  x: number;
  y: number;
}

/** The sim's `ORDER_ARRIVAL`: an order within this many tiles is done. */
export const ORDER_ARRIVAL = 0.75;
/** The furthest prediction runs ahead of the last server position, in ticks of movement. */
export const LEAD_TICKS = 0.5;
/** A local order the server hasn't confirmed is dropped after this long. */
export const PENDING_MS = 1500;
/** No server movement for this many ticks: the server is stuck, so stop predicting. */
export const STALL_TICKS = 1.5;
/** Longest and shortest glide between two server positions. */
export const GLIDE_MIN_MS = 50;
export const GLIDE_MAX_MS = 1200;
/** A move longer than this (tiles) snaps instead of gliding (a reconnect, a teleport). */
export const SNAP_TILES = 6;
/** The predicted marker snaps to its goal beyond this distance, else eases. */
export const RECONCILE_SNAP = 3;
/** Easing time constant toward the predicted position. */
export const RECONCILE_TAU_MS = 90;
/** Step (tiles) at which the terrain is checked along a predicted path. */
const PROBE = 0.25;

/**
 * Milliseconds per world tick, measured from `clock.tick` against arrival
 * times (the protocol doesn't state the tick length; `--fast` and manual
 * clocks differ). Starts at the default 1 s.
 */
export class TickClock {
  msPerTick = 1000;
  private tick = -1;
  private at = 0;
  private samples = 0;

  observe(tick: number, now: number): void {
    if (this.tick < 0 || tick < this.tick) {
      this.tick = tick;
      this.at = now;
      return;
    }
    if (tick === this.tick) return;
    const per = (now - this.at) / (tick - this.tick);
    this.tick = tick;
    this.at = now;
    if (!(per > 0)) return;
    const clamped = Math.min(5000, Math.max(5, per));
    // The first samples settle it quickly, later ones smooth out jitter.
    const a = this.samples < 3 ? 0.6 : 0.25;
    this.msPerTick += (clamped - this.msPerTick) * a;
    this.samples++;
  }
}

/**
 * Is the tile under (x, y) plainly open ground that any body can cross:
 * land that isn't mountain, without standing water or an unfrozen river?
 * Conservative on purpose: prediction stops where this says no, and the
 * server (which knows the body) decides.
 */
export function isOpenGround(g: TerrainGrid | null, x: number, y: number): boolean {
  if (!g || !(x >= 0 && y >= 0 && x < g.width && y < g.height)) return false;
  const i = Math.floor(y) * g.width + Math.floor(x);
  const b = g.biome[i];
  if (b === B.deep || b === B.shallow || b === B.mountain) return false;
  if (g.water[i] >= 25) return false;
  return g.river[i] === 0 || g.ice[i] >= 200;
}

export interface PredictParams {
  /** Last server position. */
  from: Point;
  target: Point;
  /** Tiles per millisecond on open ground. */
  speed: number;
  /** Milliseconds since `from` was current. */
  elapsedMs: number;
  /** Furthest (tiles) to run ahead of `from`. */
  maxLead: number;
  /** Terrain check; positions where it fails are never predicted. */
  passable?: (x: number, y: number) => boolean;
}

/**
 * Where the organism should be now: `from` advanced straight toward the
 * target, by its speed over the elapsed time, at most `maxLead`, stopping at
 * the arrival distance and before the first tile `passable` rejects.
 */
export function predict(p: PredictParams): Point {
  const dx = p.target.x - p.from.x;
  const dy = p.target.y - p.from.y;
  const d = Math.hypot(dx, dy);
  const room = d - ORDER_ARRIVAL;
  if (!(room > 0) || !(p.speed > 0) || !(p.elapsedMs > 0)) return { x: p.from.x, y: p.from.y };
  const adv = Math.min(p.speed * p.elapsedMs, Math.max(0, p.maxLead), room);
  const ux = dx / d;
  const uy = dy / d;
  if (!p.passable) return { x: p.from.x + ux * adv, y: p.from.y + uy * adv };
  // Walk the segment and keep the last point before a rejected tile.
  let ok = 0;
  for (let s = Math.min(PROBE, adv); ; s = Math.min(s + PROBE, adv)) {
    if (!p.passable(p.from.x + ux * s, p.from.y + uy * s)) break;
    ok = s;
    if (s >= adv) break;
  }
  return { x: p.from.x + ux * ok, y: p.from.y + uy * ok };
}

/**
 * Ease `cur` toward `goal` (exponential, time constant `tauMs`, frame-rate
 * independent); snap when they are more than `snap` tiles apart.
 */
export function smoothToward(cur: Point, goal: Point, dtMs: number, tauMs: number, snap: number): Point {
  const dx = goal.x - cur.x;
  const dy = goal.y - cur.y;
  if (dx * dx + dy * dy > snap * snap || !(tauMs > 0)) return { x: goal.x, y: goal.y };
  const k = 1 - Math.exp(-Math.max(0, dtMs) / tauMs);
  return { x: cur.x + dx * k, y: cur.y + dy * k };
}

/**
 * A marker's glide between server positions: from where it was drawn when
 * the new position arrived to that position, over the time the previous
 * move took (clamped), so steady movement stays continuous. Plain numbers so
 * hundreds of markers cost nothing per frame.
 */
export interface Track {
  fx: number;
  fy: number;
  tx: number;
  ty: number;
  /** When the current glide started (ms). */
  t0: number;
  dur: number;
  /** When the server position last changed (ms). */
  changed: number;
}

export function newTrack(x: number, y: number, now: number): Track {
  return { fx: x, fy: y, tx: x, ty: y, t0: now, dur: 0, changed: now };
}

/** Where the glide is at `now`. */
export function trackAt(t: Track, now: number): Point {
  if (!(t.dur > 0) || now >= t.t0 + t.dur) return { x: t.tx, y: t.ty };
  const u = Math.max(0, (now - t.t0) / t.dur);
  return { x: t.fx + (t.tx - t.fx) * u, y: t.fy + (t.ty - t.fy) * u };
}

/**
 * A new server position (x, y) arrived at `now`, with the marker drawn at
 * `cur`. No change keeps the glide going; a long jump snaps. True if it moved.
 */
export function retarget(t: Track, cur: Point, x: number, y: number, now: number): boolean {
  if (x === t.tx && y === t.ty) return false;
  const dur = Math.min(GLIDE_MAX_MS, Math.max(GLIDE_MIN_MS, now - t.changed));
  const far = Math.abs(cur.x - x) + Math.abs(cur.y - y) > SNAP_TILES;
  t.fx = far ? x : cur.x;
  t.fy = far ? y : cur.y;
  t.tx = x;
  t.ty = y;
  t.t0 = now;
  t.dur = far ? 0 : dur;
  t.changed = now;
  return true;
}

/**
 * Client-side prediction for the possessed organism. Feed it the player's
 * own orders (`order`, `reject`) and each state (`onState`); `goal` says
 * where to draw it now, or null when the server's position (interpolated)
 * is all there is to show.
 */
export class Predictor {
  id: number | null = null;
  /** Last server position and when it first arrived. */
  private sx = 0;
  private sy = 0;
  private st = 0;
  /** When the server position last changed. */
  private movedAt = 0;
  private action = '';
  private speed = 0;
  private serverOrder: Point | null = null;
  /** An order sent but not yet seen in a state. */
  private pending: (Point & { at: number }) | null = null;
  /** The server's last move didn't head for the order (a route around something). */
  private detour = false;

  /** The order the drawing should show: a pending one, else the server's. */
  target(): Point | null {
    return this.pending ?? this.serverOrder;
  }

  /** The player sent `order_move` for the possessed organism. */
  order(x: number, y: number, now: number): void {
    this.pending = { x, y, at: now };
    this.detour = false;
  }

  /** The server refused the order (or it failed to send). */
  reject(): void {
    this.pending = null;
  }

  onState(o: OwnOrganism | undefined, now: number): void {
    if (!o || !o.possessed) {
      this.reset();
      return;
    }
    if (this.id !== o.id) {
      this.reset();
      this.id = o.id;
      this.sx = o.x;
      this.sy = o.y;
      this.st = this.movedAt = now;
    }
    this.action = o.action;
    this.speed = o.speed ?? 0;
    this.serverOrder = o.order ? { x: o.order[0], y: o.order[1] } : null;
    const p = this.pending;
    if (p && (now - p.at > PENDING_MS || (this.serverOrder && Math.hypot(this.serverOrder.x - p.x, this.serverOrder.y - p.y) < 0.5))) {
      this.pending = null;
    }
    const dx = o.x - this.sx;
    const dy = o.y - this.sy;
    if (dx !== 0 || dy !== 0) {
      const t = this.serverOrder;
      if (t && o.action === 'ordered') {
        // Compare the move with the straight line from where it started.
        const tx = t.x - this.sx;
        const ty = t.y - this.sy;
        const cos = (dx * tx + dy * ty) / (Math.hypot(dx, dy) * Math.hypot(tx, ty) || 1);
        this.detour = cos < 0.7;
      }
      this.sx = o.x;
      this.sy = o.y;
      // A repeat of an unchanged position keeps the time it first arrived,
      // so the prediction doesn't fall back.
      this.st = this.movedAt = now;
    }
  }

  /** Where to draw the possessed organism at `now`, or null for no prediction. */
  goal(now: number, msPerTick: number, passable?: (x: number, y: number) => boolean): Point | null {
    if (this.id === null || !(this.speed > 0) || !(msPerTick > 0)) return null;
    const pending = this.pending && now - this.pending.at <= PENDING_MS ? this.pending : null;
    const target = pending ?? this.serverOrder;
    if (!target) return null;
    if (!pending) {
      // The server has the order: predict only while it says it follows it.
      if (this.action !== 'ordered' || this.detour) return null;
      if (now - this.movedAt > STALL_TICKS * msPerTick) return null;
    }
    const since = pending ? Math.max(this.st, pending.at) : this.st;
    return predict({
      from: { x: this.sx, y: this.sy },
      target,
      speed: this.speed / msPerTick,
      elapsedMs: now - since,
      maxLead: this.speed * LEAD_TICKS,
      passable,
    });
  }

  private reset(): void {
    this.id = null;
    this.pending = null;
    this.serverOrder = null;
    this.detour = false;
    this.speed = 0;
    this.action = '';
  }
}
