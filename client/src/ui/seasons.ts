// The Seasons tab: the ranked season under way (clock, standings so far,
// your places), the hall of fame (finished seasons, newest first, with
// their winners; each opens onto its full standings) and your own record
// (from `/api/seasons/current`, `/api/seasons`, `/api/seasons/:n` and
// `/api/seasons/player/:player`). The data preparation is pure and
// unit-tested; `renderSeasons` only builds DOM.

import type {
  CurrentSeason, PlayerId, PlayerPlacement, PlayerSeason, RankedSeasonHead, RankedSeasonList, RankedSeasonPage, SeasonCategory,
  StandingPage,
} from '../protocol';
import { cssColor, ownerColor, OWN_COLOR } from '../world/colors';
import { h, pct } from './dom';

export const CATEGORIES: SeasonCategory[] = ['legacy', 'lineage', 'species', 'market', 'survival'];

const LABELS: Record<SeasonCategory, string> = {
  legacy: 'Legacy',
  lineage: 'Largest lineage',
  species: 'Species founded',
  market: 'Stud market',
  survival: 'Survival',
};

const HINTS: Record<SeasonCategory, string> = {
  legacy: 'Mean share of breeding alleles over the season (a missing day counts as 0)',
  lineage: 'Most living members on a recorded day',
  species: 'Speciation events credited to your alleles',
  market: 'Renown earned from stud fees',
  survival: 'Catastrophes your lineage lived through',
};

export function categoryLabel(c: SeasonCategory | string): string {
  return LABELS[c as SeasonCategory] ?? c.replace(/_/g, ' ');
}

/** A category's value as shown: a share, a count with its unit. */
export function formatValue(c: SeasonCategory | string, v: number): string {
  const n = Math.round(v);
  switch (c) {
    case 'legacy': return pct(v);
    case 'lineage': return `${n} organism${n === 1 ? '' : 's'}`;
    case 'species': return `${n} species`;
    case 'market': return `${n} renown`;
    case 'survival': return `${n} catastrophe${n === 1 ? '' : 's'}`;
    default: return String(v);
  }
}

/** 1 → "1st", 12 → "12th", 22 → "22nd". */
export function ordinal(n: number): string {
  const tens = n % 100;
  const suffix = tens >= 11 && tens <= 13 ? 'th' : (['th', 'st', 'nd', 'rd'][n % 10] ?? 'th');
  return `${n}${suffix}`;
}

/** "2nd of 5 · 12.3%". */
export function placementText(p: PlayerPlacement): string {
  return `${ordinal(p.rank)} of ${p.of} · ${formatValue(p.category, p.value)}`;
}

/** "Season 3 · days 121–180 · 12 days left" (days 1-based, as everywhere in the client). */
export function seasonClock(c: CurrentSeason): string {
  const left = c.days_left;
  return `Season ${c.season} · days ${c.first_day + 1}–${c.last_day + 1} · ${left === 0 ? 'ends with the next daily record' : `${left} day${left === 1 ? '' : 's'} left`}`;
}

/** Where `me` stands in each category of a standings page (null: not ranked). */
export function myPlaces(standings: StandingPage[], me: PlayerId): { category: SeasonCategory; place: PlayerPlacement | null }[] {
  return standings.map((s) => {
    const row = s.rows.find((r) => r.player === me);
    return { category: s.category, place: row ? { category: s.category, rank: row.rank, value: row.value, of: s.total } : null };
  });
}

/** "Legacy: ann (12.3%) · Largest lineage: bob (412 organisms)"; tied winners joined with "&". */
export function winnersLine(head: RankedSeasonHead): string {
  const parts: string[] = [];
  for (const c of CATEGORIES) {
    const w = head.winners.filter((x) => x.category === c);
    if (w.length === 0) continue;
    parts.push(`${categoryLabel(c)}: ${w.map((x) => x.name).join(' & ')} (${formatValue(c, w[0].value)})`);
  }
  return parts.join(' · ');
}

export interface FameRow {
  player: PlayerId;
  name: string;
  /** Legacy wins (the season's champion). */
  crowns: number;
  /** Wins in any category, legacy included. */
  titles: number;
}

/** Players by seasons won in `seasons`: legacy crowns first, then all titles, then name. */
export function hallOfFame(seasons: RankedSeasonHead[]): FameRow[] {
  const rows = new Map<PlayerId, FameRow>();
  for (const s of seasons) {
    for (const w of s.winners) {
      const r = rows.get(w.player) ?? { player: w.player, name: w.name, crowns: 0, titles: 0 };
      r.titles += 1;
      if (w.category === 'legacy') r.crowns += 1;
      rows.set(w.player, r);
    }
  }
  return [...rows.values()].sort((a, b) => b.crowns - a.crowns || b.titles - a.titles || a.name.localeCompare(b.name));
}

export interface MyRecord {
  /** Finished seasons the player was ranked in. */
  seasons: number;
  crowns: number;
  titles: number;
  /** Best rank per category ever, and the season it came in (the earliest such). */
  best: { category: SeasonCategory; rank: number; season: number }[];
}

export function myRecord(history: PlayerSeason[]): MyRecord {
  let crowns = 0;
  let titles = 0;
  const best = new Map<SeasonCategory, { rank: number; season: number }>();
  // History is newest first: walk oldest first so a tie keeps the earliest.
  for (const s of [...history].reverse()) {
    for (const p of s.placements) {
      if (p.rank === 1) {
        titles += 1;
        if (p.category === 'legacy') crowns += 1;
      }
      const b = best.get(p.category);
      if (!b || p.rank < b.rank) best.set(p.category, { rank: p.rank, season: s.season });
    }
  }
  return {
    seasons: history.length,
    crowns,
    titles,
    best: CATEGORIES.filter((c) => best.has(c)).map((c) => ({ category: c, ...best.get(c)! })),
  };
}

/** The key that identifies a finished season (seasons of different worlds share numbers). */
export function seasonKey(s: { world_id: string; season: number }): string {
  return `${s.world_id}/${s.season}`;
}

export interface SeasonsData {
  current: CurrentSeason | null;
  list: RankedSeasonList | null;
  mine: PlayerSeason[] | null;
  /** Opened finished seasons' standings, by `seasonKey`. */
  details: Map<string, RankedSeasonPage>;
}

export interface SeasonsHandlers {
  /** A finished season was opened: fetch its standings. */
  open: (head: RankedSeasonHead) => void;
  /** Show more finished seasons. */
  more: () => void;
}

const swatch = (p: PlayerId, me: PlayerId) => h('span', { class: 'swatch', style: `background:${cssColor(p === me ? OWN_COLOR : ownerColor(p))}` });

function standingTables(standings: StandingPage[], me: PlayerId, idPrefix: string): HTMLElement {
  return h('div', { class: 'standings' }, ...standings.map((s) => h('div', { class: 'standing', 'data-category': s.category },
    h('h4', { title: HINTS[s.category] }, `${categoryLabel(s.category)}${s.total > 0 ? ` · ${s.total} ranked` : ''}`),
    s.rows.length === 0
      ? h('p', { class: 'muted small' }, 'Nobody yet.')
      : h('table', { class: 'board', id: `${idPrefix}-${s.category}` },
        h('tbody', {}, ...s.rows.map((r) => h('tr', { class: r.player === me ? 'me' : '' },
          h('td', { class: 'rank' }, String(r.rank)), h('td', {}, swatch(r.player, me), r.name), h('td', { class: 'num' }, formatValue(s.category, r.value)))))))));
}

function currentParts(cur: CurrentSeason, me: PlayerId): Node[] {
  const mine = myPlaces(cur.standings, me).filter((p) => p.place);
  return [
    h('h3', {}, `Season ${cur.season}`),
    h('p', { class: 'muted small', id: 'season-clock' }, `${seasonClock(cur)}. At the end the standings are final and the winners join the hall of fame.`),
    h('p', { class: 'small', id: 'season-mine' }, mine.length === 0
      ? 'You are not ranked yet this season.'
      : `You: ${mine.map((p) => `${categoryLabel(p.category)} ${ordinal(p.place!.rank)} of ${p.place!.of}`).join(' · ')}`),
    h('div', { id: 'season-current' }, standingTables(cur.standings, me, 'season-now')),
  ];
}

/**
 * Redraw only the season under way in a panel `renderSeasons` drew (pushed standings change often; the hall of
 * fame below stays as it is, opened seasons and all). False if the panel has no such part yet.
 */
export function renderCurrentSeason(el: HTMLElement, cur: CurrentSeason, me: PlayerId): boolean {
  const part = el.querySelector('#season-now');
  if (!part) return false;
  part.replaceChildren(...currentParts(cur, me));
  return true;
}

export function renderSeasons(el: HTMLElement, d: SeasonsData, me: PlayerId, on: SeasonsHandlers): void {
  if (!d.current && !d.list) {
    el.replaceChildren(h('p', { class: 'muted' }, 'Loading the seasons…'));
    return;
  }
  const open = new Set([...el.querySelectorAll<HTMLDetailsElement>('details[data-key]')].filter((x) => x.open).map((x) => x.dataset.key));
  const parts: (Node | null)[] = [];
  const cur = d.current;
  if (cur) parts.push(h('div', { id: 'season-now' }, ...currentParts(cur, me)));
  if (d.mine) {
    const rec = myRecord(d.mine);
    parts.push(h('h3', {}, 'Your record'),
      rec.seasons === 0
        ? h('p', { class: 'muted small', id: 'season-record' }, 'No finished season with you in it yet.')
        : h('div', { id: 'season-record' },
          h('p', { class: 'small' }, `${rec.seasons} season${rec.seasons === 1 ? '' : 's'} ranked · ${rec.crowns} legacy crown${rec.crowns === 1 ? '' : 's'} · ${rec.titles} title${rec.titles === 1 ? '' : 's'}`),
          h('p', { class: 'muted small' }, 'Best: ', rec.best.map((b) => `${categoryLabel(b.category)} ${ordinal(b.rank)} (season ${b.season})`).join(' · ')),
          h('ul', { class: 'feed-list', id: 'season-my-history' }, ...d.mine.slice(0, 12).map((s) => h('li', { class: 'feed-item' },
            h('strong', {}, `Season ${s.season}`),
            h('span', { class: 'muted small' }, ` · ${s.placements.map((p) => `${categoryLabel(p.category)} ${placementText(p)}`).join(' · ')}`))))));
  }
  const list = d.list;
  if (list) {
    const fame = hallOfFame(list.seasons).slice(0, 8);
    parts.push(h('h3', {}, 'Hall of fame'));
    if (list.seasons.length === 0) {
      parts.push(h('p', { class: 'muted small', id: 'season-history' }, 'No season has ended yet.'));
    } else {
      parts.push(
        h('p', { class: 'muted small', id: 'season-fame' }, 'Most titles: ', fame.map((f) => `${f.name} ${f.crowns}♛ ${f.titles}`).join(' · ')),
        h('div', { id: 'season-history' }, ...list.seasons.map((s) => {
          const key = seasonKey(s);
          const detail = d.details.get(key);
          const det = h('details', { class: 'season', 'data-key': key, open: open.has(key) || undefined },
            h('summary', {}, `Season ${s.season} · days ${s.first_day + 1}–${s.last_day + 1}${s.days < s.length ? ` (${s.days} of ${s.length} days recorded)` : ''}`),
            h('p', { class: 'small' }, s.summary),
            s.winners.length > 0 ? h('p', { class: 'muted small' }, winnersLine(s)) : null,
            detail ? standingTables(detail.standings, me, `season-${s.season}`) : h('p', { class: 'muted small' }, 'Loading the standings…'));
          det.addEventListener('toggle', () => {
            if (det.open && !d.details.has(key)) on.open(s);
          });
          return det;
        })),
        list.seasons.length < list.total
          ? h('button', { class: 'secondary', id: 'season-more', onclick: () => on.more() }, `Older seasons (${list.total - list.seasons.length} more)`)
          : null,
      );
    }
  }
  el.replaceChildren(...parts.filter((p): p is Node => p !== null));
}
