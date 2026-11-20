// Which toasts the map shows (playtest 12: on phones the reply to the
// player's own command was pushed out by the alerts it set off).

import type { Toast } from '../model';

/** News (alerts, mating pushes) goes after this long; a reply to the player's own command stays longer. */
export const NEWS_MS = 6000;
export const OWN_MS = 12000;

/** Reading time past the first 80 characters (playtest 13: a long alert went in 6 s, before it could be read). */
const MS_PER_CHAR = 40;
/** How long a toast stays: longer for a long one, at most 15 s for news. */
export function toastLife(t: Toast): number {
  const extra = Math.max(0, t.text.length - 80) * MS_PER_CHAR;
  return t.news ? Math.min(15000, NEWS_MS + extra) : Math.min(20000, OWN_MS + extra);
}

/** Still shown at `now`. */
export function toastAlive(t: Toast, now: number): boolean {
  return now - (t.at ?? now) < toastLife(t);
}

/**
 * The toasts to draw, oldest first, at most `max`: the newest reply to the
 * player's own command always among them (first, above the news), then
 * the newest others.
 */
export function pickToasts(list: readonly Toast[], max: number): Toast[] {
  if (max <= 0) return [];
  // The newest reply, with any other part of it (toasts of the player's own
  // posted within REPLY_MS of it): the whole reply is pinned, not just its
  // last line (playtest 13: a brooding note stayed and the warning went).
  const newest = [...list].reverse().find((t) => !t.news);
  const own = newest ? list.filter((t) => !t.news && (newest.at ?? 0) - (t.at ?? 0) <= REPLY_MS).slice(-max) : [];
  const room = max - own.length;
  const rest = room > 0 ? list.filter((t) => !own.includes(t)).slice(-room) : [];
  return [...own, ...rest];
}

/** Toasts of the player's own posted this close together are one reply. */
export const REPLY_MS = 1500;
