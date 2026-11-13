// Protocol 2 (`/ws?proto=2`): binary delta states.
//
// The server sends each `state` as a binary message holding only what
// changed since the state it last sent this connection; everything else
// stays JSON. This rebuilds whole `StateView`s from those frames. The format
// is described in crates/server/src/wire.rs (the source of truth: KEEP THIS
// FILE IN SYNC), and wire.fixture.json, written by a Rust test, pins the two
// together (wire.test.ts).

import type { Appearance, ClockView, GroupView, LeaderRow, NearbyOrganism, OfferView, OwnOrganism, StateView, ViewWindow, WeatherView } from './protocol';

/** The protocol this client asks for. */
export const PROTO_DELTA = 2;

export const KEYFRAME = 1;
export const DELTA = 2;

const POS = 64;
const NEED = 10;
const HEALTH = 100;
const AGE = 100;
const GROWTH = 1000;

const MALE = 1;
const MATURED = 2;
const POSSESSED = 4;
const PREGNANT = 8;

export type WireErrorCode = 'need_keyframe' | 'base_mismatch' | 'malformed';

export class WireError extends Error {
  constructor(public code: WireErrorCode, message: string) {
    super(message);
  }
}

interface OwnQ {
  lineage: number;
  x: number;
  y: number;
  action: number;
  flags: number;
  age: number;
  growth: number;
  health: number;
  needs: number[];
  generation: number;
  look: number;
  order: [number, number] | null;
  speed: number | null;
  group: number;
}

interface NearQ {
  owner: number;
  lineage: number;
  x: number;
  y: number;
  action: number;
  flags: number;
  look: number;
}

const newOwn = (): OwnQ => ({
  lineage: 0, x: 0, y: 0, action: 0, flags: 0, age: 0, growth: 0, health: 0, needs: [0, 0, 0, 0, 0, 0, 0, 0],
  generation: 0, look: 0, order: null, speed: null, group: 0,
});

const newNear = (): NearQ => ({ owner: 0, lineage: 0, x: 0, y: 0, action: 0, flags: 0, look: 0 });

interface Head {
  clock: ClockView;
  weather: WeatherView;
  player: number;
  lineage: number | null;
  slots_left: number;
  population: number;
  migration: [number, number] | null;
  groups: GroupView[];
  view: ViewWindow;
  balance: number;
  escrow: number;
  leaderboard: { day: number | null; breeding_adults: number };
}

const HEAD_KEYS: (keyof Head)[] = [
  'clock', 'weather', 'player', 'lineage', 'slots_left', 'population', 'migration', 'groups', 'view', 'balance', 'escrow', 'leaderboard',
];

const malformed = (what: string) => new WireError('malformed', what);

class Reader {
  at = 0;
  private view: DataView;
  constructor(private b: Uint8Array) {
    this.view = new DataView(b.buffer, b.byteOffset, b.byteLength);
  }

  get done(): boolean {
    return this.at === this.b.length;
  }

  u8(): number {
    if (this.at >= this.b.length) throw malformed('truncated');
    return this.b[this.at++];
  }

  /** LEB128 (exact up to 2^53). */
  v(): number {
    let x = 0;
    let mul = 1;
    for (let i = 0; i < 10; i++) {
      const b = this.u8();
      x += (b & 0x7f) * mul;
      if ((b & 0x80) === 0) {
        if (!Number.isSafeInteger(x)) throw malformed('varint out of range');
        return x;
      }
      mul *= 128;
    }
    throw malformed('varint too long');
  }

  /** Zigzag. */
  z(): number {
    const u = this.v();
    return u % 2 === 0 ? u / 2 : -(u + 1) / 2;
  }

  f32(): number {
    if (this.at + 4 > this.b.length) throw malformed('truncated');
    const f = this.view.getFloat32(this.at, true);
    this.at += 4;
    return f;
  }

  bytes(n: number): Uint8Array {
    if (this.at + n > this.b.length) throw malformed('truncated');
    const s = this.b.subarray(this.at, this.at + n);
    this.at += n;
    return s;
  }
}

function readOwn(r: Reader, o: OwnQ, m: number): void {
  if (m >= 1 << 20) throw malformed('unknown own field');
  if (m & (1 << 0)) o.lineage = r.v();
  if (m & (1 << 1)) {
    o.x += r.z();
    o.y += r.z();
  }
  if (m & (1 << 2)) o.action = r.v();
  if (m & (1 << 3)) o.flags = r.u8();
  if (m & (1 << 4)) o.age += r.z();
  if (m & (1 << 5)) o.growth += r.z();
  if (m & (1 << 6)) o.health += r.z();
  for (let i = 0; i < 8; i++) if (m & (1 << (7 + i))) o.needs[i] += r.z();
  if (m & (1 << 15)) o.generation = r.v();
  if (m & (1 << 16)) o.look = r.v();
  if (m & (1 << 17)) o.order = r.u8() === 0 ? null : [r.z(), r.z()];
  if (m & (1 << 18)) o.speed = r.u8() === 0 ? null : r.f32();
  if (m & (1 << 19)) {
    o.group = r.v();
    if (o.group > 255) throw malformed('group out of range');
  }
}

function readNear(r: Reader, o: NearQ, m: number): void {
  if (m >= 1 << 6) throw malformed('unknown nearby field');
  if (m & (1 << 0)) o.owner = r.v();
  if (m & (1 << 1)) o.lineage = r.v();
  if (m & (1 << 2)) {
    o.x += r.z();
    o.y += r.z();
  }
  if (m & (1 << 3)) o.action = r.v();
  if (m & (1 << 4)) o.flags = r.u8();
  if (m & (1 << 5)) o.look = r.v();
}

function readSection<T>(r: Reader, held: Map<number, T>, make: () => T, read: (r: Reader, o: T, m: number) => void): void {
  const gone = r.v();
  let id = 0;
  for (let i = 0; i < gone; i++) {
    id += r.v();
    if (!held.delete(id)) throw malformed('removal of an unknown id');
  }
  const n = r.v();
  id = 0;
  for (let i = 0; i < n; i++) {
    id += r.v();
    const mask = r.v();
    let o = held.get(id);
    if (!o) held.set(id, (o = make()));
    read(r, o, mask);
  }
}

const isObj = (v: unknown): v is Record<string, unknown> => typeof v === 'object' && v !== null && !Array.isArray(v);

/** The server's leaderboard order: share, highest first, then player. */
function byRank(a: LeaderRow, b: LeaderRow): number {
  return b.share - a.share || a.player - b.player;
}

const byId = (a: [number, unknown], b: [number, unknown]) => a[0] - b[0];

/**
 * One connection's view of the delta states. Make a new one per connection
 * (the server starts every connection with a keyframe). `apply` throws a
 * `WireError` for a frame it can't use, after which it waits for a keyframe:
 * send `{type: 'resync'}` to get one.
 */
export class DeltaDecoder {
  /** The seq of the state held (null until a keyframe). */
  seq: number | null = null;
  private head: Head | null = null;
  private own = new Map<number, OwnQ>();
  private near = new Map<number, NearQ>();
  private lb = new Map<number, LeaderRow>();
  private offers = new Map<number, OfferView>();
  private looks: Appearance[] = [];
  private actions: string[] = [];
  private static text = new TextDecoder();

  reset(): void {
    this.seq = null;
    this.head = null;
    this.own.clear();
    this.near.clear();
    this.lb.clear();
    this.offers.clear();
    this.looks = [];
    this.actions = [];
  }

  apply(frame: ArrayBuffer | Uint8Array): StateView {
    try {
      return this.applyInner(frame instanceof Uint8Array ? frame : new Uint8Array(frame));
    } catch (e) {
      this.reset();
      throw e instanceof WireError ? e : malformed(String(e));
    }
  }

  private applyInner(b: Uint8Array): StateView {
    const r = new Reader(b);
    const kind = r.u8();
    const seq = r.v();
    const base = r.v();
    if (kind === KEYFRAME) this.reset();
    else if (kind === DELTA) {
      if (this.seq === null) throw new WireError('need_keyframe', 'waiting for a keyframe');
      if (this.seq !== base) throw new WireError('base_mismatch', `delta on state ${base}, have ${this.seq}`);
    } else throw malformed('unknown frame kind');
    const metaBytes = r.bytes(r.v());
    const meta: unknown = metaBytes.length === 0 ? {} : JSON.parse(DeltaDecoder.text.decode(metaBytes));
    if (!isObj(meta)) throw malformed('meta is not an object');
    const head = (this.head ? { ...this.head } : {}) as Record<string, unknown>;
    for (const k of HEAD_KEYS) if (k in meta) head[k] = meta[k];
    for (const k of HEAD_KEYS) if (!(k in head)) throw malformed(`a keyframe lacks ${k}`);
    const list = (k: string): unknown[] => {
      const v = meta[k];
      if (v === undefined) return [];
      if (!Array.isArray(v)) throw malformed(`${k} is not a list`);
      return v;
    };
    for (const p of list('lb_del')) this.lb.delete(p as number);
    for (const row of list('lb_set') as LeaderRow[]) this.lb.set(row.player, row);
    for (const id of list('offers_del')) this.offers.delete(id as number);
    for (const o of list('offers_set') as OfferView[]) this.offers.set(o.id, o);
    this.looks.push(...(list('looks') as Appearance[]));
    this.actions.push(...(list('actions') as string[]));
    readSection(r, this.own, newOwn, readOwn);
    readSection(r, this.near, newNear, readNear);
    if (!r.done) throw malformed('trailing bytes');
    const state = this.state(head as unknown as Head);
    this.head = head as unknown as Head;
    this.seq = seq;
    return state;
  }

  private look(i: number): Appearance {
    const l = this.looks[i];
    if (l === undefined) throw malformed('look not in the palette');
    return l;
  }

  private action(i: number): string {
    const a = this.actions[i];
    if (a === undefined) throw malformed('action not in the palette');
    return a;
  }

  private state(h: Head): StateView {
    const organisms: OwnOrganism[] = [...this.own].sort(byId).map(([id, o]) => {
      const own: OwnOrganism = {
        id,
        lineage: o.lineage,
        x: o.x / POS,
        y: o.y / POS,
        action: this.action(o.action),
        sex: o.flags & MALE ? 'male' : 'female',
        age_days: o.age / AGE,
        growth: o.growth / GROWTH,
        matured: (o.flags & MATURED) !== 0,
        health: o.health / HEALTH,
        needs: {
          hunger: o.needs[0] / NEED, thirst: o.needs[1] / NEED, energy: o.needs[2] / NEED, cold: o.needs[3] / NEED,
          heat: o.needs[4] / NEED, safety: o.needs[5] / NEED, breeding: o.needs[6] / NEED, social: o.needs[7] / NEED,
        },
        possessed: (o.flags & POSSESSED) !== 0,
        pregnant: (o.flags & PREGNANT) !== 0,
        generation: o.generation,
        look: this.look(o.look),
      };
      if (o.order) own.order = [o.order[0] / POS, o.order[1] / POS];
      if (o.speed !== null) own.speed = o.speed;
      if (o.group !== 0) own.group = o.group;
      return own;
    });
    const nearby: NearbyOrganism[] = [...this.near].sort(byId).map(([id, o]) => ({
      id,
      owner: o.owner,
      lineage: o.lineage,
      x: o.x / POS,
      y: o.y / POS,
      action: this.action(o.action),
      sex: o.flags & MALE ? 'male' : 'female',
      matured: (o.flags & MATURED) !== 0,
      look: this.look(o.look),
    }));
    return {
      clock: h.clock,
      weather: h.weather,
      player: h.player,
      lineage: h.lineage,
      slots_left: h.slots_left,
      population: h.population,
      migration: h.migration,
      groups: h.groups,
      view: h.view,
      organisms,
      nearby,
      leaderboard: { day: h.leaderboard.day, breeding_adults: h.leaderboard.breeding_adults, rows: [...this.lb.values()].sort(byRank) },
      offers: [...this.offers].sort(byId).map(([, o]) => o),
      balance: h.balance,
      escrow: h.escrow,
    };
  }
}
