import { describe, expect, it } from 'vitest';
import type { OfferEntry } from '../model';
import type { MatingRecord, OfferView } from '../protocol';
import { historyRows, meetText, offerLine, offerUpdateText, outcomeText } from './matingHistory';

const offer = (id: number, status: OfferView['status'], tick: number): OfferView =>
  ({ id, from_player: 1, from_name: 'ann', to_player: 2, to_name: 'bob', mine: 10, partner: 20, status, created_tick: tick } as OfferView);
const outcome = { mismatch: 0, cost: 0, failure_chance: 0.1, fertility: 0.9, conceived: true };

describe('mating history', () => {
  it('merges live closed offers with the server history, one row per offer, newest first', () => {
    const live: OfferEntry[] = [{ offer: offer(5, 'declined', 500), incoming: false }];
    const log: MatingRecord[] = [
      { tick: 600, mine: 11, partner: 12, outcome },
      { tick: 510, offer: offer(5, 'declined', 500), mine: 10, partner: 20 },
      { tick: 300, offer: offer(3, 'failed', 290), mine: 10, partner: 20, error: { code: 'already_pregnant', message: 'the female is already pregnant' } },
    ];
    const rows = historyRows(live, log);
    expect(rows.map((r) => r.key)).toEqual(['m600:11:12', 'o5', 'o3']);
    expect(rows[0].line).toContain('Within your lineage: #11 × #12');
    expect(rows[0].outcome).toContain('Conceived');
    expect(rows[2].error).toContain('already pregnant');
    expect(rows[2].error).not.toContain('already_pregnant');
  });

  it('tells the buyer and the seller when a bought mating completes or fails', () => {
    const bought = { ...offer(7, 'completed', 100), listing: 4, price: 30 };
    expect(offerUpdateText(bought, outcome, undefined, 1)).toBe("The mating you bought (your #10 × bob's #20) happened: conceived.");
    expect(offerUpdateText(bought, outcome, undefined, 2)).toBe("The mating ann bought (ann's #10 × your #20) happened: conceived.");
    expect(offerUpdateText({ ...bought, status: 'failed' }, undefined, { message: 'died' }, 1)).toContain('did not happen: failed, your renown is refunded. (died)');
    expect(offerUpdateText(offer(8, 'declined', 100), undefined, undefined, 1)).toBe('Mating offer 8: declined');
    const rows = historyRows([{ offer: bought, outcome, incoming: false }], []);
    expect(rows[0].line).toContain('bought from listing 4');
  });
});

describe('escort and lineage offers', () => {
  it('says how far and which way the partner is, for either side', () => {
    const meet = { mine_x: 4, mine_y: 4, partner_x: 60, partner_y: 4, distance: 56 };
    expect(meetText({ from_player: 1, meet }, 1)).toContain('56 tiles E of yours');
    expect(meetText({ from_player: 1, meet }, 2)).toContain('56 tiles W of yours');
    expect(meetText({ from_player: 1, meet: { ...meet, distance: 1 } }, 1)).toBe('The two are meeting now.');
    expect(meetText({ from_player: 1, meet: { ...meet, asleep: true } }, 1)).toContain('One of them is asleep');
    expect(meetText({ from_player: 1, meet: { ...meet, meet_x: 32, meet_y: 4, possessed: true } }, 1))
      .toContain('meet near 32, 4 (E of yours). One is possessed');
    expect(meetText({ from_player: 1 }, 1)).toBeNull();
  });
  it("names an offer to a lineage as the owner's pick, never #0", () => {
    const o = { id: 3, mine: 5, partner: 0, from_name: 'ann', to_name: 'bob', status: 'pending' as const, lineage: 2 };
    expect(offerLine(o)).toBe("#3: ann's #5 × bob's pick · pending");
  });
});

describe('outcomeText', () => {
  it('reads fertility as the chance to conceive and shows an override risk only when there was one', () => {
    const t = outcomeText({ conceived: true, fertility: 0.08, failure_chance: 0, mismatch: 0, cost: 0 } as never);
    expect(t).toBe('Conceived (8% chance to conceive)');
    expect(outcomeText({ conceived: false, fertility: 0.9, failure_chance: 0.16, mismatch: 0.4, cost: 1 } as never))
      .toBe('Not conceived (90% chance to conceive; outside their preferences, it also had a 16% chance to fail outright)');
    expect(outcomeText({ conceived: true, fertility: 0.25, failure_chance: 0.54, mismatch: 0.4, cost: 1 } as never)).toBe('Conceived (25% chance to conceive)');
  });
});
