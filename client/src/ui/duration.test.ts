import { describe, expect, it } from 'vitest';
import { gameAndReal, gameTime, realTime, ticksToNextSeason } from './duration';

describe('durations', () => {
  it('prints real time at a sensible precision', () => {
    expect(realTime(400)).toBe('≈ 1 s');
    expect(realTime(40_000)).toBe('≈ 40 s');
    expect(realTime(3 * 60_000)).toBe('≈ 3 min');
    expect(realTime(130 * 60_000)).toBe('≈ 2 h 10 min');
    expect(realTime(5 * 86_400_000)).toBe('≈ 5 days');
  });

  it('prints game time in hours under a day, else days', () => {
    expect(gameTime(12, 48)).toBe('6 h');
    expect(gameTime(120, 48)).toBe('2.5 days');
    expect(gameTime(48 * 30, 48)).toBe('30 days');
  });

  it('pairs game and real time at the measured speed', () => {
    // A fast server: 100 ms a tick, 48 ticks a day (a day ≈ 5 s).
    expect(gameAndReal(96, 48, 100)).toBe('2.0 days (≈ 10 s)');
    // The normal 1 s tick: a game day is 48 s.
    expect(gameAndReal(48 * 3, 48, 1000)).toBe('3.0 days (≈ 2 min)');
  });

  it('counts down to the next season (quarters of the year)', () => {
    // 120-day year, 48 ticks a day: seasons start on days 0, 30, 60, 90.
    expect(ticksToNextSeason(0, 48, 120)).toBe(30 * 48);
    expect(ticksToNextSeason(29 * 48 + 10, 48, 120)).toBe(48 - 10);
    expect(ticksToNextSeason(95 * 48, 48, 120)).toBe(25 * 48);
    // Into the next year.
    expect(ticksToNextSeason(120 * 48, 48, 120)).toBe(30 * 48);
  });
});
