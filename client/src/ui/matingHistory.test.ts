import { describe, expect, it } from 'vitest';
import type { OfferEntry } from '../model';
import type { MatingRecord, OfferView } from '../protocol';
import { historyRows } from './matingHistory';

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
});
