import { describe, expect, it } from 'vitest';
import { capInfo, graceText, grouped, LEGACY_EXPLAIN, migrationText, rampText } from './lineageInfo';
import type { GroupView } from './protocol';

describe('the population cap row', () => {
  it('says when births are paused, near the cap, or nothing', () => {
    expect(capInfo(1500, 1500)).toMatchObject({ count: '1,500 / 1,500', status: 'population cap reached: births paused', level: 'full' });
    expect(capInfo(1400, 1500)).toMatchObject({ status: 'near the population cap', level: 'near' });
    expect(capInfo(300, 1500)).toMatchObject({ count: '300 / 1,500', status: null, level: 'ok' });
    expect(capInfo(1500, 1500).explain).toMatch(/young included.*no young are born/);
    // An older server without the cap: just the count.
    expect(capInfo(1234, undefined)).toEqual({ count: '1,234', status: null, level: 'ok', explain: '' });
  });

  it('explains the soft cap from lineage_stats.crowding', () => {
    const c = (count: number, litter: number, hunger = 1) => ({ soft_cap: 1500, hard_cap: 3000, count, juvenile_weight: 0.25, litter, hunger });
    const calm = capInfo(400, 1500, c(350, 0.997));
    expect(calm).toMatchObject({ count: '350 / 1,500', status: null, level: 'ok' });
    expect(calm.explain).toMatch(/young count as 0.25 of an adult/);
    expect(calm.explain).toMatch(/soft cap of 1,500 \(half size at it/);
    expect(calm.explain).toMatch(/hard cap of 3,000 living organisms/);
    expect(calm.explain).not.toMatch(/births stop at 1,500|at most 1,500/);
    expect(capInfo(1400, 1500, c(1300, 0.64)).status).toBe('breeding slowing: litters ×0.6');
    // Not "full size" at ×0.95: the factor is said.
    expect(capInfo(1100, 1500, c(1000, 0.95)).explain).toMatch(/Now: litters are ×0.9 their size|Now: litters are ×0.95 their size/);
    const past = capInfo(2100, 1500, c(1800, 0.325, 1.06));
    expect(past).toMatchObject({ status: 'past the soft cap: litters ×0.3, hunger ×1.06', level: 'near' });
    expect(past.explain).toMatch(/6% hungrier/);
    expect(capInfo(3000, 1500, c(2600, 0.1, 1.22))).toMatchObject({ status: 'hard cap reached: births paused', level: 'full' });
    // Within 10% of the hard cap (young count): said before it is reached.
    expect(capInfo(2750, 1500, c(2300, 0.15, 1.2))).toMatchObject({ status: 'near the hard cap: births stop at 3,000', level: 'near' });
    // Crowding off (soft = hard): the plain cap.
    expect(capInfo(1500, 1500, { ...c(1500, 1), hard_cap: 1500 }).status).toBe('population cap reached: births paused');
    expect(grouped(1500)).toBe('1,500');
  });
});

describe('the migration row', () => {
  const g = (id: number, migration?: [number, number], members = 5): GroupView => ({ id, members, adults: members, max_radius: 4, ...(migration ? { migration } : {}) });
  it('names every migrating group, not just the main one', () => {
    expect(migrationText(null, [g(0), g(1, [40, 52])])).toBe('Group B toward 40, 52');
    expect(migrationText([10, 12], [g(0, [10, 12])])).toBe('Group A (main) toward 10, 12');
    expect(migrationText(null, [g(0), g(1, [3, 4], 0)])).toBe('none');
    expect(migrationText([1, 2], undefined)).toBe('toward 1, 2');
  });
});

describe('action words', async () => {
  const { actionText } = await import('./lineageInfo');
  it('says a starving or parched resting animal is searching', () => {
    expect(actionText('rest', { hunger: 100, thirst: 10 })).toBe('searching for food (starving)');
    expect(actionText('rest', { hunger: 90, thirst: 95 })).toBe('searching for water (parched)');
    expect(actionText('rest', { hunger: 20, thirst: 10 })).toBe('rest');
    expect(actionText('seek_mate', { hunger: 100, thirst: 0 })).toBe('seek mate');
    expect(actionText('rest')).toBe('rest');
  });
});

describe('grace and the founding ramp', () => {
  it('says how long the grace lasts in game and real time', () => {
    const span = (t: number) => `${(t / 48).toFixed(1)} days`;
    expect(graceText(undefined, 48, span)).toBeNull();
    expect(graceText(0, 48, span)).toBeNull();
    expect(graceText(3.5, 48, span)).toMatch(/^3.5 days left: other players can't order attacks/);
    // With the server's real time (a fast clock's minimum of real minutes), and two-sided.
    expect(graceText(250, 48, span, 1500)).toMatch(/^250 days \(≈ 25 min\) left: .*yours can't be ordered to attack theirs/);
  });
  it('explains the ramp and the weighted share', () => {
    expect(rampText(undefined)).toBeNull();
    expect(rampText({ founded_day: 99, weight: 0.4, days: 10 })).toBe('40% of your share counts toward the ranked season now; it rises to 100% over the 10 days after founding (day 100)');
    expect(rampText({ founded_day: 3, weight: 1, days: 10 })).toMatch(/^full/);
    expect(LEGACY_EXPLAIN).toMatch(/stays at 0% until your first young grow up/);
  });
});
