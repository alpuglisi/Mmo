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

export function offerLine(o: Pick<OfferView, 'id' | 'mine' | 'partner' | 'from_name' | 'to_name' | 'price' | 'status'> & { listing?: number; lineage?: number }): string {
  const bought = o.listing !== undefined ? ` · bought from listing ${o.listing}` : '';
  const partner = o.partner === 0 && o.lineage !== undefined ? `${o.to_name}'s pick` : `${o.to_name}'s #${o.partner}`;
  return `#${o.id}: ${o.from_name}'s #${o.mine} × ${partner}${o.price !== undefined ? ` · price ${o.price}` : ''}${bought} · ${o.status}`;
}

/**
 * The toast for a `mating_update`. A mating bought on the market says so to
 * both sides ("The mating you bought…", "…bought from you"), so a buyer
 * learns it completed without watching the Mates tab.
 */
export function offerUpdateText(o: OfferView, outcome: MatingOutcome | undefined, error: { message: string } | undefined, me: number | null): string {
  const why = error ? ` (${error.message})` : '';
  if (o.listing !== undefined) {
    const buyer = o.from_player === me;
    const pair = buyer ? `your #${o.mine} × ${o.to_name}'s #${o.partner}` : `${o.from_name}'s #${o.mine} × your #${o.partner}`;
    const head = buyer ? `The mating you bought (${pair})` : `The mating ${o.from_name} bought (${pair})`;
    if (outcome) return `${head} happened: ${outcome.conceived ? 'conceived' : 'not conceived'}.${why}`;
    if (o.status === 'failed' || o.status === 'expired' || o.status === 'cancelled') return `${head} did not happen: ${o.status}${buyer && o.price ? ', your renown is refunded' : ''}.${why}`;
    return `${head}: ${o.status}.${why}`;
  }
  const what = outcome ? (outcome.conceived ? 'completed, conceived' : 'completed, no conception') : o.status;
  return `Mating offer ${o.id}: ${what}${why}`;
}

export function outcomeText(o: MatingOutcome | undefined): string | null {
  if (!o) return null;
  // Fertility is the chance a pairing conceives; the failure chance is the separate risk an override of
  // preferences fails outright (0 within both preferences), so show it only when there was one.
  const odds = `${Math.round(o.fertility * 100)}% chance to conceive`;
  const risk = o.failure_chance > 0 ? ` · ${Math.round(o.failure_chance * 100)}% risk the override failed` : '';
  return `${o.conceived ? 'Conceived' : 'Not conceived'} (${odds})${risk}`;
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

const COMPASS = ['E', 'SE', 'S', 'SW', 'W', 'NW', 'N', 'NE'];

/**
 * An accepted agreement's escort line for player `me`: how far the two are
 * and which way the partner animal is from yours (screen y grows south).
 */
export function meetText(o: Pick<OfferView, 'from_player' | 'meet'>, me: number | null): string | null {
  const m = o.meet;
  if (!m) return null;
  const mineIsMine = o.from_player === me;
  const [fx, fy, tx, ty] = mineIsMine ? [m.mine_x, m.mine_y, m.partner_x, m.partner_y] : [m.partner_x, m.partner_y, m.mine_x, m.mine_y];
  if (m.distance <= 2) return 'The two are meeting now.';
  if (m.asleep) return 'One of them is asleep: the meeting resumes when it wakes (the deadline waits).';
  const dirTo = (x: number, y: number) => {
    const angle = Math.atan2(y - fy, x - fx);
    return COMPASS[((Math.round(angle / (Math.PI / 4)) % 8) + 8) % 8];
  };
  const at = m.meet_x !== undefined && m.meet_y !== undefined
    ? ` They meet near ${Math.round(m.meet_x)}, ${Math.round(m.meet_y)} (${dirTo(m.meet_x, m.meet_y)} of yours).` : '';
  const held = m.possessed ? ' One is possessed: the other only walks to the meeting point and waits there.' : ' Possess yours to hurry.';
  return `Walking to meet: the partner is about ${Math.round(m.distance)} tiles ${dirTo(tx, ty)} of yours.${at}${held}`;
}
