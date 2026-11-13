// Data prep for the Market tab (stud marketplace, renown balance, ledger):
// pure functions over the server's `browse_market` / `ledger` data and the
// state, so they are unit-tested without a DOM.

import type { ClientMsg, LedgerEntry, ListingView, MarketSort, MarketView, OrgId, OwnOrganism, PlayerId, StateView, TraitView } from '../protocol';
import { compatSummary, type CompatSummary } from './mating';

export type RoleFilter = 'any' | 'stud' | 'dam';
export type KindFilter = 'any' | 'fixed' | 'auction';

/** The Market tab's filters and order; the server applies them (`browse_market`). */
export interface MarketFilter {
  role: RoleFilter;
  kind: KindFilter;
  /** Highest price shown (null: any): the fee, or an auction's current bid. */
  maxFee: number | null;
  /** Only listings the chosen organism can conceive with. */
  compatibleOnly: boolean;
  /** Hide the player's own listings (they show under "My listings"). */
  hideOwn: boolean;
  sort: MarketSort;
}

export const DEFAULT_FILTER: MarketFilter = { role: 'any', kind: 'any', maxFee: null, compatibleOnly: false, hideOwn: true, sort: 'fertility' };

/** Listings per page the Market tab asks for. */
export const PAGE_SIZE = 20;

/**
 * The `browse_market` request for one page: the filters, the buyer's
 * organism (for compatibility), and the cursor of the page before (none: the
 * first page, which also asks for the player's own listings).
 */
export function marketQuery(f: MarketFilter, mine: OrgId | undefined, cursor?: string, limit = PAGE_SIZE): ClientMsg {
  const q: ClientMsg & { type: 'browse_market' } = { type: 'browse_market', limit, sort: f.sort };
  if (mine) q.mine = mine;
  if (cursor) q.cursor = cursor;
  else q.with_own = true;
  if (f.role !== 'any') q.role = f.role;
  if (f.kind !== 'any') q.kind = f.kind;
  if (f.maxFee !== null) q.max_price = f.maxFee;
  if (f.compatibleOnly && mine) q.compatible = true;
  if (f.hideOwn) q.hide_own = true;
  return q;
}

/** The pages loaded so far, in order, each listing once (a listing may move between pages as the market changes). */
export function mergePages(pages: ListingView[][]): ListingView[] {
  const seen = new Set<number>();
  const out: ListingView[] = [];
  for (const page of pages) {
    for (const l of page) {
      if (seen.has(l.id)) continue;
      seen.add(l.id);
      out.push(l);
    }
  }
  return out;
}

/** "Showing 20 of 57" under the listings. */
export function pageText(shown: number, m: MarketView | null): string {
  if (!m) return '';
  if (m.total === 0) return '';
  return m.next_cursor ? `Showing ${shown} of ${m.total}` : `All ${m.total} shown`;
}

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

/** Why the player can't buy a listing right now, or null if they can. */
export function buyProblem(l: ListingView, me: PlayerId, balance: number, slotsLeft: number, mine: OwnOrganism | undefined): string | null {
  if (l.owner === me) return 'your own listing';
  if (l.kind === 'auction') return 'an auction: bid instead';
  if (l.busy) return 'already promised';
  if (!mine) return 'pick one of your adults';
  if (mine.sex === l.sex) return `needs a ${l.sex === 'male' ? 'female' : 'male'}`;
  if (mine.pregnant || (l.pregnant && l.sex === 'female')) return 'pregnant';
  if (l.preview && !l.preview.compatible) return 'cannot conceive';
  if (slotsLeft <= 0) return 'no mating slots left';
  if (balance < l.fee) return `needs ${l.fee} renown`;
  return null;
}

/**
 * Why the player can't bid `amount` on an auction right now, or null if
 * they can. Raising one's own highest bid only needs the difference.
 */
export function bidProblem(l: ListingView, me: PlayerId, balance: number, slotsLeft: number, mine: OwnOrganism | undefined, amount: number): string | null {
  const a = l.auction;
  if (!a) return 'not an auction';
  if (l.owner === me) return 'your own auction';
  if (!mine) return 'pick one of your adults';
  if (mine.sex === l.sex) return `needs a ${l.sex === 'male' ? 'female' : 'male'}`;
  if (mine.pregnant) return 'pregnant';
  if (l.preview && !l.preview.compatible) return 'cannot conceive';
  if (slotsLeft <= 0) return 'no mating slots left';
  if (!Number.isFinite(amount) || amount < a.min_bid) return `bid at least ${a.min_bid}`;
  const held = a.high_bidder === me ? a.high_bid ?? 0 : 0;
  if (balance < amount - held) return `needs ${amount - held} renown`;
  return null;
}

/** One line on an auction: the highest bid (and whether it is yours), the bid count and the time left. */
export function auctionText(l: ListingView, me: PlayerId, tick: number, ticksPerDay: number): string {
  const a = l.auction;
  if (!a) return '';
  const high = a.high_bid === undefined
    ? `reserve ${a.reserve}, no bids yet`
    : `high bid ${a.high_bid}${a.high_bidder === me ? ' (yours)' : ''} · ${a.bids} bid${a.bids === 1 ? '' : 's'}`;
  const left = Math.max(0, a.deadline_tick - tick);
  const when = left === 0 ? 'closing now'
    : left < ticksPerDay ? `closes in ${Math.max(1, Math.round((left / ticksPerDay) * 24))} h`
      : `closes in ${(left / ticksPerDay).toFixed(1)} days`;
  return `Auction: ${high} · ${when}`;
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

/** Days until a listing's expiry from the current tick (null: no expiry, or an auction, which says so itself). */
export function expiresIn(l: ListingView, tick: number, ticksPerDay: number): number | null {
  if (l.expires_tick === undefined || l.kind === 'auction') return null;
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
