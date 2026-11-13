import { describe, expect, it } from 'vitest';
import type { AuctionView, LedgerEntry, ListingView, MatingPreviewView, OwnOrganism, StateView } from '../protocol';
import { parseLedgerView, parseMarketView } from '../protocol';
import {
  auctionText, balanceText, bidProblem, buyProblem, DEFAULT_FILTER, expiresIn, ledgerRows, ledgerTotals, listingCompat, marketQuery,
  mergePages, ownAdults, PAGE_SIZE, pageText, traitSummary,
} from './marketData';

const preview = (extra: Partial<MatingPreviewView> = {}): MatingPreviewView => ({
  mine: 10, partner: 20, compatible: true, fertility: 1, body_plan_fertility: 1, distance_fertility: 1,
  body_plan_gaps: [], mismatch: 0.3, cost: 0, failure_chance: 0, distance: 8, meeting_distance: 1.5, ...extra,
});

function listing(id: number, extra: Partial<ListingView> = {}): ListingView {
  return {
    id, owner: 2, owner_name: 'bob', organism: 100 + id, role: 'stud', sex: 'male', fee: 10, created_tick: id,
    sales: 0, busy: false, pregnant: false, lineage: 1, lineage_name: 'Bob striders', generation: 0, offspring_raised: 0,
    age_days: 20, health: 90, x: 10, y: 12,
    traits: [{ gene: 'body_size', value: 0.45 }, { gene: 'fur', value: 0.8 }, { gene: 'carnivory', value: 0.1 }],
    kind: 'fixed',
    ...extra,
  };
}

const auction = (id: number, a: Partial<AuctionView> = {}, extra: Partial<ListingView> = {}): ListingView =>
  listing(id, { kind: 'auction', fee: 10, expires_tick: 96, auction: { reserve: 10, deadline_tick: 96, bids: 0, min_bid: 10, ...a }, ...extra });

const own = (id: number, extra: Partial<OwnOrganism> = {}): OwnOrganism => ({
  id, lineage: 0, x: 0, y: 0, action: 'graze', sex: 'female', age_days: 20, growth: 1, matured: true, health: 90,
  needs: { hunger: 0, thirst: 0, energy: 100, cold: 0, heat: 0, safety: 0, breeding: 0, social: 0 },
  possessed: false, pregnant: false, generation: 0, ...extra,
});

describe('traitSummary', () => {
  it('names the diet and the strongest features', () => {
    expect(traitSummary(listing(1).traits)).toBe('herbivore, furred');
    expect(traitSummary([
      { gene: 'body_size', value: 0.9 }, { gene: 'armor_plates', value: 0.65 }, { gene: 'toxins', value: 1 },
      { gene: 'wings', value: 0.7 }, { gene: 'fur', value: 0.62 }, { gene: 'carnivory', value: 0.8 },
    ])).toBe('carnivore, toxic, large, winged, armored');
    expect(traitSummary([{ gene: 'body_size', value: 0.1 }, { gene: 'carnivory', value: 0.45 }])).toBe('omnivore, small');
    expect(traitSummary([])).toBe('unremarkable');
  });
});

describe('marketQuery', () => {
  it('asks the server for a page with the filters, the order and the buyer', () => {
    expect(marketQuery(DEFAULT_FILTER, undefined)).toEqual({ type: 'browse_market', limit: PAGE_SIZE, sort: 'fertility', with_own: true, hide_own: true });
    expect(marketQuery({ ...DEFAULT_FILTER, role: 'dam', kind: 'auction', maxFee: 25, compatibleOnly: true, sort: 'ending' }, 7, 'c1')).toEqual({
      type: 'browse_market', limit: PAGE_SIZE, sort: 'ending', mine: 7, cursor: 'c1', role: 'dam', kind: 'auction', max_price: 25,
      compatible: true, hide_own: true,
    });
    // "Compatible only" needs a buyer organism; a later page doesn't repeat one's own listings.
    const q = marketQuery({ ...DEFAULT_FILTER, compatibleOnly: true, hideOwn: false }, undefined, 'c2', 5);
    expect(q).toEqual({ type: 'browse_market', limit: 5, sort: 'fertility', cursor: 'c2' });
  });

  it('merges pages in order, each listing once', () => {
    expect(mergePages([[listing(5), listing(4)], [listing(4), listing(3)], []]).map((l) => l.id)).toEqual([5, 4, 3]);
    expect(mergePages([])).toEqual([]);
  });

  it('says how much of the market is shown', () => {
    const m = (total: number, next?: string) => ({ balance: 0, escrow: 0, slots_left: 0, listings: [], total, next_cursor: next });
    expect(pageText(20, m(57, 'x'))).toBe('Showing 20 of 57');
    expect(pageText(57, m(57))).toBe('All 57 shown');
    expect(pageText(0, m(0))).toBe('');
    expect(pageText(0, null)).toBe('');
  });
});

describe('auctions', () => {
  const l = auction(1, {}, { preview: preview() });
  it('allows a bid at or above the lowest accepted', () => {
    expect(bidProblem(l, 1, 50, 2, own(10), 10)).toBeNull();
    expect(bidProblem(l, 1, 50, 2, own(10), 50)).toBeNull();
  });
  it('says what stands in the way of a bid', () => {
    expect(bidProblem(listing(1), 1, 50, 2, own(10), 10)).toBe('not an auction');
    expect(bidProblem(auction(1, {}, { owner: 1 }), 1, 50, 2, own(10), 10)).toBe('your own auction');
    expect(bidProblem(l, 1, 50, 2, undefined, 10)).toBe('pick one of your adults');
    expect(bidProblem(l, 1, 50, 2, own(10, { sex: 'male' }), 10)).toBe('needs a female');
    expect(bidProblem(l, 1, 50, 2, own(10, { pregnant: true }), 10)).toBe('pregnant');
    expect(bidProblem(auction(1, {}, { preview: preview({ compatible: false }) }), 1, 50, 2, own(10), 10)).toBe('cannot conceive');
    expect(bidProblem(l, 1, 50, 0, own(10), 10)).toBe('no mating slots left');
    expect(bidProblem(l, 1, 50, 2, own(10), 9)).toBe('bid at least 10');
    expect(bidProblem(l, 1, 50, 2, own(10), Number.NaN)).toBe('bid at least 10');
    expect(bidProblem(l, 1, 9, 2, own(10), 10)).toBe('needs 10 renown');
  });
  it('counts what a bidder already holds when raising their own bid', () => {
    const mine = auction(1, { high_bid: 40, high_bidder: 1, bids: 1, min_bid: 42 });
    expect(bidProblem(mine, 1, 2, 2, own(10), 42)).toBeNull();
    expect(bidProblem(mine, 1, 1, 2, own(10), 42)).toBe('needs 2 renown');
    const theirs = auction(1, { high_bid: 40, high_bidder: 3, bids: 1, min_bid: 42 });
    expect(bidProblem(theirs, 1, 2, 2, own(10), 42)).toBe('needs 42 renown');
  });
  it('a buy button refuses an auction', () => {
    expect(buyProblem(l, 1, 50, 2, own(10))).toBe('an auction: bid instead');
  });
  it('describes the highest bid and the time left', () => {
    expect(auctionText(listing(1), 1, 0, 48)).toBe('');
    expect(auctionText(auction(1), 1, 0, 48)).toBe('Auction: reserve 10, no bids yet · closes in 2.0 days');
    expect(auctionText(auction(1, { high_bid: 12, high_bidder: 1, bids: 1 }), 1, 72, 48)).toBe('Auction: high bid 12 (yours) · 1 bid · closes in 12 h');
    expect(auctionText(auction(1, { high_bid: 30, high_bidder: 2, bids: 4 }), 1, 95, 48)).toBe('Auction: high bid 30 · 4 bids · closes in 1 h');
    expect(auctionText(auction(1), 1, 200, 48)).toBe('Auction: reserve 10, no bids yet · closing now');
    // An auction's deadline is not shown as a listing expiry.
    expect(expiresIn(auction(1), 0, 48)).toBeNull();
  });
});

describe('buyProblem', () => {
  const l = listing(1, { preview: preview() });
  it('allows a compatible purchase the player can pay for', () => {
    expect(buyProblem(l, 1, 50, 2, own(10))).toBeNull();
  });
  it('says what stands in the way', () => {
    expect(buyProblem(listing(1, { owner: 1 }), 1, 50, 2, own(10))).toBe('your own listing');
    expect(buyProblem(listing(1, { busy: true }), 1, 50, 2, own(10))).toBe('already promised');
    expect(buyProblem(l, 1, 50, 2, undefined)).toBe('pick one of your adults');
    expect(buyProblem(l, 1, 50, 2, own(10, { sex: 'male' }))).toBe('needs a female');
    expect(buyProblem(l, 1, 50, 2, own(10, { pregnant: true }))).toBe('pregnant');
    expect(buyProblem(listing(1, { preview: preview({ compatible: false }) }), 1, 50, 2, own(10))).toBe('cannot conceive');
    expect(buyProblem(l, 1, 50, 0, own(10))).toBe('no mating slots left');
    expect(buyProblem(l, 1, 9, 2, own(10))).toBe('needs 10 renown');
  });
});

describe('market helpers', () => {
  it('summarizes compatibility, balance, expiry and adults', () => {
    expect(listingCompat(listing(1))).toBeNull();
    expect(listingCompat(listing(1, { preview: preview() }))?.level).toBe('ok');
    expect(balanceText(42, 0)).toBe('42 renown');
    expect(balanceText(42, 10)).toBe('42 renown (+10 in escrow)');
    expect(balanceText(undefined, undefined)).toBe('0 renown');
    expect(expiresIn(listing(1), 0, 48)).toBeNull();
    expect(expiresIn(listing(1, { expires_tick: 144 }), 48, 48)).toBe(2);
    expect(expiresIn(listing(1, { expires_tick: 10 }), 48, 48)).toBe(0);
    const s = { organisms: [own(3), own(1, { matured: false }), own(2, { sex: 'male' })] } as unknown as StateView;
    expect(ownAdults(s).map((o) => o.id)).toEqual([2, 3]);
    expect(ownAdults(null)).toEqual([]);
  });
});

describe('ledger', () => {
  const e = (seq: number, delta: number, kind: string, balance: number): LedgerEntry =>
    ({ seq, player: 1, tick: seq * 10, day: seq, delta, balance, kind, text: `entry ${seq}` });
  const entries = [e(1, 50, 'grant', 50), e(2, 4, 'income', 54), e(3, -25, 'escrow', 29), e(4, 25, 'refund', 54), e(5, 12, 'sale', 66), e(6, 40, 'achievement', 106)];

  it('formats rows newest first with signed amounts', () => {
    const rows = ledgerRows(entries);
    expect(rows.map((r) => r.seq)).toEqual([6, 5, 4, 3, 2, 1]);
    expect(rows[3]).toMatchObject({ amount: '−25', positive: false, kind: 'escrow', balance: 29 });
    expect(rows[0]).toMatchObject({ amount: '+40', kind: 'bonus' });
  });

  it('totals earnings, escrow, sales and refunds', () => {
    expect(ledgerTotals(entries)).toEqual({ earned: 94, spent: 25, sold: 12, refunded: 25 });
  });
});

describe('parsing market data', () => {
  it('parses browse_market data and drops malformed listings', () => {
    const m = parseMarketView({ balance: 40, escrow: 5, slots_left: 3, listings: [listing(1), { id: 'x' }, listing(2)] });
    expect(m?.listings.map((l) => l.id)).toEqual([1, 2]);
    expect(m?.escrow).toBe(5);
    expect(parseMarketView({ balance: 1, listings: [] })).toEqual({ balance: 1, escrow: 0, slots_left: 0, listings: [], total: 0 });
    expect(parseMarketView(null)).toBeNull();
    expect(parseMarketView({ listings: [] })).toBeNull();
  });

  it('parses a page: total, cursor, own listings and auctions', () => {
    const { kind: _k, ...old } = listing(3);
    const m = parseMarketView({
      balance: 40, escrow: 5, slots_left: 3, total: 57, next_cursor: '0.12',
      listings: [listing(1), auction(2), old, { ...auction(4), auction: { reserve: 'x' } }],
      own: [listing(9, { owner: 1 }), { id: 'x' }],
    });
    expect(m?.listings.map((l) => [l.id, l.kind])).toEqual([[1, 'fixed'], [2, 'auction'], [3, 'fixed']]);
    expect(m?.listings[1].auction?.min_bid).toBe(10);
    expect(m?.total).toBe(57);
    expect(m?.next_cursor).toBe('0.12');
    expect(m?.own?.map((l) => l.id)).toEqual([9]);
    // A server before paging: the page is everything.
    const before = parseMarketView({ balance: 1, listings: [old] });
    expect(before?.total).toBe(1);
    expect(before?.next_cursor).toBeUndefined();
    expect(before?.own).toBeUndefined();
  });

  it('parses ledger data', () => {
    const v = parseLedgerView({ balance: 12, escrow: 0, entries: [{ seq: 1, player: 1, tick: 0, day: 0, delta: 12, balance: 12, kind: 'grant', text: 'g' }, { seq: 2 }] });
    expect(v?.entries.length).toBe(1);
    expect(parseLedgerView({ entries: [] })).toBeNull();
  });
});
