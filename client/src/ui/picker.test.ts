import { describe, expect, it } from 'vitest';
import { pickMatches, type PickItem, type PickerState } from './picker';

const all: PickItem[] = Array.from({ length: 3000 }, (_, i) => ({
  id: i + 1, label: `#${i + 1} ${i % 2 ? 'male' : 'female'}`, sex: i % 2 ? 'male' : 'female',
  pregnant: i % 2 === 0 && i % 4 === 0, free: i % 10 !== 0, distance: (i * 37) % 101,
}));
const none: PickerState = { query: '', sex: 'any', free: false, notPregnant: false };

describe('pickMatches', () => {
  it('caps a huge lineage at the nearest few', () => {
    const { shown, total } = pickMatches(all, none);
    expect(total).toBe(3000);
    expect(shown).toHaveLength(50);
    for (let i = 1; i < shown.length; i++) expect(shown[i].distance!).toBeGreaterThanOrEqual(shown[i - 1].distance!);
  });

  it('filters by sex, free and pregnancy, and finds ids by prefix', () => {
    const f = pickMatches(all, { ...none, sex: 'female', notPregnant: true, free: true }, 5000).shown;
    expect(f.length).toBeGreaterThan(0);
    expect(f.every((x) => x.sex === 'female' && !x.pregnant && x.free !== false)).toBe(true);
    const byId = pickMatches(all, { ...none, query: '#2690' }).shown;
    expect(byId.map((x) => x.id)).toEqual([2690]);
    expect(pickMatches(all, { ...none, query: '26' }, 5000).shown.every((x) => String(x.id).startsWith('26'))).toBe(true);
  });

  it('puts disabled items after the pickable ones', () => {
    const items: PickItem[] = [{ id: 1, label: 'a', disabled: true, distance: 0 }, { id: 2, label: 'b', distance: 50 }];
    expect(pickMatches(items, none).shown.map((x) => x.id)).toEqual([2, 1]);
  });
});
