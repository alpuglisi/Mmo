// Friendly text for failed commands: what a player reads in a toast instead
// of the server's code, its raw wording or a command's wire name.
import type { ErrorInfo } from './protocol';

/** What each command is called in the interface. */
const COMMANDS: Record<string, string> = {
  possess: 'Possess', order_move: 'Move', possess_attack: 'Attack', release: 'Release',
  request_mating: 'Mating request', preview_mating: 'Mating preview', respond_mating: 'Answer',
  complete_mating: 'Mating', cancel_mating: 'Cancel offer',
  migrate: 'Migrate', clear_migration: 'Stop migrating', claim_territory: 'Territory', abandon_territory: 'Territory',
  rally: 'Rally', cancel_rally: 'Rally', split_group: 'Split group', merge_groups: 'Merge groups',
  set_view: 'View', clear_view: 'View',
  create_listing: 'Listing', withdraw_listing: 'Listing', buy_listing: 'Buy', create_auction: 'Auction', bid: 'Bid',
  browse_market: 'Market', ledger: 'Ledger', mating_prefs: 'Mating requests',
  retire_lineage: 'Retire lineage', renown_shop: 'Renown shop', buy_mating_slot: 'Buy mating slot',
  reinforce: 'Reinforcements', end_grace: 'End grace', compatibility: 'Who can breed', resync: 'Sync', watch_season: 'Season',
  offer_to_lineage: 'Offer to a lineage', set_nameplate: 'Nameplate', name_species: 'Name species', nameplates: 'Nameplates', provision: 'Provision', restore_pasture: 'Restore pasture', substitute_mating: 'Send another animal', survey: 'Survey', name_animal: 'Name an animal', animal_names: 'Animal names',
};

/** Codes whose own wording is replaced (the rest keep the server's message). */
const FRIENDLY: Record<string, string> = {
  bad_message: "The server didn't understand that. Reloading the page may help.",
  rate_limited: 'Too many actions at once: wait a moment and try again.',
  busy: 'The world is busy: try again in a moment.',
  timeout: 'No reply from the server: check your connection.',
  not_connected: 'Not connected to the server.',
  disconnected: 'The connection dropped.',
  closed: 'The connection is closed.',
  out_of_sight: 'You can only look where your animals are.',
  partner_unavailable: "That animal can't mate right now.",
  requests_refused: "That player isn't taking mating requests from you.",
  too_many_requests: "You've sent that player several requests recently: try again later.",
};

/** The interface's name for a command (its wire name only as a last resort). */
export function commandLabel(command: string | undefined): string {
  if (!command || command === 'message') return 'Command';
  return COMMANDS[command] ?? 'Command';
}

/** Server text that reads like a developer's: a parser's message, names in backticks. */
function technical(message: string): boolean {
  return /`|expected one of|unknown variant|missing field|invalid type|at line \d/.test(message);
}

/** A failed command as a player should read it: "Label: what went wrong." */
export function commandError(command: string | undefined, error: ErrorInfo): string {
  const text = FRIENDLY[error.code]
    ?? (error.message && !technical(error.message)
      ? error.message.charAt(0).toUpperCase() + error.message.slice(1)
      : "That didn't work.");
  return `${commandLabel(command)}: ${text}`;
}
