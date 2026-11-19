// The design screen's season notice: a lineage founded just before (or in)
// winter has the hardest start, so the screen says so and names what helps.

import type { WorldSummary } from './protocol';

/** Days before winter from which the notice shows (the server's `WINTER_PROVISION_DAYS`). */
export const SEASON_NOTICE_DAYS = 30;

/** What the founders' first winter costs them: nothing (sim `seasonal.rs`: founders sleep their first winter free). */
export const FREE_FIRST_WINTER = 'Founders sleep their first winter free: none of them die of the winter-sleep toll (their young do pay it).';

/**
 * The notice's text, or null when winter is far off (or the server doesn't
 * say). `thermo`: a cold-blooded design is told about brumation, not
 * hibernation (a cold-blooded hibernator can't warm itself awake).
 */
export function seasonNotice(
  w: (Pick<WorldSummary, 'days_to_winter' | 'winter_provision'> & Partial<Pick<WorldSummary, 'clock' | 'days_per_year'>>) | null | undefined,
  thermo: 'warm' | 'cold' = 'warm',
): string | null {
  const days = w?.days_to_winter;
  if (days === undefined || days > SEASON_NOTICE_DAYS) return null;
  // How long the winter still lasts (playtest 11: "It is winter now" read on in spring).
  const year = w?.days_per_year;
  const left = days === 0 && year && w?.clock ? year - (w.clock.day % year) : null;
  const spring = left === null ? '' : left <= 1 ? ' Spring comes within a day.' : ` Spring comes in about ${left} days.`;
  const when = days === 0 ? `It is winter now.${spring}` : `Winter starts in ${days} ${days === 1 ? 'day' : 'days'}.`;
  const help = thermo === 'cold'
    ? 'Brumation and fat stores help a cold-blooded lineage through it (not hibernation: a cold-blooded hibernator can\'t warm itself awake); cold-blooded animals don\'t breed in winter.'
    : 'Fur, fat stores and hibernation help a new lineage through it (the starting design has all three).';
  const provision = w?.winter_provision ? ' Founders arriving now start fed, with full fat reserves.' : '';
  return `${when} ${help}${provision} ${FREE_FIRST_WINTER}`;
}
