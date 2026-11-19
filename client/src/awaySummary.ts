// "While you were away": a short summary, on login, of the alerts that came
// in since the player was last seen on this device (playtest 7: none, so a
// returning player had to read the whole feed to learn what happened).

import type { FeedEntry } from './model';

/** How each alert kind is counted in the summary (daily summaries are left out). */
const NOUNS: Record<string, [string, string]> = {
  death: ['death alert', 'death alerts'],
  attack: ['attack alert', 'attack alerts'],
  need: ['warning', 'warnings'],
  world: ['world event', 'world events'],
  achievement: ['achievement', 'achievements'],
  market: ['market alert', 'market alerts'],
  group: ['group report', 'group reports'],
  birth: ['birth alert', 'birth alerts'],
  milestone: ['milestone', 'milestones'],
  offer: ['mating offer', 'mating offers'],
};

/**
 * The summary of `feed` entries after `sinceTick` (the last tick seen), or
 * null when nothing worth saying came in (or nothing was seen before).
 */
export function awaySummary(feed: readonly FeedEntry[], sinceTick: number): string | null {
  if (!(sinceTick > 0)) return null;
  const counts = new Map<string, number>();
  for (const e of feed) {
    if (e.tick <= sinceTick || !(e.kind in NOUNS)) continue;
    counts.set(e.kind, (counts.get(e.kind) ?? 0) + 1);
  }
  const total = [...counts.values()].reduce((a, b) => a + b, 0);
  if (total === 0) return null;
  const parts = [...counts.entries()]
    .sort((a, b) => b[1] - a[1] || a[0].localeCompare(b[0]))
    .map(([k, n]) => `${n} ${NOUNS[k][n === 1 ? 0 : 1]}`);
  return `While you were away: ${parts.join(', ')}. The Genes tab's feed lists them.`;
}

/** The localStorage key for the last tick seen by `player` in `world`. */
export function seenKey(world: string, player: number): string {
  return `evo.seen.${world || 'default'}.${player}`;
}
