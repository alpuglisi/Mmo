import { describe, expect, it } from 'vitest';
import { capInfo, grouped, migrationText } from './lineageInfo';
import type { GroupView } from './protocol';

describe('the population cap row', () => {
  it('says when births are paused, near the cap, or nothing', () => {
    expect(capInfo(1500, 1500)).toMatchObject({ count: '1,500 / 1,500', status: 'population cap reached: births paused', level: 'full' });
    expect(capInfo(1400, 1500)).toMatchObject({ status: 'near the population cap', level: 'near' });
    expect(capInfo(300, 1500)).toMatchObject({ count: '300 / 1,500', status: null, level: 'ok' });
    expect(capInfo(1500, 1500).explain).toMatch(/young included.*no young are born/);
    // An older server without the cap: just the count.
    expect(capInfo(1234, undefined)).toEqual({ count: '1,234', status: null, level: 'ok', explain: '' });
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
