// Client-side mirror of the founder point budget (FounderDesign::point_cost
// and validate in crates/sim/src/design.rs). The server re-checks every
// design; this only gives live feedback. The prices come from /api/genes.

import type { DesignSpec, GeneCatalog } from './protocol';

export interface CostBreakdown {
  traits: number;
  litter: number;
  founders: number;
  total: number;
  /** Points per gene that costs anything (value above its default). */
  perGene: Record<string, number>;
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
  const founders = Math.max(0, Math.floor(d.founders) - cat.base_founders) * cat.cost_per_extra_founder;
  return { traits, litter, founders, total: traits + litter + founders, perGene };
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
    if (!(v >= 0 && v <= 1)) return { code: 'value_out_of_range', message: `${k} must be in [0, 1]` };
  }
  if (!Number.isInteger(d.litter_size) || d.litter_size < 1 || d.litter_size > cat.max_litter) {
    return { code: 'litter_size', message: `litter size ${d.litter_size} not in 1..=${cat.max_litter}` };
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
