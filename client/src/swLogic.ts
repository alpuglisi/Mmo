// What the service worker (`sw.ts`) decides, kept free of worker APIs so it
// is unit-tested (`swLogic.test.ts`). Only `sw.ts` imports this module, so
// the build inlines it into `sw.js`.

import type { PushPayload } from './protocol';

const KINDS = new Set(['mating', 'death', 'market', 'species', 'season', 'catastrophe']);

/** What a notification carries for its click. */
export interface ClickData {
  world: string;
  tab: string;
}

/** A push message's payload, or null for anything else (validated: it came over the network). */
export function parsePayload(data: unknown): PushPayload | null {
  const p = data as Partial<PushPayload> | null;
  if (!p || typeof p !== 'object') return null;
  if (typeof p.title !== 'string' || typeof p.body !== 'string' || typeof p.world !== 'string' || typeof p.tab !== 'string') return null;
  if (typeof p.kind !== 'string' || !KINDS.has(p.kind)) return null;
  return {
    title: p.title.slice(0, 200),
    body: p.body.slice(0, 2000),
    world: p.world,
    tab: p.tab,
    kind: p.kind,
    count: typeof p.count === 'number' && p.count > 0 ? p.count : 1,
    tag: typeof p.tag === 'string' && p.tag ? p.tag : `evo-${p.world}`,
    organism: typeof p.organism === 'number' ? p.organism : undefined,
  };
}

/** The notification to show for a payload (a payload that doesn't parse still shows a generic one: a push must). */
export function notificationFor(p: PushPayload | null): { title: string; options: NotificationOptions & { data: ClickData | null } } {
  if (!p) {
    return { title: 'Evolution MMO', options: { body: 'Something happened in your world.', tag: 'evo', data: null } };
  }
  return {
    title: p.title,
    options: {
      body: p.body,
      // One notification per world: a newer one replaces it (and alerts again).
      tag: p.tag,
      renotify: true,
      icon: 'icon.svg',
      badge: 'icon.svg',
      data: { world: p.world, tab: p.tab },
    } as NotificationOptions & { data: ClickData },
  };
}

/** The game's URL for a click: the scope with `?world=…&tab=…`. */
export function targetUrl(scope: string, data: ClickData | null): string {
  const u = new URL(scope);
  if (data?.world) u.searchParams.set('world', data.world);
  if (data?.tab) u.searchParams.set('tab', data.tab);
  return u.toString();
}

export interface ClientLike {
  url: string;
  focused?: boolean;
  visibilityState?: string;
}

/** The open game window to reuse for a click: a focused one, else a visible one, else any in scope; -1 for none. */
export function pickClient(clients: readonly ClientLike[], scope: string): number {
  const inScope = clients.map((c, i) => ({ c, i })).filter(({ c }) => c.url.startsWith(scope));
  const best = inScope.find(({ c }) => c.focused) ?? inScope.find(({ c }) => c.visibilityState === 'visible') ?? inScope[0];
  return best ? best.i : -1;
}
