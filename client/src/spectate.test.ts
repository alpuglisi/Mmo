import { describe, expect, it } from 'vitest';
import {
  catastropheLine, catastropheTint, clockLine, densityOf, discRadius, lerpDensity, lineageColor, parseWatch, pickFollowed, PopulationHistory, topLineages,
} from './spectate';
import type { Spectate, SpectateLineage } from './protocol';

const lin = (lineage: number, alive: number, wild: boolean, cells: [number, number, number][] = []): SpectateLineage => ({
  lineage, name: `L${lineage}`, owner: wild ? 'wild' : 'ann', wild, alive, adults: alive, cells,
});
const spec = (day: number, lineages: SpectateLineage[]): Spectate => ({
  clock: { tick: day * 48, day, year: 0, hour: 0, season: 'spring', daytime: true, era: 0, era_days: 30 },
  width: 64, height: 64, cell: 8, catastrophes: [], lineages,
});

describe('parseWatch', () => {
  it('reads the watch link', () => {
    expect(parseWatch('?watch=isle&lineage=3&stream=1')).toEqual({ world: 'isle', lineage: 3, stream: true });
    expect(parseWatch('?watch=1')).toEqual({ world: '', lineage: null, stream: false });
    expect(parseWatch('?watch=&lineage=x')).toEqual({ world: '', lineage: null, stream: false });
    expect(parseWatch('?world=isle')).toBeNull();
  });
});

describe('pickFollowed', () => {
  const s = spec(1, [lin(0, 500, true), lin(4, 40, false), lin(5, 90, false), lin(6, 0, false)]);
  it('follows the asked lineage, else the largest player lineage', () => {
    expect(pickFollowed(s, 4)?.lineage).toBe(4);
    expect(pickFollowed(s, 6)?.lineage).toBe(6);
    expect(pickFollowed(s, 99)?.lineage).toBe(5);
    expect(pickFollowed(s, null)?.lineage).toBe(5);
    expect(pickFollowed(spec(1, [lin(0, 5, true), lin(1, 9, true)]), null)?.lineage).toBe(1);
    expect(pickFollowed(spec(1, []), null)).toBeNull();
  });
});

describe('density', () => {
  it('fades cells in and out between answers', () => {
    const a = densityOf(spec(1, [lin(1, 10, false, [[0, 0, 10], [1, 0, 4]])]));
    const b = densityOf(spec(2, [lin(1, 10, false, [[0, 0, 2], [2, 2, 8]])]));
    const mid = lerpDensity(a, b, 0.5);
    expect(mid.get('1:0:0')).toBe(6);
    expect(mid.get('1:1:0')).toBe(2);
    expect(mid.get('1:2:2')).toBe(4);
    expect(lerpDensity(a, b, 2).get('1:2:2')).toBe(8);
    expect(lerpDensity(a, b, -1).get('1:2:2')).toBe(0);
  });

  it('grows discs with the head count, within reach of the cell', () => {
    expect(discRadius(0, 8)).toBe(0);
    expect(discRadius(4, 8)).toBeLessThan(discRadius(16, 8));
    expect(discRadius(10_000, 8)).toBeLessThanOrEqual(8);
  });
});

describe('PopulationHistory', () => {
  it('keeps one point per day, aligned across lineages', () => {
    const h = new PopulationHistory(3);
    expect(h.push(spec(1, [lin(1, 5, false)]))).toBe(true);
    expect(h.push(spec(1, [lin(1, 6, false)]))).toBe(false);
    h.push(spec(2, [lin(1, 7, false), lin(2, 3, true)]));
    h.push(spec(3, [lin(1, 8, false)]));
    expect(h.series.get(2)).toEqual([0, 3, 0]);
    h.push(spec(4, [lin(1, 9, false)]));
    expect(h.days).toEqual([2, 3, 4]);
    expect(h.series.get(1)).toEqual([7, 8, 9]);
    expect(h.peak([1, 2])).toBe(9);
  });
});

describe('captions and colours', () => {
  it('names the clock and catastrophes, and colours lineages', () => {
    expect(clockLine(spec(240, []).clock)).toBe('Year 1 · Spring · Day 241');
    const cat = { id: 1, kind: 'severe_winter', severity: 0.8, start_day: 1, end_day: 9, active: true, population_before: 1, players_before: [], survivors: [] };
    expect(catastropheLine(cat)).toBe('Severe winter sweeps the land · 80% severity');
    expect(catastropheTint('megadrought')[3]).toBeGreaterThan(0);
    const l = lin(3, 1, false);
    expect(lineageColor(l, 3).fill).not.toBe(lineageColor(l, null).fill);
    expect(topLineages(spec(1, [lin(0, 9, true), lin(1, 3, false), lin(2, 0, false)]), 1).map((x) => x.lineage)).toEqual([1, 0]);
  });
});

describe('lineageFacts', () => {
  it('shows the share once there is one', async () => {
    const { lineageFacts } = await import('./spectate');
    expect(lineageFacts({ ...lin(1, 20, false), share: 0 })).toBe('20 alive');
    expect(lineageFacts({ ...lin(1, 20, false), share: 0.187 })).toBe('20 alive · 18.7% of the genes');
  });
});
