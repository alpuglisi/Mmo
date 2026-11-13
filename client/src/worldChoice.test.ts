import { describe, expect, it } from 'vitest';
import { setApiBase, setWorld, withWorld, wsUrl } from './api';
import type { WorldList } from './protocol';
import { describeWorld, parseMemory, pickWorld, remember, remembered } from './worldChoice';

const list: WorldList = {
  default: 'main',
  worlds: [
    { id: 'main', name: 'Main', width: 128, height: 128, day: 3, year: 0, season: 'spring', population: 1200, lineages: 5, players_online: 1 },
    { id: 'isle', name: 'Isle', width: 64, height: 64, day: 9, year: 0, season: 'summer', population: 300, lineages: 2, players_online: 4 },
  ],
};

describe('world choice', () => {
  it('remembers a world per user and server, and the last one chosen', () => {
    let m = parseMemory(null);
    expect(remembered(m, '', 'ann')).toBeNull();
    m = remember(m, '', 'Ann', 'isle');
    m = remember(m, '', 'bob', 'main');
    m = remember(m, 'https://other.example', 'ann', 'far');
    expect(remembered(m, '', 'ann ')).toBe('isle');
    expect(remembered(m, '', 'bob')).toBe('main');
    // Someone new on this server gets the last world chosen here.
    expect(remembered(m, '', 'cat')).toBe('main');
    expect(remembered(m, '', '')).toBe('main');
    expect(remembered(m, 'https://other.example', 'ann')).toBe('far');
    // It survives storage.
    expect(parseMemory(JSON.stringify(m))).toEqual(m);
    expect(parseMemory('{not json')).toEqual({ last: {}, byUser: {} });
    expect(parseMemory('{"last": 3, "byUser": {"a": 1}}')).toEqual({ last: {}, byUser: {} });
  });

  it('preselects the remembered world only if the server has it', () => {
    expect(pickWorld(list, 'isle')).toBe('isle');
    expect(pickWorld(list, 'gone')).toBe('main');
    expect(pickWorld(list, null)).toBe('main');
    expect(describeWorld(list.worlds[1])).toBe('64×64, day 9 (summer), 300 organisms, 4 players online');
    expect(describeWorld(list.worlds[0])).toContain('1 player online');
  });

  it('adds the chosen world to per-world paths and the socket', () => {
    setWorld('');
    expect(withWorld('/api/world')).toBe('/api/world');
    setWorld('isle 2');
    expect(withWorld('/api/world')).toBe('/api/world?world=isle%202');
    expect(withWorld('/api/leaderboard/seasons?limit=8')).toBe('/api/leaderboard/seasons?limit=8&world=isle%202');
    expect(withWorld('/api/world', 'main')).toBe('/api/world?world=main');
    setApiBase('http://h:1');
    expect(wsUrl('t')).toBe('ws://h:1/ws?token=t&proto=2&world=isle%202');
    setApiBase('');
    setWorld('');
  });
});
