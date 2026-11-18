// Plain-language texts for the server's mating error codes (the matings form,
// offers and the history), instead of "request_mating: … (already_pregnant)".

import type { ErrorInfo } from '../protocol';

/** Within this distance (tiles) two organisms can mate (sim `MEETING_DISTANCE`). */
export const MEETING_TILES = 1.5;

const capital = (s: string): string => (s ? s[0].toUpperCase() + s.slice(1) : s);

export function friendlyError(e: Pick<ErrorInfo, 'code' | 'message'>): string {
  switch (e.code) {
    case 'already_pregnant':
      return 'The female is already pregnant: pick another female, or wait until she gives birth.';
    case 'too_far': {
      const d = /([\d.]+) tiles apart/.exec(e.message)?.[1];
      return `They are ${d ? `${d} tiles` : 'too far'} apart: the two must be within ${MEETING_TILES} tiles to mate. Walk one to the other: "Walk to the partner" under the Mates form does it, or possess one and click the map.`;
    }
    case 'same_sex':
      return 'Both are the same sex: pick a male and a female.';
    case 'not_mature':
      return 'One of them is too young to breed yet.';
    case 'no_slots_left':
      return 'No directed-mating slots left this season; they come back when the next season starts.';
    case 'not_owned':
      return 'That organism is not one of yours.';
    case 'died':
      // The server says how ("organism 12 drowned before the mating"): keep it.
      return e.message ? `${capital(e.message.replace(/^organism (\d+)/, '#$1'))}.` : 'That organism is no longer alive.';
    case 'not_found':
      return 'That organism is no longer alive (or out of sight).';
    case 'no_consent':
      return 'The other player has not agreed yet.';
    case 'possessed_by_other':
      return 'Another player is controlling that organism right now.';
    case 'incompatible_body_plans':
      return 'Their body plans are too different: they cannot conceive.';
    case 'too_many_offers':
      return 'You have too many open offers; wait for some to close or cancel one.';
    case 'insufficient_funds':
      return 'Not enough renown for that price.';
    case 'offer_closed':
    case 'unknown_offer':
      return 'That offer has already closed.';
    case 'organism_busy':
    case 'busy':
      return 'That organism is already promised to another mating or auction.';
    case 'own_organism':
      return 'Both organisms are yours: mate them directly (no offer needed).';
    default:
      return capital(e.message.replace(/\s*\([a-z_]+\)\s*$/, ''));
  }
}
