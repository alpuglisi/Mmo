import { describe, expect, it } from 'vitest';
import { designWarnings } from './designWarnings';
import type { DesignSpec, WorldSummary } from './protocol';

const design = (traits: Record<string, number> = {}, founders = 10): DesignSpec =>
  ({ name: 'x', thermo: 'warm', birth: 'live', litter_size: 2, founders, traits } as DesignSpec);
const world = (drought: number, catastrophe?: object) =>
  ({ weather: { mean_wind: 0, storm: false, drought, flood: 0, catastrophe }, clock: { day: 10 } } as unknown as WorldSummary);

describe('design warnings', () => {
  it('warns of a drought at any founding size, louder when extreme', () => {
    expect(designWarnings(design({}, 5), world(0.1))).toEqual([]);
    const d = designWarnings(design({}, 5), world(0.4));
    expect(d.map((w) => [w.id, w.level])).toEqual([['drought', 'warn']]);
    const x = designWarnings(design({}, 5), world(0.94))[0];
    expect(x.level).toBe('bad');
    expect(x.text).toContain('Extreme drought: 94%');
  });

  it('names the catastrophe under way', () => {
    const w = designWarnings(design(), world(0, { kind: 'megadrought', active: true, end_day: 14 }));
    expect(w[0].id).toBe('catastrophe');
    expect(w[0].text).toMatch(/under way \(4 days left\)/);
    expect(designWarnings(design(), world(0, { kind: 'severe_winter', active: true, end_day: 11 }))[0].text).toContain('(its last day)');
    // Over (or the summary not caught up yet): no warning.
    expect(designWarnings(design(), world(0, { kind: 'severe_winter', active: true, end_day: 10 }))).toEqual([]);
    expect(designWarnings(design(), world(0, { kind: 'severe_winter', active: false, end_day: 14 }))).toEqual([]);
  });

  it('says when wings do nothing on a heavy body', () => {
    expect(designWarnings(design({ wings: 0.8, body_size: 0.1 }), null)).toEqual([]);
    const heavy = designWarnings(design({ wings: 0.8, body_size: 0.9 }), null);
    expect(heavy[0].id).toBe('wings');
    expect(heavy[0].text).toContain('can neither fly nor glide');
  });
});
