import { describe, expect, it } from 'vitest';
import type { OwnOrganism } from '../protocol';
import {
  GLIDE_MAX_MS, GLIDE_MIN_MS, isOpenGround, LEAD_TICKS, newTrack, ORDER_ARRIVAL, PENDING_MS, predict, Predictor,
  retarget, smoothToward, SNAP_TILES, TickClock, trackAt,
} from './motion';
import { B, type TerrainGrid } from './terrain';

const near = (a: { x: number; y: number }, b: { x: number; y: number }, eps = 1e-6) =>
  expect(Math.hypot(a.x - b.x, a.y - b.y)).toBeLessThan(eps);

function grid(w: number, h: number, biome: number = B.grass): TerrainGrid {
  const n = w * h;
  const z = () => new Uint8Array(n);
  return {
    width: w, height: h, version: 1, biome: new Uint8Array(n).fill(biome), elevation: z(), river: z(), baseWater: z(),
    water: z(), ice: z(), snow: z(), veg: z(), wetness: z(), tick: 0, day: 0, hour: 0, season: 'spring',
  };
}

function own(over: Partial<OwnOrganism> = {}): OwnOrganism {
  return {
    id: 7, lineage: 1, x: 10, y: 10, action: 'ordered', sex: 'female', age_days: 30, growth: 1, matured: true, health: 90,
    needs: { hunger: 10, thirst: 10, energy: 80, cold: 0, heat: 0, safety: 0, breeding: 0, social: 0 },
    possessed: true, pregnant: false, generation: 0, order: [20, 10], speed: 1, ...over,
  };
}

describe('TickClock', () => {
  it('measures the tick length from clock ticks over arrival times', () => {
    const c = new TickClock();
    expect(c.msPerTick).toBe(1000);
    let now = 0;
    for (let tick = 100; tick < 130; tick++) {
      c.observe(tick, now);
      // Sub-step states repeat the tick: they don't count.
      c.observe(tick, now + 25);
      now += 100;
    }
    expect(c.msPerTick).toBeGreaterThan(95);
    expect(c.msPerTick).toBeLessThan(105);
  });

  it('restarts when the tick goes back (a fresh world) and ignores nonsense', () => {
    const c = new TickClock();
    c.observe(50, 0);
    c.observe(51, 0); // zero elapsed: ignored
    expect(c.msPerTick).toBe(1000);
    c.observe(10, 5000);
    c.observe(12, 5400);
    expect(c.msPerTick).toBeLessThan(1000);
    expect(c.msPerTick).toBeGreaterThan(200);
  });
});

describe('isOpenGround', () => {
  it('accepts plain land and refuses water, mountains, rivers and the map edge', () => {
    const g = grid(4, 4);
    expect(isOpenGround(g, 1.5, 1.5)).toBe(true);
    expect(isOpenGround(null, 1.5, 1.5)).toBe(false);
    expect(isOpenGround(g, -0.1, 1)).toBe(false);
    expect(isOpenGround(g, 4, 1)).toBe(false);
    const at = (x: number, y: number) => y * 4 + x;
    g.biome[at(1, 1)] = B.deep;
    g.biome[at(2, 1)] = B.mountain;
    g.biome[at(3, 1)] = B.shallow;
    g.water[at(0, 2)] = 40; // flooded grass
    g.river[at(1, 2)] = 120;
    g.river[at(2, 2)] = 120;
    g.ice[at(2, 2)] = 250; // frozen river: a bridge
    g.biome[at(3, 2)] = B.marsh;
    expect(isOpenGround(g, 1.2, 1.9)).toBe(false);
    expect(isOpenGround(g, 2.5, 1.5)).toBe(false);
    expect(isOpenGround(g, 3.5, 1.5)).toBe(false);
    expect(isOpenGround(g, 0.5, 2.5)).toBe(false);
    expect(isOpenGround(g, 1.5, 2.5)).toBe(false);
    expect(isOpenGround(g, 2.5, 2.5)).toBe(true);
    expect(isOpenGround(g, 3.5, 2.5)).toBe(true);
  });
});

describe('predict', () => {
  const base = { from: { x: 0, y: 0 }, target: { x: 10, y: 0 }, speed: 0.001, elapsedMs: 500, maxLead: 5 };

  it('advances straight toward the target at its speed', () => {
    near(predict(base), { x: 0.5, y: 0 });
    near(predict({ ...base, target: { x: 0, y: -10 } }), { x: 0, y: -0.5 });
    near(predict({ ...base, target: { x: 6, y: 8 } }), { x: 0.3, y: 0.4 });
  });

  it('leads by at most maxLead and stops at the arrival distance', () => {
    near(predict({ ...base, elapsedMs: 1e6, maxLead: 0.4 }), { x: 0.4, y: 0 });
    near(predict({ ...base, elapsedMs: 1e6 }), { x: 5, y: 0 });
    near(predict({ ...base, target: { x: 2, y: 0 }, elapsedMs: 1e6 }), { x: 2 - ORDER_ARRIVAL, y: 0 });
    // Already there, no time, no speed: stays put.
    near(predict({ ...base, target: { x: 0.5, y: 0 } }), { x: 0, y: 0 });
    near(predict({ ...base, elapsedMs: 0 }), { x: 0, y: 0 });
    near(predict({ ...base, elapsedMs: -50 }), { x: 0, y: 0 });
    near(predict({ ...base, speed: 0 }), { x: 0, y: 0 });
  });

  it('never enters a tile the terrain check refuses', () => {
    const wallAt = 1.6;
    const passable = (x: number) => x < wallAt;
    const p = predict({ ...base, elapsedMs: 1e6, passable });
    expect(p.x).toBeLessThan(wallAt);
    expect(p.x).toBeGreaterThan(wallAt - 0.3);
    // A wall right ahead: no prediction at all.
    near(predict({ ...base, passable: () => false }), { x: 0, y: 0 });
    // On a real grid: lake from x = 3.
    const g = grid(8, 1);
    for (let x = 3; x < 8; x++) g.biome[x] = B.deep;
    const q = predict({ from: { x: 0.5, y: 0.5 }, target: { x: 7.5, y: 0.5 }, speed: 1, elapsedMs: 100, maxLead: 10,
      passable: (x, y) => isOpenGround(g, x, y) });
    expect(q.x).toBeLessThan(3);
    expect(q.x).toBeGreaterThan(2.7);
  });
});

describe('smoothToward', () => {
  it('eases frame-rate independently and snaps beyond the threshold', () => {
    const goal = { x: 1, y: 0 };
    const one = smoothToward({ x: 0, y: 0 }, goal, 100, 100, 3);
    let two = smoothToward({ x: 0, y: 0 }, goal, 50, 100, 3);
    two = smoothToward(two, goal, 50, 100, 3);
    near(one, two, 1e-9);
    expect(one.x).toBeCloseTo(1 - Math.exp(-1), 9);
    near(smoothToward({ x: 0, y: 0 }, { x: 5, y: 0 }, 16, 100, 3), { x: 5, y: 0 });
    near(smoothToward({ x: 0, y: 0 }, goal, 0, 100, 3), { x: 0, y: 0 });
    near(smoothToward({ x: 0, y: 0 }, goal, 1e6, 100, 3), goal);
  });
});

describe('Track (interpolation between states)', () => {
  it('glides over the previous move interval, continuously for steady moves', () => {
    const t = newTrack(0, 0, 0);
    expect(retarget(t, { x: 0, y: 0 }, 1, 0, 5000)).toBe(true);
    // A first move after a long rest takes the longest glide.
    expect(t.dur).toBe(GLIDE_MAX_MS);
    near(trackAt(t, 5000), { x: 0, y: 0 });
    near(trackAt(t, 5000 + GLIDE_MAX_MS / 2), { x: 0.5, y: 0 });
    near(trackAt(t, 1e9), { x: 1, y: 0 });
    // Steady once a second: the glide lasts one interval, so the marker is
    // always moving and arrives as the next position comes in.
    retarget(t, trackAt(t, 7000), 2, 0, 7000);
    // (The first steady move still glides over the long one before it.)
    retarget(t, trackAt(t, 8000), 3, 0, 8000);
    expect(t.dur).toBe(1000);
    near(trackAt(t, 8000), { x: 1 + 1000 / GLIDE_MAX_MS, y: 0 });
    near(trackAt(t, 9000), { x: 3, y: 0 });
    retarget(t, trackAt(t, 9000), 4, 0, 9000);
    near(trackAt(t, 9500), { x: 3.5, y: 0 });
    retarget(t, trackAt(t, 10000), 5, 0, 10000);
    near(trackAt(t, 10000), { x: 4, y: 0 });
    near(trackAt(t, 10250), { x: 4.25, y: 0 });
  });

  it('keeps gliding through repeated positions, starts from where it is drawn, and snaps long jumps', () => {
    const t = newTrack(0, 0, 0);
    retarget(t, { x: 0, y: 0 }, 1, 0, 100);
    expect(t.dur).toBe(100);
    expect(retarget(t, trackAt(t, 120), 1, 0, 120)).toBe(false);
    near(trackAt(t, 150), { x: 0.5, y: 0 });
    // Mid-glide retarget: continues from the drawn point.
    retarget(t, trackAt(t, 150), 1, 1, 150);
    near(trackAt(t, 150), { x: 0.5, y: 0 });
    expect(t.dur).toBe(GLIDE_MIN_MS);
    retarget(t, trackAt(t, 1000), 1 + SNAP_TILES + 1, 1, 1000);
    near(trackAt(t, 1000), { x: SNAP_TILES + 2, y: 1 });
  });

  it('handles hundreds of markers cheaply', () => {
    const tracks = Array.from({ length: 600 }, (_, i) => newTrack(i, i, 0));
    const t0 = performance.now();
    for (let f = 0; f < 60; f++) {
      for (const t of tracks) trackAt(t, f * 16);
    }
    for (const t of tracks) retarget(t, trackAt(t, 1000), t.tx + 0.3, t.ty, 1000);
    expect(performance.now() - t0).toBeLessThan(200);
  });
});

describe('Predictor', () => {
  const MS = 1000;

  it('predicts the possessed organism toward its order at its speed, capped at half a tick ahead', () => {
    const p = new Predictor();
    p.onState(own(), 0);
    expect(p.id).toBe(7);
    // Speed 1 tile per 1000 ms tick.
    near(p.goal(100, MS)!, { x: 10.1, y: 10 });
    near(p.goal(200, MS)!, { x: 10.2, y: 10 });
    // A sub-step state moves it on; prediction continues from there.
    p.onState(own({ x: 10.25 }), 250);
    near(p.goal(300, MS)!, { x: 10.3, y: 10 });
    // A late message: the lead stays bounded.
    p.onState(own({ x: 10.5 }), 500);
    near(p.goal(1100, MS)!, { x: 10.5 + LEAD_TICKS, y: 10 });
  });

  it('reconciles: a repeated position keeps its time, a corrected one restarts from it', () => {
    const p = new Predictor();
    p.onState(own(), 0);
    p.onState(own(), 200); // the regular state, no movement since
    near(p.goal(200, MS)!, { x: 10.2, y: 10 });
    p.onState(own({ x: 10.1 }), 250); // slower terrain: the server is behind
    near(p.goal(250, MS)!, { x: 10.1, y: 10 });
  });

  it('starts at once on a local order, before the server confirms it', () => {
    const p = new Predictor();
    p.onState(own({ action: 'idle', order: undefined }), 0);
    expect(p.goal(5000, MS)).toBeNull();
    p.order(10, 0, 5000);
    expect(p.target()).toEqual({ x: 10, y: 0, at: 5000 });
    // Counts from the click, not from the stale state.
    near(p.goal(5100, MS)!, { x: 10, y: 9.9 });
    // The server confirms: its order and action take over.
    p.onState(own({ x: 10, y: 9.8, order: [10, 0] }), 5200);
    expect(p.target()).toEqual({ x: 10, y: 0 });
    near(p.goal(5300, MS)!, { x: 10, y: 9.7 });
  });

  it('stops when the server rejects the order or never confirms it', () => {
    const p = new Predictor();
    p.onState(own({ action: 'idle', order: undefined }), 0);
    p.order(10, 0, 0);
    expect(p.goal(100, MS)).not.toBeNull();
    p.reject();
    expect(p.goal(100, MS)).toBeNull();
    p.order(10, 0, 1000);
    expect(p.goal(1000 + PENDING_MS + 1, MS)).toBeNull();
  });

  it('stops when the organism is not following the order: a need, danger, arrival, release', () => {
    const p = new Predictor();
    p.onState(own({ action: 'drink' }), 0);
    expect(p.goal(100, MS)).toBeNull();
    p.onState(own({ action: 'flee', x: 9 }), 100);
    expect(p.goal(200, MS)).toBeNull();
    p.onState(own({ x: 19.5, order: undefined, action: 'idle' }), 300);
    expect(p.goal(400, MS)).toBeNull();
    p.onState(own({ possessed: false, order: undefined, speed: undefined }), 500);
    expect(p.id).toBeNull();
    expect(p.goal(600, MS)).toBeNull();
    // An organism that isn't possessed at all.
    p.onState(undefined, 700);
    expect(p.goal(800, MS)).toBeNull();
    // No speed from an older server: nothing to predict with.
    p.onState(own({ speed: undefined }), 900);
    expect(p.goal(1000, MS)).toBeNull();
  });

  it('stops when the server walks a route around something or stalls', () => {
    const p = new Predictor();
    p.onState(own(), 0);
    // Moving north while the order is east: a detour around an obstacle.
    p.onState(own({ y: 9.75 }), 250);
    expect(p.goal(300, MS)).toBeNull();
    // Back on the straight line: predicting again.
    p.onState(own({ x: 10.25, y: 9.75 }), 500);
    expect(p.goal(600, MS)).not.toBeNull();
    // No server movement for 1.5 ticks: stuck, so no prediction.
    expect(p.goal(500 + 1.5 * MS + 1, MS)).toBeNull();
  });

  it('never predicts onto terrain the client sees as impassable', () => {
    const g = grid(32, 32);
    for (let y = 0; y < 32; y++) g.biome[y * 32 + 11] = B.deep; // a channel at x = 11
    const pass = (x: number, y: number) => isOpenGround(g, x, y);
    const p = new Predictor();
    p.onState(own({ x: 10.8, speed: 4 }), 0);
    const q = p.goal(1000, MS, pass)!;
    expect(q.x).toBeLessThan(11);
    expect(isOpenGround(g, q.x, q.y)).toBe(true);
  });

  it('switching organisms resets the prediction', () => {
    const p = new Predictor();
    p.onState(own(), 0);
    p.onState(own({ id: 8, x: 3, y: 3, order: [3, 13] }), 100);
    expect(p.id).toBe(8);
    near(p.goal(200, MS)!, { x: 3, y: 3.1 });
  });
});
