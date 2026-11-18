import { describe, expect, it } from 'vitest';
import { droughtNotice } from './droughtInfo';

describe('the drought notice', () => {
  it('shows from 45%, stays until below 30%, and gives way to a megadrought', () => {
    expect(droughtNotice(0.36, false).on).toBe(false);
    const n = droughtNotice(0.48, false);
    expect(n.on).toBe(true);
    expect(n.title).toBe('Drought: 48% of the land is dry');
    expect(n.advice).toMatch(/near water/);
    expect(droughtNotice(0.35, true).on).toBe(true);
    expect(droughtNotice(0.29, true).on).toBe(false);
    expect(droughtNotice(0.6, false, true).on).toBe(false);
  });
});
