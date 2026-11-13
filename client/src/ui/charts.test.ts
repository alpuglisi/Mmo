import { describe, expect, it } from 'vitest';
import type { GeneStat } from '../protocol';
import { SERIES, driftRows, histogramBars, legacySeries, linearScale, linePath, niceMax, originRows, pyramidRows, stackVitals } from './charts';
import { alertEntry, mergeFeed, MAX_FEED, type FeedEntry } from '../model';

describe('chart data prep', () => {
  it('scales linearly and rounds axis maxima', () => {
    const s = linearScale([0, 10], [100, 0]);
    expect(s(0)).toBe(100);
    expect(s(5)).toBe(50);
    expect(linearScale([3, 3], [0, 10])(3)).toBe(5);
    expect([0, 0.3, 1, 3, 7, 12, 99, 100].map(niceMax)).toEqual([1, 0.5, 1, 5, 10, 20, 100, 100]);
  });

  it('builds line paths that break at gaps', () => {
    const sx = (k: number) => k * 10;
    const sy = (v: number) => v;
    expect(linePath([1, 2, 3], sx, sy)).toBe('M0.0,1.0L10.0,2.0L20.0,3.0');
    expect(linePath([1, null, 3, 4], sx, sy)).toBe('M0.0,1.0M20.0,3.0L30.0,4.0');
    expect(linePath([NaN, 2], sx, sy)).toBe('M10.0,2.0');
    expect(linePath([], sx, sy)).toBe('');
  });

  it('stacks deaths by cause and folds rare causes into "other"', () => {
    const st = stackVitals([
      { day: 0, births: 3, deaths: { eaten: 2, thirst: 1, recessive: 1, journey: 2 } },
      { day: 1, births: 0, deaths: {} },
      { day: 2, births: 7, deaths: { old_age: 1 } },
    ]);
    expect(st.maxBirths).toBe(7);
    expect(st.maxDeaths).toBe(6);
    expect(st.days[0].segments).toEqual([
      { cause: 'eaten', y0: 0, y1: 2 },
      { cause: 'thirst', y0: 2, y1: 3 },
      { cause: 'other', y0: 3, y1: 6 },
    ]);
    expect(st.days[1].segments).toEqual([]);
    expect(st.causes).toEqual(['eaten', 'thirst', 'old_age', 'other']);
    expect(stackVitals([]).maxDeaths).toBe(0);
  });

  it('orders the age pyramid oldest first', () => {
    const p = pyramidRows([
      { from_days: 0, to_days: 10, female: 4, male: 6 },
      { from_days: 10, to_days: 20, female: 9, male: 1 },
    ]);
    expect(p.rows.map((r) => r.label)).toEqual(['10–20 d', '0–10 d']);
    expect(p.max).toBe(9);
  });

  it('picks the genes that drifted most', () => {
    const g = (gene: string, mean: number, founding: number): GeneStat => ({ gene, mean, sd: 0.01, founding_mean: founding, history: [founding, mean] });
    const rows = driftRows([g('fur', 0.5, 0.4), g('body_size', 0.3, 0.32), g('keen_nose', 0.1, 0.3), g('tail', 0.5, 0.5)], 2);
    expect(rows.map((r) => r.gene)).toEqual(['keen_nose', 'fur']);
    expect(rows[0].drift).toBeCloseTo(-0.2);
    expect(rows[0].label).toBe('keen nose');
  });

  it('converts legacy shares to percent', () => {
    const s = legacySeries([{ day: 0, share: 0.01, weighted_share: 0.03, alive: 5 }, { day: 1, share: 0.12, weighted_share: 0, alive: 6 }]);
    expect(s.share).toEqual([1, 12]);
    expect(s.weighted[0]).toBeCloseTo(3);
    expect(s.max).toBe(20);
    expect(legacySeries([]).max).toBe(1);
  });
});

describe('alerts feed', () => {
  it('merges server alerts and local entries without duplicates', () => {
    const a = alertEntry({ tick: 5, day: 0, kind: 'death', text: 'Founder #3 was eaten.', organism: 3 });
    const b: FeedEntry = { kind: 'offer', text: 'offer 1', tick: 2 };
    let feed = mergeFeed([], [a, b]);
    expect(feed.map((e) => e.tick)).toEqual([2, 5]);
    feed = mergeFeed(feed, [a]);
    expect(feed.length).toBe(2);
    const many = Array.from({ length: MAX_FEED + 5 }, (_, k): FeedEntry => ({ kind: 'birth', text: `b${k}`, tick: 10 + k }));
    feed = mergeFeed(feed, many);
    expect(feed.length).toBe(MAX_FEED);
    expect(feed[feed.length - 1].text).toBe(`b${MAX_FEED + 4}`);
  });
});

describe('gene distributions and allele origins', () => {
  it('turns bins into bars over 0..1 with shares', () => {
    const { bars, total, max } = histogramBars([0, 2, 6, 2, 0, 0, 0, 0, 0, 0]);
    expect(total).toBe(10);
    expect(max).toBe(6);
    expect(bars.length).toBe(10);
    expect(bars[2]).toMatchObject({ from: 0.2, count: 6, share: 0.6 });
    expect(bars[2].to).toBeCloseTo(0.3);
    expect(bars[9].to).toBeCloseTo(1);
    expect(histogramBars([0, 0]).bars.every((b) => b.share === 0)).toBe(true);
    expect(histogramBars([]).max).toBe(0);
  });

  it('labels origins, colors mine, the wild and rivals, and folds the tail', () => {
    const o = (player: number, share: number, lineage?: string) => ({ player, name: `p${player}`, lineage, alleles: Math.round(share * 1000), share });
    const rows = originRows([o(2, 0.2, 'Studs'), o(1, 0.6, 'Mine'), o(0, 0.1), o(3, 0.05), o(4, 0.03), o(5, 0.02)], 1, 5);
    expect(rows.map((r) => r.label)).toEqual(['you (Mine)', 'p2 (Studs)', 'wild', 'p3', '2 others']);
    expect(rows[0]).toMatchObject({ mine: true, color: SERIES[0], player: 1 });
    expect(rows[2].color).toBe(SERIES[2]);
    expect(rows[1].color).not.toBe(rows[3].color);
    expect(rows[4]).toMatchObject({ player: null, color: SERIES[6], alleles: 50 });
    expect(rows[4].share).toBeCloseTo(0.05);
    expect(originRows([o(1, 1)], 1)).toHaveLength(1);
    expect(originRows([], 1)).toEqual([]);
  });
});
