// Where to play: asked before logging in, so the server can spread players
// over its worlds and keep each with their lineage (`GET /api/assign`), and
// so a front door over several servers (`evo-lobby`, `GET /lobby/assign`)
// can send a player to another server. Sources, first match wins:
//   1. a lobby: `?lobby=https://host:port` in the page URL (remembered), or
//      the build-time default `VITE_EVO_LOBBY` (e.g. for a store build);
//   2. none: the game server's own `/api/assign`.
// Anything that goes wrong (an older server without it, a lobby that is
// down, a slow answer) yields no assignment: the client then does what it
// did before (the remembered or picked world on the server it was given).

import { normalizeOrigin } from './serverOrigin';

const LOBBY_KEY = 'evo.lobby';

/** Why the server picked the world (`worlds::AssignReason`). */
export type AssignReason = 'lineage' | 'fill' | 'spread';

export interface Assignment {
  /** What `?world=` takes. */
  world: string;
  reason: AssignReason;
  /** From a lobby: the server to log in to and play on (an origin). */
  server?: string;
}

/** An assignment from an `/api/assign` or `/lobby/assign` answer, or null if it isn't one. */
export function parseAssignment(v: unknown): Assignment | null {
  if (typeof v !== 'object' || v === null) return null;
  const o = v as Record<string, unknown>;
  if (typeof o.world !== 'string' || !o.world) return null;
  const reason = o.reason === 'lineage' || o.reason === 'fill' || o.reason === 'spread' ? o.reason : null;
  if (!reason) return null;
  const a: Assignment = { world: o.world, reason };
  if (o.server !== undefined) {
    const s = typeof o.server === 'string' ? normalizeOrigin(o.server) : null;
    if (!s) return null;
    a.server = s;
  }
  return a;
}

/**
 * The world to play: the one the player picked in the world list, else the
 * one their lineage lives in, else the one remembered for them, else the
 * one the server would fill, else none (the server's default).
 */
export function chooseWorld(o: { picked: string | null; assigned: Assignment | null; remembered: string | null }): string | null {
  if (o.picked) return o.picked;
  if (o.assigned?.reason === 'lineage') return o.assigned.world;
  return o.remembered ?? o.assigned?.world ?? null;
}

/** The lobby to ask, if any: the page URL's `?lobby=`, else the saved one, else the build default. */
export function lobbyOrigin(src: { query: string | null; saved: string | null; buildDefault: string | null | undefined }): { origin: string | null; save: boolean } {
  for (const [v, save] of [[src.query, true], [src.saved, false], [src.buildDefault ?? null, false]] as const) {
    if (v === null || v === '') continue;
    const o = normalizeOrigin(v);
    if (o) return { origin: o, save };
  }
  return { origin: null, save: false };
}

/** Resolve the lobby from the page URL, storage and build default (remembering a `?lobby=`). */
export function initialLobby(): string | null {
  let saved: string | null = null;
  try {
    saved = localStorage.getItem(LOBBY_KEY);
  } catch {
    /* storage unavailable */
  }
  const query = typeof window === 'undefined' ? null : new URLSearchParams(window.location.search).get('lobby');
  const { origin, save } = lobbyOrigin({ query, saved, buildDefault: import.meta.env.VITE_EVO_LOBBY as string | undefined });
  if (save && origin) {
    try {
      localStorage.setItem(LOBBY_KEY, origin);
    } catch {
      /* lasts for this page only */
    }
  }
  return origin;
}

/**
 * Ask where `name` should play: the lobby's `/lobby/assign` when there is
 * one, else `server`'s `/api/assign` (`server` '' is the page's own
 * origin). Null on any failure or after `timeoutMs`.
 */
export async function askAssignment(o: {
  name: string;
  server: string;
  lobby: string | null;
  timeoutMs?: number;
  fetchFn?: typeof fetch;
}): Promise<Assignment | null> {
  const f = o.fetchFn ?? fetch;
  const q = `name=${encodeURIComponent(o.name.trim())}`;
  const url = o.lobby ? `${o.lobby}/lobby/assign?${q}` : `${o.server}/api/assign?${q}`;
  const ctl = typeof AbortController === 'undefined' ? null : new AbortController();
  const timer = setTimeout(() => ctl?.abort(), o.timeoutMs ?? 2500);
  try {
    const res = await f(url, { signal: ctl?.signal });
    if (!res.ok) return null;
    const a = parseAssignment(await res.json());
    // Only a lobby names the server; a game server's own answer is for itself.
    if (a && !o.lobby) delete a.server;
    return a;
  } catch {
    return null;
  } finally {
    clearTimeout(timer);
  }
}
