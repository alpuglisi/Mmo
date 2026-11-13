import { describe, expect, it } from 'vitest';
import type { LedgerEntry, ListingView, MatingPreviewView, OwnOrganism, StateView } from '../protocol';
import { parseLedgerView, parseMarketView } from '../protocol';
import {
  balanceText, buyProblem, DEFAULT_FILTER, expiresIn, filterListings, ledgerRows, ledgerTotals, listingCompat, ownAdults, traitSummary,
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
    ...extra,
  };
}

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

describe('filterListings', () => {
  const list = [
    listing(1, { fee: 30, preview: preview({ fertility: 0.7 }) }),
    listing(2, { fee: 5, role: 'dam', sex: 'female', preview: preview() }),
    listing(3, { fee: 1, preview: preview({ compatible: false, fertility: 0 }) }),
    listing(4, { fee: 2 }),
    listing(5, { owner: 1, owner_name: 'ann' }),
  ];

  it('ranks compatible listings first, then fertility and fee, and hides your own', () => {
    expect(filterListings(list, DEFAULT_FILTER, 1).map((l) => l.id)).toEqual([2, 1, 4, 3]);
    expect(filterListings(list, { ...DEFAULT_FILTER, hideOwn: false }, 1).map((l) => l.id)).toContain(5);
  });

  it('filters by role, fee and compatibility', () => {
    expect(filterListings(list, { ...DEFAULT_FILTER, role: 'stud' }, 1).map((l) => l.id)).toEqual([1, 4, 3]);
    expect(filterListings(list, { ...DEFAULT_FILTER, role: 'dam' }, 1).map((l) => l.id)).toEqual([2]);
    expect(filterListings(list, { ...DEFAULT_FILTER, maxFee: 5 }, 1).map((l) => l.id)).toEqual([2, 4, 3]);
    expect(filterListings(list, { ...DEFAULT_FILTER, compatibleOnly: true }, 1).map((l) => l.id)).toEqual([2, 1]);
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
    expect(parseMarketView({ balance: 1, listings: [] })).toEqual({ balance: 1, escrow: 0, slots_left: 0, listings: [] });
    expect(parseMarketView(null)).toBeNull();
    expect(parseMarketView({ listings: [] })).toBeNull();
  });

  it('parses ledger data', () => {
    const v = parseLedgerView({ balance: 12, escrow: 0, entries: [{ seq: 1, player: 1, tick: 0, day: 0, delta: 12, balance: 12, kind: 'grant', text: 'g' }, { seq: 2 }] });
    expect(v?.entries.length).toBe(1);
    expect(parseLedgerView({ entries: [] })).toBeNull();
  });
});
