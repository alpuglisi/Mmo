// Alert preferences: which alert kinds show in the feed and which pop up as
// a toast. The server stores them per player (`/api/alerts/prefs`) and keeps
// recording every alert; the client applies them, so switching a kind back
// on shows its recent alerts again.

import { ALERT_KINDS, type AlertKind, type AlertPrefs } from './protocol';
import type { FeedEntry } from './model';

const known = new Set<string>(ALERT_KINDS);
const order = (a: AlertKind, b: AlertKind) => ALERT_KINDS.indexOf(a) - ALERT_KINDS.indexOf(b);

function clean(kinds: readonly unknown[] | undefined): AlertKind[] {
  return [...new Set((kinds ?? []).filter((k): k is AlertKind => typeof k === 'string' && known.has(k)))].sort(order);
}

/** Known kinds only, in the server's order, without duplicates; nothing hidden is toasted (as the server normalizes). */
export function normalizePrefs(p: Partial<AlertPrefs> | null | undefined): AlertPrefs {
  const hidden = clean(p?.hidden);
  return { hidden, toast: clean(p?.toast).filter((k) => !hidden.includes(k)) };
}

export function shows(p: AlertPrefs, kind: AlertKind): boolean {
  return !p.hidden.includes(kind);
}

export function toasts(p: AlertPrefs, kind: AlertKind): boolean {
  return shows(p, kind) && p.toast.includes(kind);
}

/** Switch one setting of one kind. Hiding a kind stops its toasts; toasting a hidden kind shows it again. */
export function setPref(p: AlertPrefs, kind: AlertKind, field: 'show' | 'toast', on: boolean): AlertPrefs {
  let hidden = p.hidden.filter((k) => k !== kind);
  let toast = p.toast.filter((k) => k !== kind);
  if (field === 'show') {
    if (!on) hidden = [...hidden, kind];
    else if (p.toast.includes(kind)) toast = [...toast, kind];
  } else {
    if (on) toast = [...toast, kind];
    else if (p.hidden.includes(kind)) hidden = [...hidden, kind];
  }
  return normalizePrefs({ hidden, toast });
}

/** Feed entries the preferences let through. Offer events are not alerts and always show. */
export function visibleFeed(feed: FeedEntry[], p: AlertPrefs): FeedEntry[] {
  if (p.hidden.length === 0) return feed;
  return feed.filter((e) => e.kind === 'offer' || shows(p, e.kind));
}
