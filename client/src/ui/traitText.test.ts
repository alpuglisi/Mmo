import { describe, expect, it } from 'vitest';
import type { DesignSpec } from '../protocol';
import { adultMass, caption, flight, iceSlip, levelOf, TRAIT_TEXT } from './traitText';

// Every slider the design editor shows: `GET /api/genes` (crates/server
// src/design.rs `catalog`: every gene but endothermy, birth mode and litter
// size, named by `gene_key`).
const EDITOR_GENES = [
  'body_size', 'muscle', 'fat_stores', 'leg_length', 'claws', 'hooves', 'foot_width', 'tail', 'swimming', 'wings',
  'climbing', 'burrowing', 'fur', 'scales', 'armor_plates', 'fangs', 'large_eyes', 'large_ears', 'keen_nose',
  'carnivory', 'camouflage', 'toxins', 'coloration', 'vocal_ability', 'boldness', 'aggression', 'wanderlust',
  'group_tolerance', 'foraging_range', 'pack_instinct', 'pickiness', 'preferred_coloration', 'coloration_tolerance',
  'preferred_body_size', 'body_size_tolerance', 'hibernation', 'brumation', 'migration', 'estivation', 'food_caching',
  'torpor', 'vigilance', 'seasonal_coat', 'timed_breeding', 'obedience',
];

function design(traits: Record<string, number> = {}, extra: Partial<DesignSpec> = {}): DesignSpec {
  return { name: 'T', thermo: 'warm', birth: 'live', litter_size: 2, traits, founders: 20, ...extra };
}

const words = (s: string) => s.split(/\s+/).filter(Boolean).length;

describe('trait text', () => {
  it('describes every slider gene the editor shows, and nothing else', () => {
    expect(Object.keys(TRAIT_TEXT).sort()).toEqual([...EDITOR_GENES].sort());
    for (const g of EDITOR_GENES) {
      const t = TRAIT_TEXT[g];
      for (const k of ['what', 'low', 'mid', 'high'] as const) {
        expect(t[k].trim(), `${g}.${k}`).not.toBe('');
        expect(words(t[k]), `${g}.${k} is short`).toBeLessThanOrEqual(16);
      }
      expect(new Set([t.low, t.mid, t.high]).size, `${g}: three different levels`).toBe(3);
      expect(t.lo ?? 0.33, g).toBeLessThanOrEqual(t.hi ?? 0.66);
    }
  });

  it('gives every gene a caption at every value', () => {
    for (const g of EDITOR_GENES) {
      for (const v of [0, 0.2, 0.5, 0.8, 1]) {
        const c = caption(g, design({ [g]: v }));
        expect(c?.text.trim(), `${g}=${v}`).toBeTruthy();
      }
    }
    expect(caption('no_such_gene', design())).toBeNull();
  });

  it('switches level at the default thresholds', () => {
    const t = TRAIT_TEXT.muscle;
    expect(levelOf(t, 0.32)).toBe('low');
    expect(levelOf(t, 0.33)).toBe('mid');
    expect(levelOf(t, 0.66)).toBe('mid');
    expect(levelOf(t, 0.67)).toBe('high');
    expect(caption('muscle', design({ muscle: 0 }))!.text).toBe(t.low);
    expect(caption('muscle', design({ muscle: 0.5 }))!.text).toBe(t.mid);
    expect(caption('muscle', design({ muscle: 0.9 }))!.text).toBe(t.high);
  });

  it('switches at a trait’s own thresholds', () => {
    // Fangs: below a small threshold there are no fangs at all.
    expect(caption('fangs', design({ fangs: 0.05 }))!.text).toBe('no fangs');
    expect(caption('fangs', design({ fangs: 0.3 }))!.text).toBe('fangs: stronger bite');
    // Seasonal strategies switch on at 0.5 (stats.rs STRATEGY_THRESHOLD).
    expect(caption('torpor', design({ torpor: 0.49 }))!.level).toBe('low');
    expect(caption('torpor', design({ torpor: 0.5 }))!.level).toBe('mid');
    expect(caption('torpor', design({ torpor: 0.9 }))!.level).toBe('high');
    // Climbing reaches the canopy and cliffs at 0.4 (terrain.rs CLIMB_CANOPY).
    expect(caption('climbing', design({ climbing: 0.39 }))!.level).toBe('low');
    expect(caption('climbing', design({ climbing: 0.4 }))!.level).toBe('mid');
  });

  it('wings: glide above 0.25 and fly above 0.5, if the body is light enough', () => {
    expect(caption('wings', design({ wings: 0.25 }))!.text).toBe('no wings: ground-bound');
    expect(caption('wings', design({ wings: 0.4 }))!.text).toMatch(/^gliding flaps/);
    expect(caption('wings', design({ wings: 0.8 }))!.text).toBe(TRAIT_TEXT.wings.high);
    expect(flight(design({ wings: 0.8 }))).toEqual({ fly: true, glide: true });
    // A deer-sized body can't get off the ground on the same wings.
    const heavy = design({ wings: 0.8, body_size: 1 });
    expect(flight(heavy).fly).toBe(false);
    expect(caption('wings', heavy)!.text).toMatch(/^too heavy to fly/);
    expect(adultMass(design())).toBeCloseTo(0.2 * 10 ** 0.75, 5);
  });

  it('fur reads differently on a cold-blooded body: it blocks basking', () => {
    expect(caption('fur', design({ fur: 0.9 }))!.text).toBe(TRAIT_TEXT.fur.high);
    expect(caption('fur', design({ fur: 0.9 }, { thermo: 'cold' }))!.text).toMatch(/blocks basking/);
    expect(caption('fur', design({ fur: 0.5 }, { thermo: 'cold' }))!.text).toMatch(/blocks basking/);
    expect(caption('fur', design({ fur: 0 }, { thermo: 'cold' }))!.text).toBe('bare skin: basks freely');
  });

  it('hooves slip on ice unless claws give grip', () => {
    expect(caption('hooves', design({ hooves: 0.9 }))!.text).toMatch(/slips on ice/);
    const clawed = design({ hooves: 0.9, claws: 1 });
    expect(iceSlip(clawed)).toBeLessThan(0.1);
    expect(caption('hooves', clawed)!.text).toMatch(/claws offset the ice slip/);
    expect(caption('hooves', design({ hooves: 0.5, claws: 0.8 }))!.text).toMatch(/claws offset the ice slip/);
  });

  it('dormancy captions follow thermoregulation', () => {
    expect(caption('hibernation', design({ hibernation: 0.8 }, { thermo: 'cold' }))!.text).toMatch(/mismatch/);
    expect(caption('brumation', design({ brumation: 0.8 }))!.text).toMatch(/burns fat/);
    expect(caption('brumation', design({ brumation: 0.8, hibernation: 0.8 }))!.text).toMatch(/hibernates instead/);
    expect(caption('hibernation', design({ brumation: 0.8, hibernation: 0.8 }, { thermo: 'cold' }))!.text).toMatch(/brumates instead/);
    expect(caption('seasonal_coat', design({ seasonal_coat: 0.7 }, { thermo: 'cold' }))!.text).toMatch(/blocks basking/);
  });

  it('group-trait captions name the missing partner trait', () => {
    expect(caption('coloration', design({ coloration: 0.8, toxins: 0.8 }))!.text).toMatch(/warning colours/);
    expect(caption('coloration', design({ coloration: 0.8 }))!.text).toMatch(/not toxic enough/);
    expect(caption('pack_instinct', design({ pack_instinct: 0.9, carnivory: 0.2 }))!.text).toMatch(/only meat eaters/);
    expect(caption('pack_instinct', design({ pack_instinct: 0.9, carnivory: 0.9 }))!.text).toBe(TRAIT_TEXT.pack_instinct.high);
    expect(caption('vocal_ability', design({ vocal_ability: 0.9 }))!.text).toMatch(/need large ears/);
  });
});
