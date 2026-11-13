// Data prep for the Market tab (stud marketplace, renown balance, ledger):
// pure functions over the server's `browse_market` / `ledger` data and the
// state, so they are unit-tested without a DOM.

import type { LedgerEntry, ListingView, OwnOrganism, PlayerId, StateView, TraitView } from '../protocol';
import { compatSummary, type CompatSummary } from './mating';

export type RoleFilter = 'any' | 'stud' | 'dam';

export interface MarketFilter {
  role: RoleFilter;
  /** Highest fee shown (null: any). */
  maxFee: number | null;
  /** Only listings the chosen organism can conceive with. */
  compatibleOnly: boolean;
  /** Hide the player's own listings. */
  hideOwn: boolean;
}

export const DEFAULT_FILTER: MarketFilter = { role: 'any', maxFee: null, compatibleOnly: false, hideOwn: true };

/** Words for traits clearly above (or, for body size, below) the middle. */
const HIGH: Record<string, string> = {
  body_size: 'large', leg_length: 'long-legged', muscle: 'muscular', fur: 'furred', scales: 'scaled',
  armor_plates: 'armored', wings: 'winged', swimming: 'swimmer', climbing: 'climber', burrowing: 'burrower',
  camouflage: 'camouflaged', coloration: 'brightly colored', toxins: 'toxic',
};

/**
 * A short appearance summary from a listing's key traits: the diet, then
 * the most pronounced features (strongest first, at most `max`).
 */
export function traitSummary(traits: TraitView[], max = 4): string {
  const v = new Map(traits.map((t) => [t.gene, t.value]));
  const words: [number, string][] = [];
  const size = v.get('body_size');
  if (size !== undefined && size < 0.3) words.push([0.3 - size + 0.1, 'small']);
  for (const [gene, word] of Object.entries(HIGH)) {
    const x = v.get(gene);
    if (x !== undefined && x >= 0.6) words.push([x - 0.6 + 0.1, word]);
  }
  words.sort((a, b) => b[0] - a[0]);
  const c = v.get('carnivory');
  const diet = c === undefined ? null : c < 0.3 ? 'herbivore' : c < 0.6 ? 'omnivore' : 'carnivore';
  const parts = [...(diet ? [diet] : []), ...words.slice(0, max).map((w) => w[1])];
  return parts.length ? parts.join(', ') : 'unremarkable';
}

/** The listing's compatibility with the buyer's chosen organism, if previewed. */
export function listingCompat(l: ListingView): CompatSummary | null {
  return l.preview ? compatSummary(l.preview) : null;
}

/**
 * Listings passing the filter, best matches first: compatible before
 * incompatible or unknown, then higher fertility, then lower fee, then
 * newer.
 */
export function filterListings(listings: ListingView[], f: MarketFilter, me: PlayerId): ListingView[] {
  const rank = (l: ListingView) => (l.preview ? (l.preview.compatible ? 0 : 2) : 1);
  return listings
    .filter((l) => (f.role === 'any' || l.role === f.role)
      && (f.maxFee === null || l.fee <= f.maxFee)
      && (!f.hideOwn || l.owner !== me)
      && (!f.compatibleOnly || l.preview?.compatible === true))
    .sort((a, b) => rank(a) - rank(b)
      || (b.preview?.fertility ?? 0) - (a.preview?.fertility ?? 0)
      || a.fee - b.fee
      || b.id - a.id);
}

/** Why the player can't buy a listing right now, or null if they can. */
export function buyProblem(l: ListingView, me: PlayerId, balance: number, slotsLeft: number, mine: OwnOrganism | undefined): string | null {
  if (l.owner === me) return 'your own listing';
  if (l.busy) return 'already promised';
  if (!mine) return 'pick one of your adults';
  if (mine.sex === l.sex) return `needs a ${l.sex === 'male' ? 'female' : 'male'}`;
  if (mine.pregnant || (l.pregnant && l.sex === 'female')) return 'pregnant';
  if (l.preview && !l.preview.compatible) return 'cannot conceive';
  if (slotsLeft <= 0) return 'no mating slots left';
  if (balance < l.fee) return `needs ${l.fee} renown`;
  return null;
}

/** The player's adults that could be offered or used as a buyer, by id. */
export function ownAdults(s: StateView | null): OwnOrganism[] {
  return (s?.organisms ?? []).filter((o) => o.matured).sort((a, b) => a.id - b.id);
}

/** Balance with what is held in escrow, for the top bar and the Market tab. */
export function balanceText(balance: number | undefined, escrow: number | undefined): string {
  const b = balance ?? 0;
  return escrow ? `${b} renown (+${escrow} in escrow)` : `${b} renown`;
}

/** Days until a listing's expiry from the current tick (null: no expiry). */
export function expiresIn(l: ListingView, tick: number, ticksPerDay: number): number | null {
  if (l.expires_tick === undefined) return null;
  return Math.max(0, (l.expires_tick - tick) / ticksPerDay);
}

export interface LedgerRow {
  seq: number;
  day: number;
  /** "+12" or "−30". */
  amount: string;
  positive: boolean;
  kind: string;
  text: string;
  balance: number;
}

const KIND_LABEL: Record<string, string> = {
  grant: 'grant', income: 'income', achievement: 'bonus', escrow: 'escrow', sale: 'sale', refund: 'refund',
};

/** Ledger entries as table rows, newest first. */
export function ledgerRows(entries: LedgerEntry[]): LedgerRow[] {
  return [...entries].sort((a, b) => b.seq - a.seq).map((e) => ({
    seq: e.seq,
    day: e.day,
    amount: e.delta >= 0 ? `+${e.delta}` : `−${-e.delta}`,
    positive: e.delta >= 0,
    kind: KIND_LABEL[e.kind] ?? e.kind,
    text: e.text,
    balance: e.balance,
  }));
}

/** Income, spending and sales over the given entries (for the ledger summary). */
export function ledgerTotals(entries: LedgerEntry[]): { earned: number; spent: number; sold: number; refunded: number } {
  const t = { earned: 0, spent: 0, sold: 0, refunded: 0 };
  for (const e of entries) {
    if (e.kind === 'income' || e.kind === 'grant' || e.kind === 'achievement') t.earned += e.delta;
    else if (e.kind === 'escrow') t.spent -= e.delta;
    else if (e.kind === 'sale') t.sold += e.delta;
    else if (e.kind === 'refund') t.refunded += e.delta;
  }
  return t;
}
