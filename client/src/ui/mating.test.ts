import { describe, expect, it } from 'vitest';
import type { MatingPreviewView } from '../protocol';
import { compatSummary } from './mating';

const base: MatingPreviewView = {
  mine: 1, partner: 2, compatible: true, fertility: 1, body_plan_fertility: 1, distance_fertility: 1,
  body_plan_gaps: [], mismatch: 0.4, cost: 0, failure_chance: 0, distance: 12.3, meeting_distance: 1.5,
};

describe('compatSummary', () => {
  it('reports a fertile pair inside both preferences', () => {
    const s = compatSummary(base);
    expect(s.level).toBe('ok');
    expect(s.text).toContain('fertility 100%');
    expect(s.text).toContain('within both preferences');
    expect(s.text).toContain('12 tiles apart');
  });

  it('warns about reduced fertility and override costs', () => {
    const s = compatSummary({ ...base, fertility: 0.8, body_plan_fertility: 0.8, body_plan_gaps: [{ gene: 'body_size', gap: 0.15 }], mismatch: 1.6, cost: 0.36, failure_chance: 0.16, distance: 1 });
    expect(s.level).toBe('warn');
    expect(s.text).toContain('reduced by body size');
    expect(s.text).toContain('failure chance 16%');
    expect(s.text).toContain('together now');
  });

  it('names the body-plan genes of an incompatible pair', () => {
    const s = compatSummary({
      ...base, compatible: false, fertility: 0, body_plan_fertility: 0,
      body_plan_gaps: [{ gene: 'endothermy', gap: 0.8 }, { gene: 'birth_mode', gap: 0.7 }],
      problem: { code: 'incompatible_body_plans', message: 'different body plans' },
    });
    expect(s.level).toBe('bad');
    expect(s.text).toBe('Incompatible: different body plans (endothermy 0.80 apart, birth mode 0.70 apart). They cannot conceive.');
    expect(compatSummary({ ...base, compatible: false, fertility: 0, distance_fertility: 0 }).text).toContain('outbreeding');
  });
});
