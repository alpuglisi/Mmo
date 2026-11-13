// The login session, remembered across reloads when storage is available.
// The server keeps tokens in memory only, so a server restart invalidates it
// (the socket then fails and the client returns to the login screen).

import type { AuthResponse } from './protocol';

const KEY = 'evo.session';

/** A login, and the world it plays in ('' or absent: the server's default). */
export type Session = AuthResponse & { world?: string };

export function loadSession(): Session | null {
  try {
    const raw = localStorage.getItem(KEY);
    if (!raw) return null;
    const s = JSON.parse(raw) as Session;
    if (s.world !== undefined && typeof s.world !== 'string') s.world = undefined;
    return typeof s.token === 'string' && typeof s.player === 'number' ? s : null;
  } catch {
    return null;
  }
}

export function saveSession(s: Session | null): void {
  try {
    if (s) localStorage.setItem(KEY, JSON.stringify(s));
    else localStorage.removeItem(KEY);
  } catch {
    /* storage unavailable: the session lasts for this page only */
  }
}
