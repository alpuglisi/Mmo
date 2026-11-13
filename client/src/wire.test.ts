import { describe, expect, it, vi } from 'vitest';
import fixture from './wire.fixture.json';
import { DELTA, DeltaDecoder, KEYFRAME, WireError } from './wire';
import { GameSocket } from './socket';
import type { ServerMsg, StateView } from './protocol';

// wire.fixture.json is written by the Rust test `wire::tests::the_client_fixture_is_current`
// (crates/server/src/wire.rs): the frames of a short session (ticks and
// possession sub-steps of a steering player, a keyframe in the middle) and
// the states the server's own decoder rebuilds from them.

function bytes(b64: string): Uint8Array {
  const bin = atob(b64);
  const out = new Uint8Array(bin.length);
  for (let i = 0; i < bin.length; i++) out[i] = bin.charCodeAt(i);
  return out;
}

const frames = fixture.frames.map((f) => ({ frame: bytes(f.frame), state: f.state as unknown as StateView }));

/** Deep equality, numbers within float rounding (the fixture holds f32 values). */
function expectClose(got: unknown, want: unknown, path = 'state'): void {
  if (typeof want === 'number') {
    expect(typeof got, path).toBe('number');
    expect(Math.abs((got as number) - want), `${path}: ${got} vs ${want}`).toBeLessThanOrEqual(1e-4 * Math.max(1, Math.abs(want)));
    return;
  }
  if (Array.isArray(want)) {
    expect(Array.isArray(got), path).toBe(true);
    expect((got as unknown[]).length, `${path}.length`).toBe(want.length);
    want.forEach((w, i) => expectClose((got as unknown[])[i], w, `${path}[${i}]`));
    return;
  }
  if (typeof want === 'object' && want !== null) {
    expect(typeof got === 'object' && got !== null, path).toBe(true);
    const g = got as Record<string, unknown>;
    const w = want as Record<string, unknown>;
    expect(Object.keys(g).sort(), `${path} keys`).toEqual(Object.keys(w).sort());
    for (const k of Object.keys(w)) expectClose(g[k], w[k], `${path}.${k}`);
    return;
  }
  expect(got, path).toEqual(want);
}

const kinds = frames.map((f) => f.frame[0]);
const keyAt = kinds.indexOf(KEYFRAME, 1);

describe('DeltaDecoder', () => {
  it('rebuilds every state of the fixture from keyframes and deltas', () => {
    expect(kinds[0]).toBe(KEYFRAME);
    expect(kinds).toContain(DELTA);
    expect(keyAt).toBeGreaterThan(2);
    const d = new DeltaDecoder();
    for (const [i, f] of frames.entries()) {
      expectClose(d.apply(f.frame), f.state, `frame ${i}`);
      expect(d.seq).toBe(i + 1);
    }
  });

  it('carries the possessed organism and the shared palettes through deltas', () => {
    const d = new DeltaDecoder();
    const states = frames.map((f) => d.apply(f.frame));
    const possessed = states.map((s) => s.organisms.find((o) => o.possessed));
    expect(possessed.every((p) => p && p.speed! > 0 && p.order)).toBe(true);
    // Sub-step frames (after each tick's) are small: only what moved.
    const sub = frames.filter((f, i) => i > 0 && f.frame[0] === DELTA && states[i].clock.tick === states[i - 1].clock.tick);
    expect(sub.length).toBeGreaterThan(0);
    for (const f of sub) expect(f.frame.length).toBeLessThan(200);
    expect(states.every((s) => s.nearby.every((n) => n.look && typeof n.look.diet === 'string'))).toBe(true);
  });

  it('refuses a delta after a lost frame until the next keyframe', () => {
    const d = new DeltaDecoder();
    d.apply(frames[0].frame);
    d.apply(frames[1].frame);
    // Frame 2 never arrives.
    expect(() => d.apply(frames[3].frame)).toThrow(WireError);
    try {
      d.apply(frames[3].frame);
    } catch (e) {
      expect((e as WireError).code).toBe('need_keyframe');
    }
    for (let i = 4; i < keyAt; i++) expect(() => d.apply(frames[i].frame)).toThrow(WireError);
    expect(d.seq).toBeNull();
    for (let i = keyAt; i < frames.length; i++) expectClose(d.apply(frames[i].frame), frames[i].state, `frame ${i}`);
  });

  it('reports a base mismatch and garbled frames as WireErrors', () => {
    const d = new DeltaDecoder();
    d.apply(frames[0].frame);
    let err: unknown;
    try {
      d.apply(frames[2].frame);
    } catch (e) {
      err = e;
    }
    expect((err as WireError).code).toBe('base_mismatch');
    const f = frames[1].frame;
    for (let cut = 0; cut < f.length; cut++) {
      const d2 = new DeltaDecoder();
      d2.apply(frames[0].frame);
      expect(() => d2.apply(f.subarray(0, cut)), `cut at ${cut}`).toThrow(WireError);
    }
    let seed = 7;
    const rand = () => (seed = (seed * 1103515245 + 12345) % 2147483648) / 2147483648;
    for (let i = 0; i < 300; i++) {
      const bad = f.slice();
      bad[Math.floor(rand() * bad.length)] = Math.floor(rand() * 256);
      const d2 = new DeltaDecoder();
      d2.apply(frames[0].frame);
      try {
        d2.apply(bad);
      } catch (e) {
        expect(e).toBeInstanceOf(WireError);
      }
    }
  });
});

/** A WebSocket stand-in that can deliver binary frames. */
class FakeWs {
  static all: FakeWs[] = [];
  readyState = 0;
  binaryType = 'blob';
  sent: string[] = [];
  onopen: (() => void) | null = null;
  onclose: (() => void) | null = null;
  onerror: (() => void) | null = null;
  onmessage: ((ev: { data: string | ArrayBuffer }) => void) | null = null;
  constructor(public url: string) {
    FakeWs.all.push(this);
  }
  send(s: string) { this.sent.push(s); }
  close() { this.readyState = 3; this.onclose?.(); }
  open() { this.readyState = 1; this.onopen?.(); }
  binary(b: Uint8Array) { this.onmessage?.({ data: b.slice().buffer }); }
}

describe('GameSocket with delta states', () => {
  function make() {
    FakeWs.all = [];
    const msgs: ServerMsg[] = [];
    const sock = new GameSocket({
      url: () => 'ws://test/ws?token=t&proto=2',
      onMessage: (m) => msgs.push(m),
      onState: () => {},
      WebSocketImpl: FakeWs as unknown as typeof WebSocket,
      resyncIntervalMs: 0,
    });
    sock.start();
    const ws = FakeWs.all[0];
    ws.open();
    return { sock, ws, msgs };
  }

  it('turns frames into whole state messages', () => {
    const { ws, msgs } = make();
    expect(ws.binaryType).toBe('arraybuffer');
    for (const f of frames.slice(0, 4)) ws.binary(f.frame);
    expect(msgs).toHaveLength(4);
    msgs.forEach((m, i) => {
      expect(m.type).toBe('state');
      const { type: _type, ...state } = m as ServerMsg & { type: 'state' };
      expectClose(state, frames[i].state, `message ${i}`);
    });
  });

  it('asks for a keyframe when a delta does not apply, and recovers on it', () => {
    const { sock, ws, msgs } = make();
    ws.binary(frames[0].frame);
    ws.binary(frames[2].frame); // frame 1 lost
    expect(ws.sent.map((s) => JSON.parse(s))).toEqual([{ type: 'resync' }]);
    ws.binary(frames[3].frame);
    expect(ws.sent).toHaveLength(2); // (interval 0: one request per failed frame)
    expect(sock.decodeErrors).toBe(2);
    expect(msgs).toHaveLength(1);
    ws.binary(frames[keyAt].frame);
    expect(msgs).toHaveLength(2);
    expectClose((({ type: _t, ...s }) => s)(msgs[1] as ServerMsg & { type: 'state' }), frames[keyAt].state);
  });

  it('starts every connection afresh', () => {
    vi.useFakeTimers();
    try {
      const { ws, msgs } = make();
      ws.binary(frames[0].frame);
      ws.binary(frames[1].frame);
      ws.close();
      vi.advanceTimersByTime(60_000);
      const again = FakeWs.all[1];
      again.open();
      // The new connection's decoder holds nothing: a delta is refused.
      again.binary(frames[2].frame);
      expect(msgs).toHaveLength(2);
      expect(again.sent.map((s) => JSON.parse(s))).toEqual([{ type: 'resync' }]);
      again.binary(frames[0].frame);
      expect(msgs).toHaveLength(3);
    } finally {
      vi.useRealTimers();
    }
  });
});
