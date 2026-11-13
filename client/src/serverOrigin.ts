// Which game server the client talks to. Served by evo-server (or the Vite
// dev proxy) the API is on the page's own origin; a Capacitor app's page
// lives on `capacitor://localhost` / `https://localhost`, so it needs a
// remote origin. Sources, first match wins:
//   1. `?server=https://host:port` in the page URL (remembered);
//   2. the origin saved from the login screen's Server field;
//   3. the build-time default `VITE_EVO_SERVER` (e.g. for a store build);
//   4. none: the page's own origin.

const KEY = 'evo.server';

export interface OriginSources {
  query: string | null;
  saved: string | null;
  buildDefault: string | null | undefined;
}

/**
 * A server origin from user input: `host:port` gets `https://`, a path or
 * trailing slash is dropped; empty input means "same origin" (''). Null when
 * it isn't an http(s) URL.
 */
export function normalizeOrigin(input: string): string | null {
  const t = input.trim();
  if (!t) return '';
  const withScheme = /^[a-z][a-z0-9+.-]*:\/\//i.test(t) ? t : `https://${t}`;
  try {
    const u = new URL(withScheme);
    if (u.protocol !== 'http:' && u.protocol !== 'https:') return null;
    if (!u.hostname) return null;
    return u.origin;
  } catch {
    return null;
  }
}

/** The origin to use (or '' for the page's own) and whether to remember it. */
export function pickOrigin(src: OriginSources): { origin: string; save: boolean } {
  if (src.query !== null) {
    const q = normalizeOrigin(src.query);
    if (q !== null) return { origin: q, save: true };
  }
  if (src.saved) {
    const s = normalizeOrigin(src.saved);
    if (s !== null) return { origin: s, save: false };
  }
  const d = src.buildDefault ? normalizeOrigin(src.buildDefault) : null;
  return { origin: d ?? '', save: false };
}

/** Running inside a Capacitor native shell (the page is not served by the game server). */
export function isNativeShell(): boolean {
  const cap = (window as unknown as { Capacitor?: { isNativePlatform?: () => boolean } }).Capacitor;
  return Boolean(cap?.isNativePlatform?.());
}

export function loadSavedOrigin(): string | null {
  try {
    return localStorage.getItem(KEY);
  } catch {
    return null;
  }
}

/** Remember an origin ('' forgets it: back to the page's own origin). */
export function saveOrigin(origin: string): void {
  try {
    if (origin) localStorage.setItem(KEY, origin);
    else localStorage.removeItem(KEY);
  } catch {
    /* storage unavailable: lasts for this page only */
  }
}

/** Resolve the origin from the page URL, storage and build default. */
export function initialOrigin(): string {
  const query = new URLSearchParams(window.location.search).get('server');
  const { origin, save } = pickOrigin({ query, saved: loadSavedOrigin(), buildDefault: import.meta.env.VITE_EVO_SERVER as string | undefined });
  if (save) saveOrigin(origin);
  return origin;
}
