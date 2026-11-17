// Small text helpers for the Lineage tab (playtest 1 findings 5 and 23): the
// population cap, and which groups are migrating. Pure, so they are tested.

import type { GroupView } from './protocol';
import { groupName } from './groups';

/** Thousands separators: 1500 → "1,500". */
export function grouped(n: number): string {
  return Math.round(n).toLocaleString('en-US');
}

export interface CapInfo {
  /** "1,500 / 1,500" (or just "1,234" without a cap from the server). */
  count: string;
  /** "population cap reached: births paused", "near the population cap", or null. */
  status: string | null;
  level: 'ok' | 'near' | 'full';
  /** The plain-language explanation, for the row's note. */
  explain: string;
}

/** Near the cap from this fraction of it. */
export const CAP_NEAR = 0.9;

/** The cap row: how many of the cap are used and what that means. */
export function capInfo(population: number, cap: number | undefined): CapInfo {
  if (!cap || cap <= 0) return { count: grouped(population), status: null, level: 'ok', explain: '' };
  const full = population >= cap;
  const near = !full && population >= CAP_NEAR * cap;
  const explain = `Each player may have at most ${grouped(cap)} living organisms on this map, young included. `
    + (full
      ? 'You are at the cap: no young are born until some of yours die (pregnancies wait). A bigger number no longer raises your share; healthier breeding adults and their genes do.'
      : near
        ? 'Close to it, births will pause until some of yours die.'
        : 'At the cap births pause until some of yours die.');
  return {
    count: `${grouped(population)} / ${grouped(cap)}`,
    status: full ? 'population cap reached: births paused' : near ? 'near the population cap' : null,
    level: full ? 'full' : near ? 'near' : 'ok',
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
  return action.replace(/_/g, ' ');
}
