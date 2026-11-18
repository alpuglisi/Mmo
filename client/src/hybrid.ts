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
/** Weight of an eye-count step in the gap for fertility (sim `1 − Balance::eye_hybrid`, 0.6 by default):
 * 2 → 5 eyes counts 0.12 (fertile), not a sterile 0.30 (playtest 3). Leg steps count fully. */
export const EYE_WEIGHT = 0.4;
/** Weight of warm/cold blood in the gap for fertility (sim `1 − Balance::thermo_hybrid`, 0.7 by default):
 * warm × cold, 0.8 apart, counts 0.24, about a third as fertile, not sterile (playtest 5). */
export const THERMO_WEIGHT = 0.3;

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
    ['thermoregulation', THERMO_WEIGHT * Math.abs(a.endothermy - b.endothermy)],
    ['birth', BIRTH_WEIGHT * Math.abs(a.birth_mode - b.birth_mode)],
    ['body size', Math.abs(a.body_size - b.body_size)],
    ['diet', Math.abs(a.carnivory - b.carnivory)],
    ['legs', (GAP_PER_STEP * Math.abs(a.leg_count - b.leg_count)) / 2],
    ['eyes', EYE_WEIGHT * GAP_PER_STEP * Math.abs(a.eye_count - b.eye_count)],
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

/**
 * Individuals vary around their species' averages (and drift after
 * founding): a species this much past the sterile gap on average can still
 * have some fertile pairs. The builder lists such species as "possible".
 */
export const GAP_MARGIN = 0.08;

export interface Neighbour {
  species: SpeciesView;
  gap: number;
  /** The trait the gap comes from (what to change to move away). */
  by: GapPart;
  /** Fertility at the averages (0 for a "possible" neighbour). */
  fertility: number;
  /** In range only through variation: sterile at the averages, within `GAP_MARGIN` of fertile. */
  possible: boolean;
  /** Only the wild ecosystem's lineages are in it (a species with any player's lineage is not wild). */
  wild: boolean;
  /** Its lineages' share of wild genes counts for the wild ecosystem (`dominant_share` when the wild dominates). */
  wildShare: number;
  /** The players with lineages in it (not you). */
  players: { player: PlayerId; name: string }[];
  /** How far to move `by` to be sterile with it, variation allowed for. */
  toSterile: number;
}

/**
 * Living species a design could have young with (gap under `GAP_STERILE`,
 * or within `GAP_MARGIN` past it through individual variation), nearest
 * first. Your own lineage's species are left out.
 */
export function neighbours(d: DesignSpec, species: readonly SpeciesView[], me: PlayerId): Neighbour[] {
  const plan = designPlan(d);
  const out: Neighbour[] = [];
  for (const s of species) {
    if (s.extinct_day !== undefined || s.adults <= 0) continue;
    const sp = speciesPlan(s);
    if (!sp) continue;
    const players = s.lineages.filter((l) => l.owner !== 0 && l.owner !== me)
      .map((l) => ({ player: l.owner, name: l.owner_name || (l.owner === s.dominant ? s.dominant_name : l.name) }));
    const mineOnly = s.lineages.length > 0 && s.lineages.every((l) => l.owner === me);
    if (mineOnly) continue;
    const { gap, by } = gapBetween(plan, sp);
    if (gap >= GAP_STERILE + GAP_MARGIN) continue;
    const wild = s.lineages.every((l) => l.owner === 0) && (s.lineages.length > 0 || s.dominant === 0);
    out.push({
      species: s, gap, by, fertility: fertility(gap), possible: gap >= GAP_STERILE, wild,
      wildShare: s.dominant === 0 ? s.dominant_share : 0, players, toSterile: Math.max(0, GAP_STERILE + GAP_MARGIN - gap),
    });
  }
  return out.sort((a, b) => a.gap - b.gap || b.species.adults - a.species.adults);
}

const pct = (x: number) => `${Math.round(x * 100)}%`;

/** The gap that makes two kinds sterile, variation allowed for. */
const SAFE_GAP = GAP_STERILE + GAP_MARGIN;

/**
 * Changes that alone would make the design sterile with `n` (the gap is the
 * largest single difference, so one is enough), each one the editor can make:
 * diet or body size within 0..1, or another birth type. Thermoregulation,
 * legs and eyes never reach the sterile gap alone, so they are not offered
 * (playtest 6: "move birth mode 0.09 further", "move thermoregulation further"
 * at the extreme).
 */
export function sterileOptions(d: DesignSpec, s: SpeciesView): string[] {
  const sp = speciesPlan(s);
  if (!sp) return [];
  const out: string[] = [];
  const axis = (label: string, gene: string, theirs: number) => {
    const up = theirs + SAFE_GAP;
    const down = theirs - SAFE_GAP;
    const mine = geneValue(d, gene);
    if (Math.abs(mine - theirs) >= SAFE_GAP) return;
    const opts: string[] = [];
    if (up <= 1) opts.push(`raise ${label} to ${up.toFixed(2)} or more`);
    if (down >= 0) opts.push(`lower ${label} to ${down.toFixed(2)} or less`);
    // The nearer direction first.
    if (opts.length === 2 && mine < theirs) opts.reverse();
    if (opts.length) out.push(opts.join(' or '));
  };
  axis('diet (carnivory)', 'carnivory', sp.carnivory);
  axis('body size', 'body_size', sp.body_size);
  const births: [DesignSpec['birth'], string, number][] = [['live', 'live birth', 0.85], ['incubated_eggs', 'incubated eggs', 0.5], ['abandoned_eggs', 'abandoned eggs', 0.15]];
  for (const [b, label, mode] of births) {
    if (b !== d.birth && BIRTH_WEIGHT * Math.abs(mode - sp.birth_mode) >= SAFE_GAP) out.push(`choose ${label}`);
  }
  return out;
}

/** "To keep your genes your own, one of these is enough: …" for the nearest wild neighbour. */
function moveAdvice(d: DesignSpec, n: Neighbour): string {
  const opts = sterileOptions(d, n.species);
  const why = `(a gap of ${GAP_STERILE.toFixed(2)} in diet, body size or birth mode makes two kinds sterile; ${GAP_MARGIN.toFixed(2)} more allows for variation)`;
  return opts.length
    ? `To keep your genes your own from ${n.species.name}, one of these is enough: ${opts.join('; ')} ${why}.`
    : `No single change of diet, body size or birth type moves this design out of ${n.species.name}'s range; change several (thermoregulation, legs and eyes help only a little) or found far from where it lives.`;
}


export interface HybridAdvice {
  level: 'ok' | 'warn';
  /** One line for the warning's title. */
  headline: string;
  /** Plain-language points: what happens, what to change. */
  points: string[];
  /** Who this design could ever breed with through directed matings and the market. */
  market: string;
  /** How sure this is (it compares averages; animals vary). */
  confidence: string;
}

/** How sure the builder's check is: it compares the design with species averages. */
export const HYBRID_CONFIDENCE = `Compared with each species' averages: animals vary (about ±${GAP_MARGIN.toFixed(2)}) and drift after founding, so treat this as a guide. `
  + 'Once founded, the Market tab\'s "Who your lineage can breed with" compares real animals.';

/** A neighbour as the card names it: "Steppe antelope (120 adults, 100% fertile)", "GeneBroker's Broker voles (…)". */
function neighbourName(n: Neighbour, measured?: ReadonlyMap<number, number>): string {
  const owner = n.wild ? '' : n.players.length ? `${[...new Set(n.players.map((p) => p.name))].slice(0, 2).join(' and ')}'s ` : '';
  // The server's sampled figure for the species' lineages wins over the estimate (one figure per species).
  const m = measuredFertility(n.species, measured);
  const odds = m !== null ? `${pct(m)} fertile, measured`
    : n.possible ? 'sterile on average, some pairs fertile' : `${pct(n.fertility)} fertile`;
  return `${owner}${n.species.name} (${Math.round(n.species.adults)} adults, ${odds})`;
}

/** The best measured fertility among a species' lineages, or null if none was measured. */
export function measuredFertility(s: SpeciesView, measured?: ReadonlyMap<number, number>): number | null {
  if (!measured) return null;
  const fs = s.lineages.map((l) => measured.get(l.lineage)).filter((f): f is number => f !== undefined);
  return fs.length ? Math.max(...fs) : null;
}

/** The builder's interbreeding warning for a design, from the living species. */
/** `measured`: the server's sampled fertility by lineage (`POST /api/design/compatibility`), when it has answered. */
export function hybridAdvice(d: DesignSpec, species: readonly SpeciesView[], me: PlayerId, measured?: ReadonlyMap<number, number>): HybridAdvice {
  const near = neighbours(d, species, me);
  const sure = near.filter((n) => !n.possible);
  const wild = sure.filter((n) => n.wild);
  const rivals = sure.filter((n) => !n.wild);
  const maybe = near.filter((n) => n.possible);
  // Named up to three, then "and N more" (the header said "4 wild species" over a list of 3).
  const list = (ns: Neighbour[]) => ns.slice(0, 3).map((n) => neighbourName(n, measured)).join(', ') + (ns.length > 3 ? ` and ${ns.length - 3} more` : '');
  const points: string[] = [];
  if (wild.length) {
    const n = wild[0];
    points.push(`Your design sits inside the interbreeding range of ${wild.length === 1 ? 'a wild species' : `${wild.length} wild species`}: ${list(wild)}. Where they meet, your animals mate with them, and the mixed young carry wild genes, which count for the wild ecosystem, not for you: your legacy share can end up far below your head count.`);
    points.push(moveAdvice(d, n));
  }
  if (rivals.length) {
    const mixed = rivals.filter((n) => n.wildShare > 0.5);
    points.push(`Players' species in range: ${list(rivals)}. Chance meetings mix your genes with theirs both ways.`
      + (mixed.length ? ` ${mixed[0].species.name} is mostly wild genes by now (${pct(mixed[0].wildShare)}), so mixing with it also brings in wild genes.` : ''));
  }
  if (maybe.length) {
    points.push(`Just out of range on average, but close enough that some pairs may still be fertile: ${list(maybe)}.`);
  }
  const breedable = [...new Set(near.flatMap((n) => n.players.map((p) => p.name)))];
  const market = breedable.length
    ? `Directed matings and the market: you could breed with ${breedable.slice(0, 5).join(', ')}${breedable.length > 5 ? ' and others' : ''}${maybe.some((n) => n.players.length) ? ' (some only with their closest animals)' : ''}. Everyone else is too far in diet, size, birth or thermoregulation (gap ${GAP_STERILE.toFixed(2)} or more): their studs would be sterile with yours.`
    : `Directed matings and the market: no other player's species is within breeding range of this design's averages; a stud would most likely be sterile with yours unless its diet, size, birth mode and thermoregulation are each within ${GAP_STERILE.toFixed(2)} of yours.`;
  if (!points.length) {
    return {
      level: 'ok', headline: 'No wild or rival species is in breeding range of this design',
      points: ['Your genes should stay your own: on average, mates outside your lineage would be sterile.'], market, confidence: HYBRID_CONFIDENCE,
    };
  }
  const headline = wild.length
    ? `Interbreeds with ${wild[0].species.name}${wild.length + rivals.length > 1 ? ` and ${wild.length + rivals.length - 1} more` : ''}`
    : rivals.length ? `Interbreeds with ${neighbourName(rivals[0]).replace(/ \(.*$/, '')}${rivals.length > 1 ? ` and ${rivals.length - 1} more` : ''}`
    : 'May interbreed with close species';
  return { level: wild.length || rivals.length ? 'warn' : 'ok', headline, points, market, confidence: HYBRID_CONFIDENCE };
}

/** Plain-language reasons a lineage's gene share differs from its head count ("Whose genes"). */
export function originExplain(origins: readonly AlleleOrigin[], me: PlayerId): { headline: string; points: string[] } {
  const total = origins.reduce((n, o) => n + o.share, 0) || 1;
  const own = origins.filter((o) => o.player === me).reduce((n, o) => n + o.share, 0) / total;
  const wild = origins.filter((o) => o.player === 0).reduce((n, o) => n + o.share, 0) / total;
  const rival = Math.max(0, 1 - own - wild);
  // One decimal, as the share beside it: "Only 100%" next to 99.7% read as a contradiction.
  const one = (x: number) => `${(Math.floor(x * 1000) / 10).toFixed(1)}%`;
  const headline = own >= 0.9995
    ? 'All of your animals\' genes are yours'
    : own >= 0.99
      ? `Nearly all (${one(own)}) of your animals' genes are yours`
      : `Only ${pct(own)} of your animals' genes are yours`;
  const points: string[] = [
    'Your legacy share counts genes (alleles), not heads: each of your animals scores only for the share of its genes that came from your founders or mutated in your lineage.',
  ];
  if (wild >= 0.005) points.push(`${pct(wild)} came from wild animals: your lineage interbreeds with a wild species whose diet, body size, birth mode and thermoregulation are close to yours. Those genes score for the wild ecosystem.`);
  if (rival >= 0.005) points.push(`${pct(rival)} came from other players' lineages (directed matings, market studs or chance meetings). Those genes score for them.`);
  if (own < 0.9995) points.push('A design further from its neighbours (a gap of 0.30 in diet, size, birth or thermoregulation) breeds only with its own kind and keeps its genes.');
  return { headline, points };
}
