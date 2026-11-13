import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { backoffDelay, GameSocket, type ConnState } from './socket';
import { GameModel } from './model';
import type { OfferView, ServerMsg } from './protocol';

/** A scriptable stand-in for the browser WebSocket. */
class FakeWs {
  static all: FakeWs[] = [];
  readyState = 0;
  sent: string[] = [];
  onopen: (() => void) | null = null;
  onclose: (() => void) | null = null;
  onerror: (() => void) | null = null;
  onmessage: ((ev: { data: string }) => void) | null = null;
  constructor(public url: string) {
    FakeWs.all.push(this);
  }
  send(s: string) { this.sent.push(s); }
  close() { this.readyState = 3; this.onclose?.(); }
  open() { this.readyState = 1; this.onopen?.(); }
  receive(m: object) { this.onmessage?.({ data: JSON.stringify(m) }); }
  drop() { this.readyState = 3; this.onclose?.(); }
}

describe('backoffDelay', () => {
  it('doubles from the minimum up to the cap', () => {
    const mid = () => 0.5; // no jitter
    expect(backoffDelay(1, 500, 15000, mid)).toBe(500);
    expect(backoffDelay(2, 500, 15000, mid)).toBe(1000);
    expect(backoffDelay(4, 500, 15000, mid)).toBe(4000);
    expect(backoffDelay(20, 500, 15000, mid)).toBe(15000);
    expect(backoffDelay(1, 500, 15000, () => 0)).toBe(400);
    expect(backoffDelay(1, 500, 15000, () => 1)).toBe(600);
  });
});

describe('GameSocket', () => {
  beforeEach(() => {
    FakeWs.all = [];
    vi.useFakeTimers();
  });
  afterEach(() => vi.useRealTimers());

  function make() {
    const states: ConnState[] = [];
    const msgs: ServerMsg[] = [];
    const sock = new GameSocket({
      url: () => 'ws://test/ws?token=t',
      onMessage: (m) => msgs.push(m),
      onState: (s) => states.push(s),
      WebSocketImpl: FakeWs as unknown as typeof WebSocket,
      minDelayMs: 100,
      maxDelayMs: 1000,
    });
    return { sock, states, msgs };
  }

  it('pairs results with requests by id', async () => {
    const { sock, msgs } = make();
    sock.start();
    const ws = FakeWs.all[0];
    ws.open();
    const p = sock.send({ type: 'possess', organism: 7 });
    const sent = JSON.parse(ws.sent[0]);
    expect(sent).toEqual({ type: 'possess', organism: 7, id: sent.id });
    ws.receive({ type: 'result', id: sent.id, command: 'possess', ok: false, error: { code: 'not_owned', message: 'no' } });
    const r = await p;
    expect(r.ok).toBe(false);
    expect(r.error?.code).toBe('not_owned');
    expect(msgs).toHaveLength(1);
  });

  it('answers locally when not connected', async () => {
    const { sock } = make();
    const r = await sock.send({ type: 'clear_view' });
    expect(r.error?.code).toBe('not_connected');
  });

  it('reconnects with backoff after a drop and fails pending requests', async () => {
    const { sock, states } = make();
    sock.start();
    FakeWs.all[0].open();
    const pending = sock.send({ type: 'clear_migration' });
    FakeWs.all[0].drop();
    expect((await pending).error?.code).toBe('disconnected');
    await vi.advanceTimersByTimeAsync(0);
    expect(states.at(-1)).toBe('reconnecting');
    expect(FakeWs.all).toHaveLength(1);
    await vi.advanceTimersByTimeAsync(200);
    expect(FakeWs.all).toHaveLength(2);
    FakeWs.all[1].open();
    expect(states.at(-1)).toBe('open');
    sock.stop();
    expect(states.at(-1)).toBe('closed');
  });

  it('stops retrying when told the token is bad', async () => {
    const states: ConnState[] = [];
    const sock = new GameSocket({
      url: () => 'ws://x',
      onMessage: () => {},
      onState: (s) => states.push(s),
      onFailedAttempt: () => false,
      WebSocketImpl: FakeWs as unknown as typeof WebSocket,
    });
    sock.start();
    FakeWs.all[0].drop(); // closed without opening
    await vi.advanceTimersByTimeAsync(60000);
    expect(FakeWs.all).toHaveLength(1);
    expect(states.at(-1)).toBe('closed');
  });
});

describe('GameModel offers', () => {
  const offer = (status: OfferView['status']): OfferView => ({
    id: 1, from_player: 2, from_name: 'bob', to_player: 1, to_name: 'ann', mine: 5, partner: 6, status, created_tick: 0,
  });

  it('tracks a request through acceptance to its outcome', () => {
    const m = new GameModel();
    m.player = 1;
    m.apply({ type: 'mating_request', offer: offer('pending') });
    expect(m.offers.get(1)?.incoming).toBe(true);
    m.apply({ type: 'mating_update', offer: offer('accepted'), error: { code: 'too_far', message: 'too far apart' } });
    expect(m.offers.get(1)?.offer.status).toBe('accepted');
    m.apply({ type: 'mating_update', offer: offer('completed'), outcome: { mismatch: 0, cost: 0, failure_chance: 0, fertility: 1, conceived: true } });
    expect(m.offers.get(1)?.outcome?.conceived).toBe(true);
    expect(m.toasts.length).toBeGreaterThan(0);
  });

  it('drops open offers the state no longer lists, keeps closed ones', () => {
    const m = new GameModel();
    m.player = 1;
    m.apply({ type: 'mating_request', offer: offer('pending') });
    m.apply({ type: 'mating_update', offer: { ...offer('declined'), id: 2 } });
    const base = {
      clock: { tick: 0, day: 0, year: 0, hour: 0, season: 'spring', daytime: true },
      weather: { mean_wind: 0, storm: false, drought: 0, flood: 0 },
      player: 1, lineage: 1, slots_left: 6, population: 0, migration: null, view: { x: 0, y: 0, radius: 16 },
      organisms: [], nearby: [], leaderboard: { day: null, breeding_adults: 0, rows: [] },
    };
    m.apply({ type: 'state', ...base, offers: [] });
    expect(m.offers.has(1)).toBe(false);
    expect(m.offers.get(2)?.offer.status).toBe('declined');
  });

  it('turns failed results into error toasts', () => {
    const m = new GameModel();
    m.apply({ type: 'result', id: 1, command: 'possess', ok: false, error: { code: 'possessed_by_other', message: 'taken' } });
    expect(m.toasts[0]).toEqual({ kind: 'error', text: 'possess: taken (possessed_by_other)' });
  });
});
