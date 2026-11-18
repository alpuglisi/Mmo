import { describe, expect, it } from 'vitest';
import { seasonNotice } from './seasonNotice';

describe('season notice', () => {
  it('says nothing while winter is far off or the server does not tell', () => {
    expect(seasonNotice({ days_to_winter: 31, winter_provision: false })).toBeNull();
    expect(seasonNotice({})).toBeNull();
    expect(seasonNotice(null)).toBeNull();
  });
  it('counts down the last 30 days and names what helps', () => {
    const t = seasonNotice({ days_to_winter: 12, winter_provision: true })!;
    expect(t).toContain('Winter starts in 12 days.');
    expect(t).toContain('Fur, fat stores and hibernation');
    expect(t).toContain('start fed');
    expect(seasonNotice({ days_to_winter: 1, winter_provision: false })).toContain('in 1 day.');
    expect(seasonNotice({ days_to_winter: 1, winter_provision: false })).not.toContain('start fed');
    expect(seasonNotice({ days_to_winter: 0, winter_provision: true })).toContain('It is winter now.');
  });
  it('advises brumation for a cold-blooded design and mentions the free first winter', () => {
    const t = seasonNotice({ days_to_winter: 5, winter_provision: true }, 'cold')!;
    expect(t).toContain('Brumation');
    expect(t).toContain('first winter free');
    expect(seasonNotice({ days_to_winter: 5, winter_provision: true })).toContain('hibernation');
  });
});
