import { describe, expect, it } from 'vitest';
import { designWarnings, serverText } from './designWarnings';
import type { DesignSpec, WorldSummary } from './protocol';

const design = (traits: Record<string, number> = {}, founders = 10): DesignSpec =>
  ({ name: 'x', thermo: 'warm', birth: 'live', litter_size: 2, founders, traits } as DesignSpec);
const world = (drought: number, catastrophe?: object) =>
  ({ weather: { mean_wind: 0, storm: false, drought, flood: 0, catastrophe }, clock: { day: 10 } } as unknown as WorldSummary);

describe('design warnings', () => {
  it('warns of a drought at any founding size, louder when extreme', () => {
    expect(designWarnings(design({}, 5), world(0.1))).toEqual([]);
    const d = designWarnings(design({}, 5), world(0.35));
    expect(d.map((w) => [w.id, w.level])).toEqual([['drought', 'warn']]);
    // From 40% founding now is called dangerous.
    const f = designWarnings(design({}, 5), world(0.42))[0];
    expect(f.level).toBe('bad');
    expect(f.text).toContain('can die out within days');
    // The server's text wins when it sends one.
    expect(designWarnings(design(), world(0.1), 'founding now in a drought (playtest 6: x) is risky')[0].text).toBe('Founding now in a drought is risky');
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
  it('warns that warm-blooded abandoned eggs chill and die', () => {
    const w = designWarnings({ ...design(), birth: 'abandoned_eggs' } as DesignSpec, null);
    expect(w.map((x) => [x.id, x.level])).toEqual([['eggs', 'bad']]);
    expect(w[0].text).toContain('Warm-blooded eggs left unattended chill and die unless the weather is hot');
    expect(designWarnings({ ...design(), birth: 'abandoned_eggs', thermo: 'cold' } as DesignSpec, null)).toEqual([]);
  });
  it('shows server text without developers\' notes', () => {
    expect(serverText('warm eggs chill in the first cool night (playtest 5: 252 chilled, none hatched); choose incubated eggs'))
      .toBe('Warm eggs chill in the first cool night; choose incubated eggs');
    expect(serverText('Too close to "Herd" (another player)')).toBe('Too close to "Herd" (another player)');
  });
});
