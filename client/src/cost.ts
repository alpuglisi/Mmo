// Client-side mirror of the founder point budget (FounderDesign::point_cost
// and validate in crates/sim/src/design.rs). The server re-checks every
// design; this only gives live feedback. The prices come from /api/genes.

import type { DesignSpec, GeneCatalog, MorphSpec } from './protocol';
import { traitName } from './ui/traitNames';

export interface CostBreakdown {
  traits: number;
  litter: number;
  founders: number;
  /** Cold blood, and a body smaller than the default or above the free size. */
  body: number;
  /** Extra structures of the body plan (eyes, leg pairs, ear pairs). */
  morph: number;
  total: number;
  /** Points per gene that costs anything (value above its default). */
  perGene: Record<string, number>;
  /** Points per morphology parameter that costs anything. */
  perMorph: Record<string, number>;
}

/** A count's step: the smallest gap between its allowed values (2 for legs and ears). */
function countStep(values: readonly (number | string)[] | undefined): number {
  const nums = (values ?? []).filter((v): v is number => typeof v === 'number').sort((a, b) => a - b);
  let step = Infinity;
  for (let i = 1; i < nums.length; i++) step = Math.min(step, nums[i] - nums[i - 1]);
  return Number.isFinite(step) && step > 0 ? step : 1;
}

/**
 * Points of a design's `morph` (sim `design::morph_cost`): each count row's
 * price per step above its default (an eye; a pair of legs or ears), nothing
 * below it. Shapes and axes are free unless a row says otherwise.
 */
export function morphCost(m: MorphSpec | undefined, cat: Pick<GeneCatalog, 'morph'>): Record<string, number> {
  const out: Record<string, number> = {};
  if (!m || !Array.isArray(cat.morph)) return out;
  for (const row of cat.morph) {
    if (!(row.cost > 0)) continue;
    const v = (m as Record<string, number | string | undefined>)[row.name];
    if (v === undefined) continue;
    let c = 0;
    if (row.kind === 'count' && typeof v === 'number' && typeof row.default === 'number') {
      c = (Math.max(0, v - row.default) / countStep(row.values)) * row.cost;
    } else if (row.kind === 'axis' && typeof v === 'number' && typeof row.default === 'number') {
      c = Math.max(0, v - row.default) * row.cost;
    } else if (row.kind === 'enum' && v !== row.default) {
      c = row.cost;
    }
    if (c > 0) out[row.name] = c;
  }
  return out;
}

/** Points `founders` founders cost (`design::founder_cost` on the server): free
 * up to `base_founders`, `cost_per_extra_founder` each up to `flat_founders`,
 * then each one `founder_cost_step` dearer than the last. */
export function founderCost(founders: number, cat: GeneCatalog): number {
  const n = Math.floor(founders);
  const extra = Math.max(0, n - cat.base_founders);
  const flat = Math.max(cat.flat_founders ?? Infinity, cat.base_founders);
  const past = Math.max(0, n - flat);
  const step = cat.founder_cost_step ?? 0;
  return extra * cat.cost_per_extra_founder + (past > 0 ? (step * past * (past + 1)) / 2 : 0);
}

export function designCost(d: DesignSpec, cat: GeneCatalog): CostBreakdown {
  const perGene: Record<string, number> = {};
  let traits = 0;
  for (const g of cat.genes) {
    const v = d.traits[g.name] ?? g.default;
    const c = Math.max(0, v - g.default) * g.cost;
    if (c > 0) perGene[g.name] = c;
    traits += c;
  }
  const litter = Math.max(0, Math.floor(d.litter_size) - 1) * cat.cost_per_extra_offspring;
  const founders = founderCost(d.founders, cat);
  const sizeGene = cat.genes.find((g) => g.name === 'body_size');
  const size = sizeGene ? d.traits.body_size ?? sizeGene.default : 0;
  const smaller = sizeGene ? Math.max(0, sizeGene.default - size) : 0;
  const larger = sizeGene ? Math.max(0, size - (cat.free_body_size ?? Infinity)) : 0;
  const body =
    (d.thermo === 'cold' ? cat.cost_cold_blooded ?? 0 : 0) +
    smaller * (cat.cost_per_smaller_body ?? 0) +
    larger * (cat.cost_per_larger_body ?? 0);
  const perMorph = morphCost(d.morph, cat);
  const morph = Object.values(perMorph).reduce((a, b) => a + b, 0);
  return { traits, litter, founders, body, morph, total: traits + litter + founders + body + morph, perGene, perMorph };
}

export interface DesignProblem {
  code: string;
  message: string;
}

/** The same checks as the server, in the same order (first problem wins). */
export function validateDesign(d: DesignSpec, cat: GeneCatalog): DesignProblem | null {
  const name = d.name.trim();
  if (name.length === 0 || [...name].length > 40) {
    return { code: 'bad_name', message: 'a lineage name needs 1 to 40 characters' };
  }
  const known = new Set(cat.genes.map((g) => g.name));
  for (const [k, v] of Object.entries(d.traits)) {
    if (!known.has(k)) return { code: 'unknown_gene', message: `unknown gene "${k}"` };
    if (!(v >= 0 && v <= 1)) return { code: 'value_out_of_range', message: `${traitName(k)} must be in [0, 1]` };
  }
  if (!Number.isInteger(d.litter_size) || d.litter_size < 1 || d.litter_size > cat.max_litter) {
    return { code: 'litter_size', message: `${traitName('litter_size')} ${d.litter_size} not in 1..=${cat.max_litter}` };
  }
  if (!Number.isInteger(d.founders) || d.founders < 2) {
    return { code: 'no_founders', message: 'a design needs at least two founders' };
  }
  const cost = designCost(d, cat).total;
  if (cost > cat.budget + 1e-3) {
    return { code: 'over_budget', message: `design costs ${cost.toFixed(1)} points, budget is ${cat.budget.toFixed(1)}` };
  }
  return null;
}

/** Only the traits that differ from their defaults (what the server stores). */
export function changedTraits(traits: Record<string, number>, cat: GeneCatalog): Record<string, number> {
  const out: Record<string, number> = {};
  for (const g of cat.genes) {
    const v = traits[g.name];
    if (v !== undefined && Math.abs(v - g.default) > 1e-6) out[g.name] = v;
  }
  return out;
}
