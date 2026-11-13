import { describe, expect, it } from 'vitest';
import { isOpen, parseServerMsg, type StateView } from './protocol';

const offer = {
  id: 4, from_player: 1, from_name: 'ann', to_player: 2, to_name: 'bob',
  mine: 10, partner: 20, price: 5, status: 'pending', created_tick: 99,
};

// Shaped like serde's output for ServerMsg::State (a newtype variant: the
// state's fields sit beside "type").
const state: StateView = {
  clock: { tick: 120, day: 2, year: 0, hour: 7, season: 'spring', daytime: true, era: 0, era_days: 360 },
  weather: { mean_wind: 0.2, storm: false, drought: 0, flood: 0.1 },
  player: 1,
  lineage: 3,
  slots_left: 6,
  population: 2,
  migration: null,
  view: { x: 30, y: 31, radius: 16 },
  organisms: [{
    id: 10, lineage: 3, x: 30.5, y: 31, action: 'graze', sex: 'female', age_days: 30, growth: 1, matured: true,
    health: 90, needs: { hunger: 20, thirst: 10, energy: 80, cold: 0, heat: 0, safety: 0, breeding: 30, social: 5 },
    possessed: false, pregnant: false, generation: 0,
  }],
  nearby: [{ id: 20, owner: 2, lineage: 4, x: 33, y: 29, action: 'wander', sex: 'male', matured: true }],
  leaderboard: { day: null, breeding_adults: 0, rows: [] },
  offers: [],
};

describe('parseServerMsg', () => {
  it('parses every server message type', () => {
    expect(parseServerMsg('{"type":"welcome","player":1,"name":"ann","lineage":null}'))
      .toEqual({ type: 'welcome', player: 1, name: 'ann', lineage: null });
    expect(parseServerMsg('{"type":"welcome","player":2,"name":"bob","lineage":7}')).toMatchObject({ lineage: 7 });

    const s = parseServerMsg(JSON.stringify({ type: 'state', ...state }));
    expect(s?.type).toBe('state');
    if (s?.type === 'state') {
      expect(s.organisms[0].needs.hunger).toBe(20);
      expect(s.nearby[0].owner).toBe(2);
    }

    const ok = parseServerMsg('{"type":"result","id":3,"command":"possess","ok":true}');
    expect(ok).toEqual({ type: 'result', id: 3, command: 'possess', ok: true });
    const err = parseServerMsg('{"type":"result","id":4,"command":"order_move","ok":false,"error":{"code":"not_owned","message":"organism 5 is not yours"}}');
    expect(err?.type === 'result' && err.error?.code).toBe('not_owned');

    expect(parseServerMsg(JSON.stringify({ type: 'mating_request', offer }))?.type).toBe('mating_request');
    const upd = parseServerMsg(JSON.stringify({
      type: 'mating_update', offer: { ...offer, status: 'completed' },
      outcome: { mismatch: 0.1, cost: 0.2, failure_chance: 0.05, fertility: 0.9, conceived: true },
    }));
    expect(upd?.type === 'mating_update' && upd.outcome?.conceived).toBe(true);

    const alert = parseServerMsg('{"type":"alert","alert":{"tick":96,"day":2,"kind":"death","text":"Founder #3 was eaten.","organism":3}}');
    expect(alert?.type === 'alert' && alert.alert.organism).toBe(3);
    expect(parseServerMsg('{"type":"session_ended","reason":"expired"}')).toEqual({ type: 'session_ended', reason: 'expired' });
    expect(parseServerMsg('{"type":"alert","alert":{"kind":"death"}}')).toBeNull();
  });

  it('rejects malformed or unknown messages', () => {
    expect(parseServerMsg('not json')).toBeNull();
    expect(parseServerMsg('[1,2]')).toBeNull();
    expect(parseServerMsg('{"no":"type"}')).toBeNull();
    expect(parseServerMsg('{"type":"teleport"}')).toBeNull();
    expect(parseServerMsg('{"type":"welcome","player":"one","name":"ann"}')).toBeNull();
    expect(parseServerMsg('{"type":"result","command":"possess"}')).toBeNull();
    expect(parseServerMsg('{"type":"result","command":"x","ok":false,"error":"boom"}')).toBeNull();
    const { organisms: _o, ...noOrgs } = state;
    expect(parseServerMsg(JSON.stringify({ type: 'state', ...noOrgs }))).toBeNull();
    expect(parseServerMsg(JSON.stringify({ type: 'mating_request', offer: { id: 1 } }))).toBeNull();
  });

  it('parses the economy fields: balance, escrow, listing offers and market alerts', () => {
    const s = parseServerMsg(JSON.stringify({ type: 'state', ...state, balance: 42, escrow: 10, offers: [{ ...offer, status: 'accepted', listing: 3, escrow: 10 }] }));
    expect(s?.type === 'state' && [s.balance, s.escrow, s.offers[0].listing, s.offers[0].escrow]).toEqual([42, 10, 3, 10]);
    // An older server without the economy still parses (no balance).
    const old = parseServerMsg(JSON.stringify({ type: 'state', ...state }));
    expect(old?.type === 'state' && old.balance).toBeUndefined();
    const a = parseServerMsg('{"type":"alert","alert":{"tick":5,"day":0,"kind":"market","text":"Sale: 12 renown from bob"}}');
    expect(a?.type === 'alert' && a.alert.kind).toBe('market');
  });

  it('knows which offer statuses are open', () => {
    expect(isOpen('pending')).toBe(true);
    expect(isOpen('accepted')).toBe(true);
    expect(isOpen('completed')).toBe(false);
    expect(isOpen('expired')).toBe(false);
  });
});
