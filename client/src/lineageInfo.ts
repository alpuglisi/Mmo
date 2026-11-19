// Small text helpers for the Lineage tab (playtest 1 findings 5 and 23): the
// population cap, and which groups are migrating. Pure, so they are tested.

import type { CrowdingView, GroupView } from './protocol';
import { realTime } from './ui/duration';
import { groupName } from './groups';

/** Thousands separators: 1500 → "1,500". */
export function grouped(n: number): string {
  return Math.round(n).toLocaleString('en-US');
}

export interface CapInfo {
  /** "1,234 / 1,500" (crowding count / soft cap), or just "1,234" without a cap from the server. */
  count: string;
  /** "breeding slowed: litters ×0.6", "hard cap reached: births paused", …, or null. */
  status: string | null;
  level: 'ok' | 'near' | 'full';
  /** The plain-language explanation, for the row's note. */
  explain: string;
}

/** Near the cap from this fraction of it. */
export const CAP_NEAR = 0.9;
/** Litters at or below this multiplier read as slowed. */
export const LITTER_SLOWED = 0.9;
/** Litters at or above this multiplier are "full size" (the server's `FULL_LITTER`). */
export const FULL_LITTER = 0.98;

const times = (x: number) => `×${x.toFixed(x >= 0.95 ? 2 : 1)}`;

/**
 * The cap row. With `crowding` (`lineage_stats.crowding`, the soft cap)
 * breeding slows as the lineage grows: litters shrink toward the soft cap
 * (half size at it), members get hungrier past it, and births stop only at
 * the hard cap of living members. Without it (older servers), `cap` is a
 * plain cap at which births pause.
 */
export function capInfo(population: number, cap: number | undefined, crowding?: CrowdingView | null): CapInfo {
  if (crowding && crowding.hard_cap > crowding.soft_cap) return softCapInfo(population, crowding);
  const hard = crowding?.hard_cap ?? cap;
  if (!hard || hard <= 0) return { count: grouped(population), status: null, level: 'ok', explain: '' };
  const full = population >= hard;
  const near = !full && population >= CAP_NEAR * hard;
  const explain = `Each player may have at most ${grouped(hard)} living organisms on this map, young included. `
    + (full
      ? 'You are at the cap: no young are born until some of yours die (pregnancies wait). A bigger number no longer raises your share; healthier breeding adults and their genes do.'
      : near
        ? 'Close to it, births will pause until some of yours die.'
        : 'At the cap births pause until some of yours die.');
  return {
    count: `${grouped(population)} / ${grouped(hard)}`,
    status: full ? 'population cap reached: births paused' : near ? 'near the population cap' : null,
    level: full ? 'full' : near ? 'near' : 'ok',
    explain,
  };
}

function softCapInfo(population: number, c: CrowdingView): CapInfo {
  const full = population >= c.hard_cap;
  const nearHard = !full && population >= CAP_NEAR * c.hard_cap;
  // Crowded by its share of the world before the soft cap (playtest 9).
  const worldCap = c.world_cap !== undefined && c.world_cap < c.soft_cap ? c.world_cap : null;
  const pastWorld = worldCap !== null && c.count >= worldCap;
  const past = c.count >= c.soft_cap || pastWorld;
  const slowed = c.litter <= LITTER_SLOWED;
  const young = c.juvenile_weight < 1 ? `young count as ${c.juvenile_weight.toFixed(2).replace(/0$/, '')} of an adult` : 'young count in full';
  const hungrier = c.hunger > 1.005 ? `, and they get ${Math.round((c.hunger - 1) * 100)}% hungrier` : '';
  const status = full
    ? 'hard cap reached: births paused'
    : nearHard
      ? `near the hard cap: births stop at ${grouped(c.hard_cap)}`
    : past
      ? `past ${pastWorld && c.count < c.soft_cap ? 'its share of the world' : 'the soft cap'}: litters ${times(c.litter)}${hungrier ? `, hunger ${times(c.hunger)}` : ''}`
      : slowed ? `breeding slowing: litters ${times(c.litter)}` : null;
  const explain = `Breeding slows as your lineage grows. Its crowding count (adults, ${young}) is ${grouped(c.count)}; `
    + `litters shrink toward the soft cap of ${grouped(c.soft_cap)} (half size at it, smaller past it) and past it members compete for food. `
    + `Births stop only at the hard cap of ${grouped(c.hard_cap)} living organisms, young included. `
    + (worldCap !== null
      ? `A lineage's litters also shrink as at the soft cap once its crowding count reaches ${Math.round((c.world_share ?? 0.2) * 100)}% of the whole world's, counted the same way (every lineage and the wild; breeding only, not hunger): at today's numbers from ${grouped(worldCap)}. `
      : '')
    + (full
      ? 'You are at it: no young are born until some of yours die. A bigger number no longer raises your share; healthier breeding adults and their genes do.'
      : past
        ? `Now: litters are ${times(c.litter)} their size${hungrier}.`
        : c.litter < FULL_LITTER
          ? `Now: litters are ${times(c.litter)} their size.`
          : 'Now: litters are full size.');
  return {
    // The limit that binds: the share of the world when it is lower (playtest 10: "653 / 1,500" while 764 bound).
    count: worldCap !== null ? `${grouped(c.count)} / ${grouped(worldCap)} (world share)` : `${grouped(c.count)} / ${grouped(c.soft_cap)}`,
    status,
    level: full ? 'full' : nearHard || past || slowed ? 'near' : 'ok',
    explain,
  };
}

/** The Migration row: every migrating group (the main group's order alone said "none" while others moved). */
export function migrationText(main: [number, number] | null | undefined, groups: readonly GroupView[] | undefined): string {
  const moving = (groups ?? []).filter((g) => g.migration && g.members > 0);
  if (moving.length === 0) return main ? `toward ${main[0].toFixed(0)}, ${main[1].toFixed(0)}` : 'none';
  return moving.map((g) => `${groupName(g.id)} toward ${g.migration![0].toFixed(0)}, ${g.migration![1].toFixed(0)}`).join('; ');
}

/** A need from this level is pressing: "resting" then reads as searching. */
export const NEED_PRESSING = 85;

/**
 * An organism's action in words. A starving or parched animal the sim shows
 * as resting (no food or water found nearby; it waits for its energy to
 * search again) reads as searching, not as a choice to rest (playtest 1).
 */
export function actionText(action: string, needs?: { hunger: number; thirst: number }): string {
  const idle = action === 'rest' || action === 'idle' || action === 'wander';
  if (idle && needs) {
    if (needs.thirst >= NEED_PRESSING && needs.thirst >= needs.hunger) return 'searching for water (parched)';
    if (needs.hunger >= NEED_PRESSING) return 'searching for food (starving)';
  }
  // A night's shallow torpor, not a hibernation (the torpor gene, or one inherited from a cross).
  if (action === 'torpid') return 'torpid (a cold night)';
  return action.replace(/_/g, ' ');
}

/**
 * The newcomer grace in words (playtest 2: it was invisible). `span`
 * renders a tick count as game and real time ("3.0 days (≈ 15 s)").
 */
export function graceText(daysLeft: number | undefined, ticksPerDay: number, span: (ticks: number) => string, realSecondsLeft?: number): string | null {
  if (daysLeft === undefined || !(daysLeft > 0)) return null;
  // The server's real time when it sends it (the grace lasts at least some real minutes at a fast clock).
  const left = realSecondsLeft !== undefined
    ? `${daysLeft < 10 ? daysLeft.toFixed(1) : Math.round(daysLeft)} days (${realTime(realSecondsLeft * 1000)})`
    : span(daysLeft * ticksPerDay);
  return `${left} left: other players can't order attacks on your animals, and their animals leave yours alone even when hunting on their own; wild predators still hunt them. Yours may hunt rivals' unprotected animals, but the first such attack ends your grace.`;
}

/** The founding ramp in words: how much of the share counts toward the ranked season now. */
export function rampText(ramp: { founded_day: number; weight: number; days: number } | undefined): string | null {
  if (!ramp) return null;
  if (ramp.weight >= 1) return `full: founded on day ${ramp.founded_day + 1}, more than ${ramp.days} days ago`;
  // The end day, said as one (playtest 12: "(day 15)" read as the end but was the founding day).
  return `${Math.round(ramp.weight * 100)}% of your share counts toward the ranked season now; it rises to 100% by day ${ramp.founded_day + ramp.days + 1} (${ramp.days} days after founding on day ${ramp.founded_day + 1})`;
}

/** What the two leaderboard columns mean (Lineage and Ranks tabs). */
export const LEGACY_EXPLAIN = 'Share: your alleles among all living breeding adults (genes, not heads; your genes in other lineages count too). '
  + 'Weighted: the same, but each adult counts by how many of its young reached adulthood, so it stays at 0% until your first young grow up. '
  + 'A new lineage counts toward the ranked season gradually (the founding ramp), so dying out and founding again does not pay.';

/**
 * Why the share can fall while the herd grows (playtest 11: it halved during
 * a boom of young): only breeding adults count. Null unless the young
 * outnumber the adults.
 */
export function boomShareText(adults: number, young: number): string | null {
  if (young <= adults || young < 10) return null;
  return `Most of yours are young (${grouped(young)} of ${grouped(adults + young)}): the legacy share counts breeding adults only, `
    + 'so it can fall during a boom while older adults die, and rises as the young grow up.';
}
