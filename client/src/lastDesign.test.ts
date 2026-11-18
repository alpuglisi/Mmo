import { describe, expect, it } from 'vitest';
import { restoreDesign } from './lastDesign';
import type { GeneCatalog } from './protocol';

const cat = {
  budget: 100, max_litter: 8, base_founders: 20, cost_per_extra_offspring: 1, cost_per_extra_founder: 1,
  genes: [{ name: 'muscle', default: 0, cost: 4 }, { name: 'carnivory', default: 0, cost: 0 }],
} as unknown as GeneCatalog;

describe('last design', () => {
  it('comes back with known genes and valid values only, and no name', () => {
    const d = restoreDesign({ name: 'Old', thermo: 'cold', birth: 'abandoned_eggs', litter_size: 6, founders: 30,
      traits: { muscle: 0.7, carnivory: 2, ghost: 0.5 }, morphology: { eye_count: 3 } }, cat)!;
    expect(d.name).toBe('');
    expect([d.thermo, d.birth, d.litter_size, d.founders]).toEqual(['cold', 'abandoned_eggs', 6, 30]);
    expect(d.traits.muscle).toBe(0.7);
    expect(d.traits.carnivory).not.toBe(2);
    expect('ghost' in d.traits).toBe(false);
    expect(d.morphology.eye_count).toBe(3);
  });
  it('falls back to the starter for nonsense', () => {
    expect(restoreDesign(null, cat)).toBeNull();
    const d = restoreDesign({ thermo: 'lukewarm', litter_size: 99, founders: 1 }, cat)!;
    expect(d.thermo).toBe('warm');
    expect(d.litter_size).toBeLessThanOrEqual(8);
    expect(d.founders).toBe(20);
  });
});
