import { describe, expect, it } from 'vitest';
import { changedTraits, designCost, validateDesign } from './cost';
import type { DesignSpec, GeneCatalog } from './protocol';

// A slice of `GET /api/genes` (values as in crates/sim/src/design.rs).
const cat: GeneCatalog = {
  budget: 100,
  base_founders: 20,
  cost_per_extra_founder: 0.4,
  cost_per_extra_offspring: 1.5,
  max_litter: 12,
  genes: [
    { name: 'body_size', default: 0.3, cost: 20 },
    { name: 'muscle', default: 0, cost: 15 },
    { name: 'leg_length', default: 0, cost: 12 },
    { name: 'claws', default: 0, cost: 10 },
    { name: 'wings', default: 0, cost: 25 },
    { name: 'fur', default: 0, cost: 8 },
    { name: 'armor_plates', default: 0, cost: 15 },
    { name: 'fangs', default: 0, cost: 10 },
    { name: 'keen_nose', default: 0, cost: 8 },
    { name: 'toxins', default: 0, cost: 15 },
    { name: 'aggression', default: 0.5, cost: 0 },
  ],
};

function spec(traits: Record<string, number>, extra: Partial<DesignSpec> = {}): DesignSpec {
  return { name: 'Test', thermo: 'warm', birth: 'live', litter_size: 2, traits, founders: 20, ...extra };
}

describe('designCost', () => {
  it('is zero for a blank design with one offspring', () => {
    expect(designCost(spec({}, { litter_size: 1 }), cat).total).toBe(0);
  });

  it('matches the README example design', () => {
    // body 0.1×20 + legs 0.5×12 + fur 0.4×8 + nose 0.3×8 + one extra offspring 1.5
    const c = designCost(spec({ body_size: 0.4, leg_length: 0.5, fur: 0.4, keen_nose: 0.3 }), cat);
    expect(c.total).toBeCloseTo(15.1, 5);
    expect(c.traits).toBeCloseTo(13.6, 5);
    expect(c.litter).toBe(1.5);
    expect(c.perGene.leg_length).toBeCloseTo(6, 5);
  });

  it('charges nothing below a default or for free genes', () => {
    const c = designCost(spec({ body_size: 0.1, aggression: 1 }, { litter_size: 1 }), cat);
    expect(c.total).toBe(0);
  });

  it('charges extra founders and offspring', () => {
    const c = designCost(spec({}, { litter_size: 5, founders: 30 }), cat);
    expect(c.litter).toBe(6);
    expect(c.founders).toBeCloseTo(4, 5);
    expect(designCost(spec({}, { litter_size: 1, founders: 10 }), cat).founders).toBe(0);
  });
});

describe('validateDesign', () => {
  it('accepts a design within budget', () => {
    expect(validateDesign(spec({ fur: 0.5, keen_nose: 0.4 }), cat)).toBeNull();
  });

  it('refuses what the server refuses, with the same codes', () => {
    const over = spec({ wings: 1, armor_plates: 1, toxins: 1, muscle: 1, body_size: 1, claws: 1, fangs: 1 }, { founders: 300 });
    expect(validateDesign(over, cat)?.code).toBe('over_budget');
    expect(validateDesign(spec({ teleport: 1 }), cat)?.code).toBe('unknown_gene');
    expect(validateDesign(spec({ fur: 1.5 }), cat)?.code).toBe('value_out_of_range');
    expect(validateDesign(spec({}, { litter_size: 13 }), cat)?.code).toBe('litter_size');
    expect(validateDesign(spec({}, { litter_size: 0 }), cat)?.code).toBe('litter_size');
    expect(validateDesign(spec({}, { founders: 1 }), cat)?.code).toBe('no_founders');
    expect(validateDesign(spec({}, { name: '   ' }), cat)?.code).toBe('bad_name');
  });

  it('allows exactly the budget', () => {
    // wings 1.0 (25) + muscle 1.0 (15) + armor 1.0 (15) + toxins 1.0 (15) + claws 1.0 (10) + fangs 1.0 (10) + fur 1.0 (8) = 98, + litter 1 → 98
    // + 5 extra founders × 0.4 = 2 → exactly 100
    const d = spec({ wings: 1, muscle: 1, armor_plates: 1, toxins: 1, claws: 1, fangs: 1, fur: 1 }, { litter_size: 1, founders: 25 });
    expect(designCost(d, cat).total).toBeCloseTo(100, 5);
    expect(validateDesign(d, cat)).toBeNull();
    expect(validateDesign({ ...d, founders: 26 }, cat)?.code).toBe('over_budget');
  });
});

describe('changedTraits', () => {
  it('keeps only values that differ from the defaults', () => {
    expect(changedTraits({ body_size: 0.3, fur: 0.2, aggression: 0.5, muscle: 0 }, cat)).toEqual({ fur: 0.2 });
  });
});
