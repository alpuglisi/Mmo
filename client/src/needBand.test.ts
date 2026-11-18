import { describe, expect, it } from 'vitest';
import { NeedBand, bandText } from './needBand';

describe('need band', () => {
  it('keeps one day of readings and says the range when it swings', () => {
    const b = new NeedBand();
    const day = 100;
    [[0, 14], [20, 41], [40, 87], [60, 55]].forEach(([t, v]) => b.add(7, t, v, day));
    expect(b.band(7, day)).toEqual({ lo: 14, hi: 87, mean: 49.25 });
    expect(bandText(b.band(7, day))).toBe('14–87 over the last day, about 49 on average');
    // A day later the oldest readings are gone.
    b.add(7, 130, 60, day);
    expect(b.band(7, day)!.lo).toBe(55);
    // Steady readings need no band; too few readings give none.
    [[0, 30], [20, 33], [40, 31]].forEach(([t, v]) => b.add(8, t, v, day));
    expect(bandText(b.band(8, day))).toBeNull();
    expect(b.band(9, day)).toBeNull();
  });
});
