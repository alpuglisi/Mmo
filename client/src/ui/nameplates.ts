// Bought nameplates (`set_nameplate`, a cosmetic): a colour from the shop's
// palette and a short title, shown next to the player's name in the Ranks
// tab, the hall of fame, the leaderboard and a rival animal's inspector.
// One store for the page, filled from the `nameplates` command. Titles are
// player text: always set as text, never as markup.

import type { PlayerId } from '../protocol';
import { h } from './dom';

export interface Nameplate {
  color: string;
  title: string;
}

const COLORS = new Set(['amber', 'crimson', 'emerald', 'azure', 'violet', 'ivory', 'jet', 'copper']);

let plates = new Map<PlayerId, Nameplate>();

/** The `nameplates` command's data; null if malformed. */
export function parseNameplates(data: unknown): Map<PlayerId, Nameplate> | null {
  const raw = (data as { nameplates?: unknown } | null)?.nameplates;
  if (!raw || typeof raw !== 'object') return null;
  const out = new Map<PlayerId, Nameplate>();
  for (const [k, v] of Object.entries(raw as Record<string, unknown>)) {
    const p = Number(k);
    const n = v as { color?: unknown; title?: unknown } | null;
    if (!Number.isInteger(p) || !n || typeof n.color !== 'string' || typeof n.title !== 'string' || n.title.trim() === '') continue;
    out.set(p, { color: n.color, title: n.title });
  }
  return out;
}

export function setNameplates(next: Map<PlayerId, Nameplate>): void {
  plates = next;
}

export function nameplateOf(p: PlayerId): Nameplate | undefined {
  return plates.get(p);
}

/** A stable key of the store (for "redraw if it changed"). */
export function nameplatesKey(): string {
  return JSON.stringify([...plates].sort((a, b) => a[0] - b[0]));
}

/** The badge for `p`'s nameplate, or null without one. */
export function plateBadge(p: PlayerId): HTMLElement | null {
  const n = plates.get(p);
  if (!n) return null;
  const color = COLORS.has(n.color) ? n.color : 'ivory';
  const el = h('span', { class: `nameplate plate-${color}`, 'data-player': p, title: `Nameplate: ${n.title}` });
  el.textContent = n.title;
  return el;
}

/** "ann «Herd queen»" for plain-text places (lists joined with " · "). */
export function nameWithPlate(name: string, p: PlayerId): string {
  const n = plates.get(p);
  return n ? `${name} «${n.title}»` : name;
}
