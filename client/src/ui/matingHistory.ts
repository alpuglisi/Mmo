// The Matings tab's "Recent outcomes": closed offers seen live this session
// merged with the server's history (`GET /api/matings/history`), which also
// holds matings within the player's own lineage and what happened while the
// tab was closed or before a reload. One row per offer (the live one wins).

import { tiles } from './dom';
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
  // Fertility is the chance a pairing conceives; the failure chance is the separate risk that a pairing
  // outside the animals' preferences fails outright. A conception needs neither explained; a miss says why
  // it may have missed (playtest 6: "Conceived · 54% risk the override failed" read as a contradiction).
  const odds = `${Math.round(o.fertility * 100)}% chance to conceive`;
  if (o.conceived) return `Conceived (${odds})`;
  const risk = o.failure_chance > 0 ? `; outside their preferences, it also had a ${Math.round(o.failure_chance * 100)}% chance to fail outright` : '';
  return `Not conceived (${odds}${risk})`;
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
  // A meeting point on your animal has no direction (playtest 11: atan2(0, 0) read "E of yours").
  const where = (x: number, y: number) => Math.hypot(x - fx, y - fy) < 1.5 ? 'at your animal' : `${dirTo(x, y)} of yours`;
  const at = m.meet_x !== undefined && m.meet_y !== undefined
    ? ` They meet near ${Math.round(m.meet_x)}, ${Math.round(m.meet_y)} (${where(m.meet_x, m.meet_y)}).` : '';
  const held = m.possessed ? ' One is possessed: the other only walks to the meeting point and waits there.' : ' Possess yours to hurry.';
  return `Walking to meet: the partner is about ${tiles(m.distance)} ${dirTo(tx, ty)} of yours.${at}${held}`;
}

/**
 * How far apart the two animals of a pending request are, from what this
 * client can see (playtest 10: Accept gave no idea of the walk). Prefers the
 * server's `meet.distance`; otherwise measures from the requester's animal in
 * sight to `ours` (the requested partner, or the nearest free adult for a
 * request to the lineage). Null when the requester's animal is out of sight.
 */
export function apartText(
  o: Pick<OfferView, 'mine' | 'partner' | 'meet'>,
  own: readonly { id: number; x: number; y: number; matured?: boolean; pregnant?: boolean }[],
  seen: readonly { id: number; x: number; y: number }[],
): string | null {
  if (o.meet) return `About ${tiles(o.meet.distance)} apart.`;
  const theirs = seen.find((n) => n.id === o.mine);
  if (!theirs) return null;
  const d = (a: { x: number; y: number }) => Math.hypot(a.x - theirs.x, a.y - theirs.y);
  if (o.partner) {
    const ours = own.find((m) => m.id === o.partner);
    return ours ? `About ${tiles(d(ours))} apart.` : null;
  }
  const free = own.filter((m) => m.matured !== false && !m.pregnant);
  if (free.length === 0) return null;
  const best = Math.min(...free.map(d));
  return `Your nearest free adult is about ${tiles(best)} from theirs.`;
}
