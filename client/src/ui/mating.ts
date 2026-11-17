// Compatibility of a chosen pair, from the server's `preview_mating` reply.

import type { MatingPreviewView } from '../protocol';
import { pct } from './dom';
import { traitNameLower } from './traitNames';

export interface CompatSummary {
  /** 'ok' fully fertile and inside both preferences, 'warn' reduced fertility or an override cost, 'bad' cannot conceive. */
  level: 'ok' | 'warn' | 'bad';
  text: string;
}

/** What the form knows about the pair besides the server's preview. */
export interface PairContext {
  /** Both are the player's own (a mating within the lineage runs at once, but only when they are close). */
  ownPair?: boolean;
  /** The id of a pregnant female in the pair, if the client can see it (own organisms only). */
  pregnant?: number | null;
}

export function compatSummary(p: MatingPreviewView, ctx: PairContext = {}): CompatSummary {
  if (ctx.pregnant) {
    return { level: 'bad', text: `#${ctx.pregnant} is already pregnant: she can't conceive again until she gives birth.` };
  }
  if (!p.compatible) {
    const genes = p.body_plan_gaps.map((g) => `${traitNameLower(g.gene)} ${g.gap.toFixed(2)} apart`).join(', ');
    const why = p.body_plan_fertility <= 0
      ? `different body plans${genes ? ` (${genes})` : ''}`
      : 'genomes too different (outbreeding depression)';
    return { level: 'bad', text: `Incompatible: ${why}. They cannot conceive.` };
  }
  const parts = [`fertility ${pct(p.fertility, 0)}`];
  if (p.body_plan_gaps.length) parts.push(`reduced by ${p.body_plan_gaps.map((g) => traitNameLower(g.gene)).join(', ')}`);
  parts.push(p.cost > 0
    ? `outside their preferences (mismatch ${p.mismatch.toFixed(2)}): cost ${p.cost.toFixed(2)}, failure chance ${pct(p.failure_chance, 0)}`
    : 'within both preferences');
  const close = p.distance <= p.meeting_distance;
  parts.push(close ? 'together now' : `${p.distance.toFixed(0)} tiles apart`);
  let level: CompatSummary['level'] = p.fertility >= 0.9 && p.cost === 0 ? 'ok' : 'warn';
  let after = '';
  if (!close) {
    if (ctx.ownPair) {
      // Within the lineage the mating happens on the spot or not at all.
      level = 'warn';
      after = ` Within your lineage they must be within ${p.meeting_distance} tiles to mate: walk one to the other first.`;
    } else {
      after = ` After the other player agrees, the two must meet (within ${p.meeting_distance} tiles).`;
    }
  }
  return { level, text: `Compatible: ${parts.join(' · ')}.${after}` };
}
