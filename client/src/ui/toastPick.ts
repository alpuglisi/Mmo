// Which toasts the map shows (playtest 12: on phones the reply to the
// player's own command was pushed out by the alerts it set off).

import type { Toast } from '../model';

/** News (alerts, mating pushes) goes after this long; a reply to the player's own command stays longer. */
export const NEWS_MS = 6000;
export const OWN_MS = 12000;

/** Still shown at `now`. */
export function toastAlive(t: Toast, now: number): boolean {
  return now - (t.at ?? now) < (t.news ? NEWS_MS : OWN_MS);
}

/**
 * The toasts to draw, oldest first, at most `max`: the newest reply to the
 * player's own command always among them (first, above the news), then
 * the newest others.
 */
export function pickToasts(list: readonly Toast[], max: number): Toast[] {
  const own = [...list].reverse().find((t) => !t.news);
  const rest = list.filter((t) => t !== own).slice(-(own ? max - 1 : max));
  return own && max > 0 ? [own, ...rest] : rest;
}
