// The design screen's season notice: a lineage founded just before (or in)
// winter has the hardest start, so the screen says so and names what helps.

import type { WorldSummary } from './protocol';

/** Days before winter from which the notice shows (the server's `WINTER_PROVISION_DAYS`). */
export const SEASON_NOTICE_DAYS = 30;

/** The notice's text, or null when winter is far off (or the server doesn't say). */
export function seasonNotice(w: Pick<WorldSummary, 'days_to_winter' | 'winter_provision'> | null | undefined): string | null {
  const days = w?.days_to_winter;
  if (days === undefined || days > SEASON_NOTICE_DAYS) return null;
  const when = days === 0 ? 'It is winter now.' : `Winter starts in ${days} ${days === 1 ? 'day' : 'days'}.`;
  const help = 'Fur, fat stores and hibernation help a new lineage through it (the starting design has all three).';
  const provision = w?.winter_provision ? ' Founders arriving now start fed, with full fat reserves.' : '';
  return `${when} ${help}${provision}`;
}
