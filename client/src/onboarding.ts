// First-session onboarding, the pure part: the guided tour's steps and state
// machine, the one-time contextual hints and what triggers them, and the
// per-user progress kept in localStorage. The DOM side is `ui/onboarding.ts`.
//
// The design doc's "typical session" (check the dashboard and alerts,
// arrange a trade, possess an organism and escort it, log off while the
// lineage carries on) and its three levers (possession, directed matings,
// group commands) set what the tour covers. Everything here is client-only:
// the server neither knows nor stores whether a player has seen the tour.

import type { FeedEntry, OfferEntry } from './model';

// ---------------------------------------------------------------- progress

export type TourStatus = 'new' | 'active' | 'completed' | 'skipped';
export type HintId = 'pregnancy' | 'death' | 'species' | 'offer';
export const HINT_IDS: readonly HintId[] = ['pregnancy', 'death', 'species', 'offer'];

export interface Progress {
  tour: TourStatus;
  /** The step shown (or to resume at) while `tour` is `active`. */
  step: number;
  /** Hints already shown, never shown again. */
  hints: HintId[];
}

export const NEW_PROGRESS: Progress = Object.freeze({ tour: 'new', step: 0, hints: [] as HintId[] }) as Progress;

const STATUSES = new Set<string>(['new', 'active', 'completed', 'skipped']);
const KNOWN_HINTS = new Set<string>(HINT_IDS);

/** Progress from stored JSON; anything unreadable counts as a new player. */
export function parseProgress(raw: string | null | undefined): Progress {
  if (!raw) return { ...NEW_PROGRESS, hints: [] };
  try {
    const p = JSON.parse(raw) as Partial<Progress>;
    const tour = typeof p.tour === 'string' && STATUSES.has(p.tour) ? p.tour : 'new';
    const step = typeof p.step === 'number' && Number.isInteger(p.step) && p.step >= 0 ? p.step : 0;
    const hints = Array.isArray(p.hints) ? [...new Set(p.hints.filter((h): h is HintId => typeof h === 'string' && KNOWN_HINTS.has(h)))] : [];
    return { tour, step, hints };
  } catch {
    return { ...NEW_PROGRESS, hints: [] };
  }
}

/** The storage key for one account: player ids restart with an in-memory server, so the name is part of it. */
export function progressKey(player: number, name: string): string {
  return `evo.onboarding.v1.${player}.${name.toLowerCase()}`;
}

export interface KeyValueStore {
  getItem(key: string): string | null;
  setItem(key: string, value: string): void;
}

function defaultStore(): KeyValueStore | null {
  try {
    return typeof localStorage === 'undefined' ? null : localStorage;
  } catch {
    return null;
  }
}

export function loadProgress(key: string, store: KeyValueStore | null = defaultStore()): Progress {
  try {
    return parseProgress(store?.getItem(key));
  } catch {
    return parseProgress(null);
  }
}

/** Save; false when storage is unavailable (private mode, quota): progress then lasts for this page only. */
export function saveProgress(key: string, p: Progress, store: KeyValueStore | null = defaultStore()): boolean {
  try {
    if (!store) return false;
    store.setItem(key, JSON.stringify(p));
    return true;
  } catch {
    return false;
  }
}

/** `?tour=off` (or `onboarding=off`) turns the tour and the hints off for this page: the e2e tests use it. */
export function onboardingDisabled(search: string): boolean {
  const q = new URLSearchParams(search);
  return ['tour', 'onboarding'].some((k) => ['off', '0', 'false', 'no'].includes((q.get(k) ?? '').toLowerCase()));
}

// ---------------------------------------------------------------- tour

/** The sidebar tabs a step can open (the game screen's tab ids). */
export type TourTab = 'organism' | 'matings' | 'market' | 'genetics' | 'lineage' | 'seasons' | 'world';

export interface TourStep {
  id: string;
  title: string;
  body: string;
  /** CSS selectors of what to point at; the first visible one wins. None: a centered card. */
  targets: string[];
  /** Open this tab first (wide layout only: on a phone the sheet stays closed and the tab button is pointed at). */
  tab?: TourTab;
  /** Select one of the player's organisms first, so the inspector and the Possess button have something to show. */
  selectOwn?: boolean;
}

export interface TourContext {
  /** Touch or narrow screen: say "tap" and "long-press". */
  touch: boolean;
  name: string;
}

export function tourSteps({ touch, name }: TourContext): TourStep[] {
  const tap = touch ? 'Tap' : 'Click';
  return [
    {
      id: 'welcome',
      title: `Welcome${name ? `, ${name}` : ''}`,
      body: 'Your founders are alive in a shared world that keeps running while you are away. You never edit their genes: you steer the lineage by possessing organisms, arranging matings and giving group commands. Your score is the share of breeding alleles that started with you. This short tour shows where things are.',
      targets: [],
    },
    {
      id: 'map',
      title: 'The map and the camera',
      body: touch
        ? 'Drag to pan and pinch to zoom. Follow keeps the camera on your lineage (or the organism you possess). Migrate sends your adults somewhere: a group command. Legend explains the rings and icons.'
        : 'Drag to pan, use the wheel or + and − to zoom. Follow keeps the camera on your lineage (or the organism you possess). Migrate… sends your adults somewhere: a group command.',
      targets: ['.toolbar', '#stage'],
    },
    {
      id: 'lineage',
      title: 'Find your lineage',
      body: `Yours have a yellow ellipse at their feet. The Lineage tab counts them, ranks every player by legacy share and lists your organisms: ${tap.toLowerCase()} one to center the map on it.`,
      targets: ['#tab-lineage'],
      tab: 'lineage',
    },
    {
      id: 'inspector',
      title: 'The inspector',
      body: `${tap} any organism on the map to inspect it. For yours you see its action, age, health and the needs that drive it; for others only what can be seen.`,
      targets: ['#panel-organism:not([hidden])', '#tab-organism'],
      tab: 'organism',
      selectOwn: true,
    },
    {
      id: 'possess',
      title: 'Possess an organism',
      body: touch
        ? 'Press Possess (or long-press one of yours) to take direct control, then tap the map to walk it there: to food, away from danger, or to a mate. Release it and it goes back to its genes.'
        : 'Press Possess (or hold the mouse on one of yours) to take direct control, then click the map to walk it there: to food, away from danger, or to a mate. Release it and it goes back to its genes.',
      targets: ['#btn-possess', '#btn-release', '#fab-possess', '#stage'],
      selectOwn: true,
    },
    {
      id: 'market',
      title: 'Matings and the Market',
      body: 'Directed matings pair one of your adults with any partner, including another player\'s (they must agree; the two must meet). The Market tab spends renown, earned from your legacy share, on stud fees and auctions.',
      targets: ['#tab-market'],
      tab: 'market',
    },
    {
      id: 'seasons',
      title: 'Seasons',
      body: 'Ranked seasons last a few in-game weeks. The Seasons tab shows the one under way, your places and the hall of fame.',
      targets: ['#tab-seasons'],
      tab: 'seasons',
    },
    {
      id: 'alerts',
      title: 'Alerts',
      body: 'Births, deaths, needs, milestones and market news arrive as alerts: a badge on the Genetics tab, toasts on the map, and a feed under the genetics dashboard. Alert preferences choose which ones show.',
      targets: ['#tab-genetics'],
      tab: 'genetics',
    },
    {
      id: 'done',
      title: 'That\'s it',
      body: 'Your lineage carries on while you are offline. A few tips will appear the first time something new happens. Help replays this tour.',
      targets: ['#help-btn'],
    },
  ];
}

export type TourEvent =
  | { type: 'start' }
  | { type: 'next' }
  | { type: 'back' }
  | { type: 'skip' }
  | { type: 'goto'; step: number };

/**
 * The tour's state machine. `start` begins (or replays) from the first step,
 * or resumes a tour left active at a reload; `next` past the last step
 * completes it; `skip` ends it early. Events that make no sense in a state
 * (`next` on a finished tour) change nothing.
 */
export function tourReduce(p: Progress, e: TourEvent, total: number): Progress {
  const last = Math.max(0, total - 1);
  const clamp = (s: number) => Math.max(0, Math.min(last, s));
  switch (e.type) {
    case 'start':
      return p.tour === 'active' ? { ...p, step: clamp(p.step) } : { ...p, tour: 'active', step: 0 };
    case 'next':
      if (p.tour !== 'active') return p;
      return p.step >= last ? { ...p, tour: 'completed', step: 0 } : { ...p, step: p.step + 1 };
    case 'back':
      return p.tour === 'active' ? { ...p, step: clamp(p.step - 1) } : p;
    case 'goto':
      return p.tour === 'active' ? { ...p, step: clamp(e.step) } : p;
    case 'skip':
      return p.tour === 'active' || p.tour === 'new' ? { ...p, tour: 'skipped', step: 0 } : p;
  }
}

/** Open the tour by itself: a new player, or one who reloaded mid-tour. */
export function autoStarts(p: Progress): boolean {
  return p.tour === 'new' || p.tour === 'active';
}

// ---------------------------------------------------------------- hints

export interface HintDef {
  title: string;
  body: string;
  /** The tab "Show me" opens. */
  tab: TourTab;
}

export const HINTS: Record<HintId, HintDef> = {
  pregnancy: {
    title: 'A pregnancy',
    body: 'One of your organisms is pregnant (green ring). Keep it fed and safe: possess it to walk it to food or away from predators. The Lineage tab counts pregnancies.',
    tab: 'lineage',
  },
  death: {
    title: 'A death in your lineage',
    body: 'Organisms die of hunger, thirst, cold, predators, age and accidents. The Genetics tab charts deaths by cause, which shows what is pressing on your lineage.',
    tab: 'genetics',
  },
  species: {
    title: 'A new species',
    body: 'Part of your lineage has drifted far enough to split off as a species of its own. Its alleles still count toward your legacy wherever they live. The World tab shows the species tree.',
    tab: 'world',
  },
  offer: {
    title: 'A mating offer',
    body: 'Another player wants to mate one of their organisms with yours. Accept or decline in the Matings tab; an accepted pair must still meet on the map.',
    tab: 'matings',
  },
};

/** What the hint triggers look at: the client model, reduced to plain data. */
export interface HintInput {
  player: number;
  organisms: readonly { pregnant: boolean }[];
  feed: readonly FeedEntry[];
  offers: Iterable<OfferEntry>;
}

const SPECIES_TEXT = /^species founded/i;

/** Which hints the input calls for, in `HINT_IDS` order, leaving out the ones already seen. */
export function dueHints(input: HintInput, seen: readonly HintId[]): HintId[] {
  const due = new Set<HintId>();
  if (input.organisms.some((o) => o.pregnant)) due.add('pregnancy');
  for (const e of input.feed) {
    if (e.kind === 'death') due.add('death');
    if (e.kind === 'achievement' && SPECIES_TEXT.test(e.text)) due.add('species');
  }
  for (const o of input.offers) {
    if (o.offer.to_player === input.player && o.offer.status === 'pending') due.add('offer');
  }
  return HINT_IDS.filter((h) => due.has(h) && !seen.includes(h));
}

// ---------------------------------------------------------------- layout

export const CARD_MARGIN = 16;
const CARD_GAP = 12;

/**
 * Where the tour card goes for a target rectangle (null: centered), the
 * card's size and the viewport. Phones: full width less the gutters, above
 * a target in the lower half of the screen, below one in the upper half.
 * Wide screens: beside the target (left, right, below, above: the first
 * that fits), else centered on it. Always inside the viewport.
 */
export function placeCard(r: { left: number; top: number; right: number; bottom: number; width: number; height: number } | null,
  w: number, ch: number, vw: number, vh: number, narrow: boolean): { left: number; top: number } {
  const clampX = (x: number) => Math.max(CARD_MARGIN, Math.min(vw - CARD_MARGIN - w, x));
  const clampY = (y: number) => Math.max(CARD_MARGIN, Math.min(vh - CARD_MARGIN - ch, y));
  if (!r) return { left: clampX((vw - w) / 2), top: clampY((vh - ch) / 2) };
  if (narrow) {
    const mid = (r.top + r.bottom) / 2;
    const top = mid > vh / 2 ? r.top - CARD_GAP - ch : r.bottom + CARD_GAP;
    // A target covering most of the screen (the map): centered over it.
    if (top < CARD_MARGIN || top + ch > vh - CARD_MARGIN) return { left: clampX((vw - w) / 2), top: clampY((vh - ch) / 2) };
    return { left: clampX((vw - w) / 2), top: clampY(top) };
  }
  const cy = clampY(r.top + r.height / 2 - ch / 2);
  const cx = clampX(r.left + r.width / 2 - w / 2);
  if (r.left - CARD_GAP - w >= CARD_MARGIN) return { left: r.left - CARD_GAP - w, top: cy };
  if (r.right + CARD_GAP + w <= vw - CARD_MARGIN) return { left: clampX(r.right + CARD_GAP), top: cy };
  if (r.bottom + CARD_GAP + ch <= vh - CARD_MARGIN) return { left: cx, top: clampY(r.bottom + CARD_GAP) };
  if (r.top - CARD_GAP - ch >= CARD_MARGIN) return { left: cx, top: r.top - CARD_GAP - ch };
  return { left: clampX(r.left + r.width / 2 - w / 2), top: clampY(r.top + r.height / 2 - ch / 2) };
}
