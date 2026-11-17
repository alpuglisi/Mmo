// The Matings tab's "Recent outcomes": closed offers seen live this session
// merged with the server's history (`GET /api/matings/history`), which also
// holds matings within the player's own lineage and what happened while the
// tab was closed or before a reload. One row per offer (the live one wins).

import type { OfferEntry } from '../model';
import type { MatingOutcome, MatingRecord, OfferView } from '../protocol';
import { friendlyError } from './friendlyErrors';

export interface HistoryRow {
  key: string;
  /** Tick it closed or happened (newest first). */
  at: number;
  status: string;
  line: string;
  outcome: string | null;
  error: string | null;
}

export function offerLine(o: Pick<OfferView, 'id' | 'mine' | 'partner' | 'from_name' | 'to_name' | 'price' | 'status'>): string {
  return `#${o.id}: ${o.from_name}'s #${o.mine} × ${o.to_name}'s #${o.partner}${o.price !== undefined ? ` · price ${o.price}` : ''} · ${o.status}`;
}

export function outcomeText(o: MatingOutcome | undefined): string | null {
  if (!o) return null;
  return `${o.conceived ? 'Conceived' : 'Not conceived'} · fertility ${Math.round(o.fertility * 100)}% · failure chance ${Math.round(o.failure_chance * 100)}%`;
}

export function historyRows(closed: OfferEntry[], log: MatingRecord[], limit = 30): HistoryRow[] {
  const rows: HistoryRow[] = [];
  const seen = new Set<number>();
  for (const e of closed) {
    seen.add(e.offer.id);
    rows.push({
      key: `o${e.offer.id}`, at: e.offer.created_tick, status: e.offer.status, line: offerLine(e.offer),
      outcome: outcomeText(e.outcome), error: e.error ? friendlyError(e.error) : null,
    });
  }
  for (const r of log) {
    if (r.offer) {
      if (seen.has(r.offer.id)) continue;
      seen.add(r.offer.id);
      rows.push({
        key: `o${r.offer.id}`, at: r.tick, status: r.offer.status, line: offerLine(r.offer),
        outcome: outcomeText(r.outcome), error: r.error ? friendlyError(r.error) : null,
      });
    } else {
      rows.push({
        key: `m${r.tick}:${r.mine}:${r.partner}`, at: r.tick, status: 'completed',
        line: `Within your lineage: #${r.mine} × #${r.partner}`,
        outcome: outcomeText(r.outcome), error: r.error ? friendlyError(r.error) : null,
      });
    }
  }
  return rows.sort((a, b) => b.at - a.at).slice(0, limit);
}
