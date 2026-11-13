// Compatibility of a chosen pair, from the server's `preview_mating` reply.

import type { MatingPreviewView } from '../protocol';
import { label, pct } from './dom';

export interface CompatSummary {
  /** 'ok' fully fertile and inside both preferences, 'warn' reduced fertility or an override cost, 'bad' cannot conceive. */
  level: 'ok' | 'warn' | 'bad';
  text: string;
}

export function compatSummary(p: MatingPreviewView): CompatSummary {
  if (!p.compatible) {
    const genes = p.body_plan_gaps.map((g) => `${label(g.gene)} ${g.gap.toFixed(2)} apart`).join(', ');
    const why = p.body_plan_fertility <= 0
      ? `different body plans${genes ? ` (${genes})` : ''}`
      : 'genomes too different (outbreeding depression)';
    return { level: 'bad', text: `Incompatible: ${why}. They cannot conceive.` };
  }
  const parts = [`fertility ${pct(p.fertility, 0)}`];
  if (p.body_plan_gaps.length) parts.push(`reduced by ${p.body_plan_gaps.map((g) => label(g.gene)).join(', ')}`);
  parts.push(p.cost > 0
    ? `outside their preferences (mismatch ${p.mismatch.toFixed(2)}): cost ${p.cost.toFixed(2)}, failure chance ${pct(p.failure_chance, 0)}`
    : 'within both preferences');
  parts.push(p.distance <= p.meeting_distance ? 'together now' : `${p.distance.toFixed(0)} tiles apart`);
  const level = p.fertility >= 0.9 && p.cost === 0 ? 'ok' : 'warn';
  return { level, text: `Compatible: ${parts.join(' · ')}.` };
}
