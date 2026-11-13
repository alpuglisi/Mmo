// Which world a player plays in, remembered per user (and per server) in
// local storage, and the world picker's choice from the server's list.

import type { WorldList, WorldListing } from './protocol';

const KEY = 'evo.worlds';

/** What is remembered: the last world chosen, and each user's own. */
export interface WorldMemory {
  last: Record<string, string>;
  byUser: Record<string, string>;
}

function emptyMemory(): WorldMemory {
  return { last: {}, byUser: {} };
}

function userKey(origin: string, name: string): string {
  return `${origin}|${name.trim().toLowerCase()}`;
}

/** Parse what storage holds; anything unexpected reads as nothing remembered. */
export function parseMemory(raw: string | null): WorldMemory {
  if (!raw) return emptyMemory();
  try {
    const m = JSON.parse(raw) as Partial<WorldMemory>;
    const ok = (r: unknown): r is Record<string, string> =>
      typeof r === 'object' && r !== null && Object.values(r).every((v) => typeof v === 'string');
    return { last: ok(m.last) ? m.last : {}, byUser: ok(m.byUser) ? m.byUser : {} };
  } catch {
    return emptyMemory();
  }
}

/** The world remembered for `name` on `origin` ('' = the page's own), else the last chosen there. */
export function remembered(mem: WorldMemory, origin: string, name: string): string | null {
  return (name.trim() ? mem.byUser[userKey(origin, name)] : undefined) ?? mem.last[origin] ?? null;
}

/** `mem` with `world` remembered for `name` on `origin` and as the last chosen there. */
export function remember(mem: WorldMemory, origin: string, name: string, world: string): WorldMemory {
  const next: WorldMemory = { last: { ...mem.last, [origin]: world }, byUser: { ...mem.byUser } };
  if (name.trim()) next.byUser[userKey(origin, name)] = world;
  return next;
}

/** The world to preselect: the wanted one if the server has it, else its default. */
export function pickWorld(list: WorldList, wanted: string | null): string {
  if (wanted && list.worlds.some((w) => w.id === wanted)) return wanted;
  return list.default;
}

/** One line about a world for the picker. */
export function describeWorld(w: WorldListing): string {
  const online = w.players_online === 1 ? '1 player online' : `${w.players_online} players online`;
  return `${w.width}×${w.height}, day ${w.day} (${w.season}), ${w.population.toLocaleString('en')} organisms, ${online}`;
}

export function loadMemory(): WorldMemory {
  try {
    return parseMemory(localStorage.getItem(KEY));
  } catch {
    return emptyMemory();
  }
}

export function saveMemory(mem: WorldMemory): void {
  try {
    localStorage.setItem(KEY, JSON.stringify(mem));
  } catch {
    /* storage unavailable: the choice lasts for this page only */
  }
}
