// Hybridisation, explained (playtest 1 findings 1 and 2). Two animals can
// have young together when their body plans are close: the sim compares
// thermoregulation, birth mode, body size and diet (and leg and eye counts)
// and takes the largest difference, the body-plan gap
// (`mating::body_plan_gap`): fully fertile up to 0.12, sterile from 0.30.
// A design inside a wild species' range mixes with it wherever they meet,
// and mixed young carry wild genes, which score for the wild ecosystem;
// a rival player's lineage likewise. The same range decides who a lineage
// can ever breed with through directed matings and the market.
//
// Pure, so the builder warning and the "Whose genes" explanations are tested.

import type { AlleleOrigin, DesignSpec, PlayerId, SpeciesView } from './protocol';
import { geneValue } from './bodyMath';

/** Gaps up to this are fully fertile (sim `BODY_PLAN_FULL`). */
export const GAP_FULL = 0.12;
/** Gaps from this are sterile (sim `BODY_PLAN_STERILE`). */
export const GAP_STERILE = 0.3;
/** Weight of birth mode in the gap for fertility (sim `1 − Balance::egg_hybrid`, 0.2 by default):
 * adjacent birth modes, 0.35 apart, count 0.28, so an egg layer is a little fertile with close live bearers. */
export const BIRTH_WEIGHT = 0.8;
/** Body-plan gap per step of leg pairs or eyes (sim `BODY_PLAN_STEP`). */
export const GAP_PER_STEP = 0.1;

/** A body plan as the interbreeding rule sees it. */
export interface BodyPlan {
  endothermy: number;
  birth_mode: number;
  body_size: number;
  carnivory: number;
  leg_count: number;
  eye_count: number;
}

/** A design's expressed body plan (thermoregulation and birth as the design sets their genes). */
export function designPlan(d: DesignSpec): BodyPlan {
  return {
    endothermy: d.thermo === 'warm' ? 0.9 : 0.1,
    birth_mode: d.birth === 'live' ? 0.85 : d.birth === 'incubated_eggs' ? 0.5 : 0.15,
    body_size: geneValue(d, 'body_size'),
    carnivory: geneValue(d, 'carnivory'),
    leg_count: d.morph?.leg_count ?? 4,
    eye_count: d.morph?.eye_count ?? 2,
  };
}

/** A species' body plan, or null when the server didn't send enough of it. */
export function speciesPlan(s: SpeciesView): BodyPlan | null {
  if (s.endothermy === undefined || s.birth_mode === undefined) return null;
  return {
    endothermy: s.endothermy, birth_mode: s.birth_mode, body_size: s.body_size, carnivory: s.carnivory,
    leg_count: s.leg_count ?? 4, eye_count: s.eye_count ?? 2,
  };
}

export type GapPart = 'thermoregulation' | 'birth' | 'body size' | 'diet' | 'legs' | 'eyes';

/** The body-plan gap and the trait that makes it. */
export function gapBetween(a: BodyPlan, b: BodyPlan): { gap: number; by: GapPart } {
  const parts: [GapPart, number][] = [
    ['thermoregulation', Math.abs(a.endothermy - b.endothermy)],
    ['birth', BIRTH_WEIGHT * Math.abs(a.birth_mode - b.birth_mode)],
    ['body size', Math.abs(a.body_size - b.body_size)],
    ['diet', Math.abs(a.carnivory - b.carnivory)],
    ['legs', (GAP_PER_STEP * Math.abs(a.leg_count - b.leg_count)) / 2],
    ['eyes', GAP_PER_STEP * Math.abs(a.eye_count - b.eye_count)],
  ];
  let best = parts[0];
  for (const p of parts) if (p[1] > best[1]) best = p;
  return { gap: best[1], by: best[0] };
}

/** Fertility from a gap: 1 up to `GAP_FULL`, 0 from `GAP_STERILE`. */
export function fertility(gap: number): number {
  if (gap <= GAP_FULL) return 1;
  if (gap >= GAP_STERILE) return 0;
  return 1 - (gap - GAP_FULL) / (GAP_STERILE - GAP_FULL);
}

export interface Neighbour {
  species: SpeciesView;
  gap: number;
  /** The trait the gap comes from (what to change to move away). */
  by: GapPart;
  fertility: number;
  /** Wild (no player's lineage in it), or the players with lineages in it (not you). */
  wild: boolean;
  players: { player: PlayerId; name: string }[];
  /** How far to move `by` to be sterile with it. */
  toSterile: number;
}

/**
 * Living species a design could have young with (gap under `GAP_STERILE`),
 * nearest first; wild ones are those the wild ecosystem dominates. Your own
 * lineage's species are left out.
 */
export function neighbours(d: DesignSpec, species: readonly SpeciesView[], me: PlayerId): Neighbour[] {
  const plan = designPlan(d);
  const out: Neighbour[] = [];
  for (const s of species) {
    if (s.extinct_day !== undefined || s.adults <= 0) continue;
    const sp = speciesPlan(s);
    if (!sp) continue;
    const players = s.lineages.filter((l) => l.owner !== 0 && l.owner !== me).map((l) => ({ player: l.owner, name: l.name }));
    const mineOnly = s.lineages.length > 0 && s.lineages.every((l) => l.owner === me);
    if (mineOnly) continue;
    const { gap, by } = gapBetween(plan, sp);
    if (gap >= GAP_STERILE) continue;
    // A species holding a player's lineage is never called wild, even where
    // wild alleles dominate it (playtest 2: a player's species named "wild").
    const wild = s.dominant === 0 && players.length === 0;
    out.push({ species: s, gap, by, fertility: fertility(gap), wild, players, toSterile: GAP_STERILE - gap });
  }
  return out.sort((a, b) => a.gap - b.gap || b.species.adults - a.species.adults);
}

const pct = (x: number) => `${Math.round(x * 100)}%`;

/** What to change, in words: "diet (carnivory)", "body size", … */
const BY_WORDS: Record<GapPart, string> = {
  thermoregulation: 'thermoregulation', birth: 'birth mode', 'body size': 'body size', diet: 'diet', legs: 'leg count', eyes: 'eye count',
};

export interface HybridAdvice {
  level: 'ok' | 'warn';
  /** One line for the warning's title. */
  headline: string;
  /** Plain-language points: what happens, what to change. */
  points: string[];
  /** Who this design could ever breed with through directed matings and the market. */
  market: string;
}

/** The builder's interbreeding warning for a design, from the living species. */
export function hybridAdvice(d: DesignSpec, species: readonly SpeciesView[], me: PlayerId): HybridAdvice {
  const near = neighbours(d, species, me);
  const wild = near.filter((n) => n.wild);
  const rivals = near.filter((n) => !n.wild && n.players.length > 0);
  const names = (ns: Neighbour[]) => ns.slice(0, 3).map((n) => `${n.species.name} (${Math.round(n.species.adults)} adults, ${pct(n.fertility)} fertile)`).join(', ');
  const points: string[] = [];
  if (wild.length) {
    const n = wild[0];
    points.push(`Your design sits inside the interbreeding range of ${wild.length === 1 ? 'a wild species' : `${wild.length} wild species`}: ${names(wild)}. Where they meet, your animals mate with them, and the mixed young carry wild genes, which count for the wild ecosystem, not for you: your legacy share can end up far below your head count.`);
    points.push(`To keep your genes your own, move ${BY_WORDS[n.by]} at least ${n.toSterile.toFixed(2)} further from ${n.species.name} (a gap of ${GAP_STERILE.toFixed(2)} in thermoregulation, birth mode, body size or diet makes two kinds sterile).`);
  }
  if (rivals.length) {
    const who = [...new Set(rivals.flatMap((n) => n.players.map((p) => p.name)))].slice(0, 4).join(', ');
    points.push(`Players' lineages in range: ${who}. Chance meetings mix your genes with theirs both ways.`);
  }
  const breedable = [...new Set(near.flatMap((n) => (n.wild ? [] : n.players.map((p) => p.name))))];
  const market = breedable.length
    ? `Directed matings and the market: you could breed with ${breedable.slice(0, 5).join(', ')}${breedable.length > 5 ? ' and others' : ''}. Everyone else is too far in diet, size, birth or thermoregulation (gap ${GAP_STERILE.toFixed(2)} or more): their studs would be sterile with yours.`
    : `Directed matings and the market: no other player's lineage is within breeding range of this design now; any stud you buy would be sterile with yours unless its diet, size, birth mode and thermoregulation are each within ${GAP_STERILE.toFixed(2)} of yours.`;
  if (!points.length) {
    return { level: 'ok', headline: 'No wild or rival species can interbreed with this design', points: ['Your genes stay your own: mates outside your lineage would be sterile.'], market };
  }
  return {
    level: 'warn',
    headline: wild.length ? `Interbreeds with ${wild[0].species.name}${wild.length > 1 ? ` and ${wild.length - 1} more` : ''}` : 'Interbreeds with other players\' lineages',
    points,
    market,
  };
}

/** Plain-language reasons a lineage's gene share differs from its head count ("Whose genes"). */
export function originExplain(origins: readonly AlleleOrigin[], me: PlayerId): { headline: string; points: string[] } {
  const total = origins.reduce((n, o) => n + o.share, 0) || 1;
  const own = origins.filter((o) => o.player === me).reduce((n, o) => n + o.share, 0) / total;
  const wild = origins.filter((o) => o.player === 0).reduce((n, o) => n + o.share, 0) / total;
  const rival = Math.max(0, 1 - own - wild);
  const headline = own >= 0.9995
    ? 'All of your animals\' genes are yours'
    : `Only ${pct(own)} of your animals' genes are yours`;
  const points: string[] = [
    'Your legacy share counts genes (alleles), not heads: each of your animals scores only for the share of its genes that came from your founders or mutated in your lineage.',
  ];
  if (wild >= 0.005) points.push(`${pct(wild)} came from wild animals: your lineage interbreeds with a wild species whose diet, body size, birth mode and thermoregulation are close to yours. Those genes score for the wild ecosystem.`);
  if (rival >= 0.005) points.push(`${pct(rival)} came from other players' lineages (directed matings, market studs or chance meetings). Those genes score for them.`);
  if (own < 0.9995) points.push('A design further from its neighbours (a gap of 0.30 in diet, size, birth or thermoregulation) breeds only with its own kind and keeps its genes.');
  return { headline, points };
}
