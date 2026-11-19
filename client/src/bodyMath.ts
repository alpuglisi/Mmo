// Body formulas shared by the editor's captions (`ui/traitText.ts`) and the
// founder avatar (`world/avatar.ts`): gene values with their defaults, and
// the physical properties mirrored from crates/sim/src/stats.rs `derive`.
// No DOM, no UI: pure functions of a design's traits.

import type { DesignSpec } from './protocol';

/** Anything with a design's trait map. */
export type Traits = Pick<DesignSpec, 'traits'>;

// Defaults as in crates/sim/src/design.rs `default_value` (the editor always
// fills every gene; this is for partial designs). Genes not listed default to 0.
export const GENE_DEFAULTS: Readonly<Record<string, number>> = {
  body_size: 0.3,
  boldness: 0.5,
  aggression: 0.5,
  wanderlust: 0.5,
  group_tolerance: 0.5,
  foraging_range: 0.5,
  pack_instinct: 0.5,
  pickiness: 0.5,
  obedience: 0.5,
  coloration_tolerance: 0.5,
  body_size_tolerance: 0.5,
};

/** The design's value for `gene`, or its default if the design leaves it out. */
export function geneValue(d: Traits, gene: string): number {
  return d.traits[gene] ?? GENE_DEFAULTS[gene] ?? 0;
}

/** The one size scale (inspector, designer, sprites): the server's weight classes, tiny < 1 kg, small < 8 kg, medium < 40 kg, large. */
export const SIZE_WORDS = ['tiny', 'small', 'medium', 'large'] as const;

/** The size word for an adult of `kg` (`SIZE_WORDS`; the server's `appearance.rs` buckets). */
export function sizeWord(kg: number): (typeof SIZE_WORDS)[number] {
  return SIZE_WORDS[kg < 1 ? 0 : kg < 8 ? 1 : kg < 40 ? 2 : 3];
}

/** Adult mass in kg. */
export function adultMass(d: Traits): number {
  const v = (g: string) => geneValue(d, g);
  return 0.2 * 10 ** (2.5 * v('body_size')) * (1 + 0.3 * v('muscle') + 0.3 * v('fat_stores') + 0.4 * v('armor_plates'));
}

/** Can it fly / glide (stats.rs: wings and wing loading)? */
export function flight(d: Traits): { fly: boolean; glide: boolean } {
  const w = geneValue(d, 'wings');
  const loading = adultMass(d) / (w * 6 + 0.01);
  return { fly: w > 0.5 && loading < 2, glide: w > 0.25 && loading < 6 };
}

/** Grip on ice and slopes (stats.rs `grip`). */
export function grip(d: Traits): number {
  return Math.min(1, 0.3 + 0.6 * geneValue(d, 'claws') + 0.2 * geneValue(d, 'climbing'));
}

/** Speed lost on ice (terrain.rs: `0.6 * hoof_slip * (1 - grip)`). */
export function iceSlip(d: Traits): number {
  return 0.6 * geneValue(d, 'hooves') * (1 - grip(d));
}

// --- contextual downsides (stats.rs `Traits::plant_efficiency`, `Traits::insulation`)

/** Fang expression below which there are no fangs at all (stats.rs `FANG_THRESHOLD`). */
export const FANG_THRESHOLD = 0.1;
/** Balance `fang_grazing` (default). */
export const FANG_GRAZING = 0.5;
/** Balance `scale_cover` (default). */
export const SCALE_COVER = 0.5;
/** Insulation per unit of fur on a warm-blooded body (stats.rs `FUR_WARMTH`). */
export const FUR_WARMTH = 0.6;
/** Roughly how many °C of cold tolerance one unit of insulation is worth (0.3 ≈ 9 °C). */
export const DEGREES_PER_INSULATION = 30;

/** Fang size as the sim uses it: 0 below the threshold. */
export function fangSize(d: Traits): number {
  const f = geneValue(d, 'fangs');
  return f < FANG_THRESHOLD ? 0 : f;
}

/**
 * Share of plant food's value long fangs cost: `fang_grazing × fangs ×
 * (1 − carnivory)` (0 for no fangs or a pure meat eater).
 */
export function fangPlantLoss(d: Traits): number {
  const plants = Math.min(1, Math.max(0, 1 - geneValue(d, 'carnivory')));
  return Math.min(1, FANG_GRAZING * fangSize(d) * plants);
}

/**
 * Insulation scales displace: `scale_cover × scales × fur warmth`, only on a
 * warm-blooded body (fur insulates only there).
 */
export function scaleWarmthLoss(d: Pick<DesignSpec, 'traits' | 'thermo'>): number {
  if (d.thermo !== 'warm') return 0;
  return SCALE_COVER * geneValue(d, 'scales') * FUR_WARMTH * geneValue(d, 'fur');
}

/** The insulation stat after the scales' share (stats.rs `derive`, then `Traits::insulation`). */
export function insulation(d: Pick<DesignSpec, 'traits' | 'thermo'>): number {
  const v = (g: string) => geneValue(d, g);
  const fur = d.thermo === 'warm' ? v('fur') : 0;
  const base = Math.min(1, Math.max(0, FUR_WARMTH * fur + 0.35 * v('fat_stores') - 0.15 * v('large_ears')));
  return Math.max(0, base - scaleWarmthLoss(d));
}

/** How well the body floats, 0–1 (stats.rs). */
export function buoyancy(d: Traits): number {
  const v = (g: string) => geneValue(d, g);
  return Math.max(0, Math.min(1, 0.4 + 0.5 * v('fat_stores') - 0.5 * v('armor_plates') - 0.2 * v('muscle')));
}

// --- food upkeep (stats.rs `derive` metabolism, world/apply.rs hunger, organism.rs `food_per_hunger`)

/** Running cost of organs: the share stats.rs adds to the metabolism. */
export function organUpkeep(d: Traits): number {
  const v = (g: string) => geneValue(d, g);
  const fangs = v('fangs') < FANG_THRESHOLD ? 0 : v('fangs');
  return 0.1 * (v('large_eyes') + v('large_ears') + v('keen_nose'))
    + 0.15 * v('muscle')
    + 0.1 * (v('toxins') + v('armor_plates') + v('claws') + fangs)
    + 0.2 * v('wings')
    + 0.05 * v('fur');
}

/** stats.rs `metabolism`: muscle, blood and organ upkeep. */
export function metabolism(d: Pick<DesignSpec, 'traits' | 'thermo'>): number {
  return (0.6 + 0.3 * geneValue(d, 'muscle')) * (d.thermo === 'cold' ? 0.35 : 1) * (1 + organUpkeep(d));
}

/**
 * Food eaten a day, in relative units: hunger rises with the metabolism ×
 * mass^−¼ (`Balance::size_burn`) and a hunger point takes mass^¾ of food,
 * so a day's food goes with metabolism × √mass.
 */
export function dailyFood(d: Pick<DesignSpec, 'traits' | 'thermo'>): number {
  return metabolism(d) * Math.sqrt(adultMass(d));
}

/** How much more food a day `gene` at its value costs than at `base` (0.25 = +25%). */
export function foodCostOf(d: Pick<DesignSpec, 'traits' | 'thermo'>, gene: string, base = 0): number {
  const without = { ...d, traits: { ...d.traits, [gene]: base } };
  return dailyFood(d) / dailyFood(without) - 1;
}
