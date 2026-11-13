// Push notifications (Web Push): whether this browser can have them, the
// per-kind switches (stored on the server per account, `/api/push/prefs`),
// and subscribing or unsubscribing this device. The service worker
// (`sw.ts`) shows what arrives; the server decides what is worth a push and
// skips players who are in the game.
//
// The browser plumbing goes through `PushDeps`, so the flows are tested with
// fakes (`push.test.ts`).

import { PUSH_KINDS, type PushKind, type PushPrefs, type PushSubscriptionView } from './protocol';

/** What this device can do about push, in the order the settings explain it. */
export type PushStatus =
  /** No service worker, Push API or notifications here (or a native app shell). */
  | 'unsupported'
  /** Push needs https (or localhost). */
  | 'insecure'
  /** The server has push notifications off. */
  | 'disabled'
  /** The user blocked notifications for this site. */
  | 'denied'
  /** Possible, not subscribed on this device. */
  | 'off'
  /** This device is subscribed. */
  | 'on';

/** The browser features push needs. */
export interface PushEnv {
  secure: boolean;
  serviceWorker: boolean;
  pushManager: boolean;
  notification: boolean;
  permission: NotificationPermission | null;
  /** A Capacitor native shell: its WebView has no Web Push (native push would need FCM/APNs). */
  native: boolean;
}

export function detectEnv(w: Window & typeof globalThis = window): PushEnv {
  const cap = (w as unknown as { Capacitor?: { isNativePlatform?: () => boolean } }).Capacitor;
  const notification = typeof w.Notification !== 'undefined';
  return {
    secure: w.isSecureContext === true,
    serviceWorker: 'serviceWorker' in w.navigator,
    pushManager: 'PushManager' in w,
    notification,
    permission: notification ? w.Notification.permission : null,
    native: cap?.isNativePlatform?.() === true,
  };
}

/** The device's push status. `serverEnabled`: null while unknown (treated as on). */
export function pushStatus(env: PushEnv, serverEnabled: boolean | null, subscribed: boolean): PushStatus {
  if (env.native || !env.serviceWorker || !env.pushManager || !env.notification) return env.secure || env.native ? 'unsupported' : 'insecure';
  if (!env.secure) return 'insecure';
  if (serverEnabled === false) return 'disabled';
  if (env.permission === 'denied') return 'denied';
  return subscribed && env.permission === 'granted' ? 'on' : 'off';
}

export function statusText(s: PushStatus): string {
  switch (s) {
    case 'unsupported': return 'This browser or app can’t receive push notifications. Alerts still show in the game.';
    case 'insecure': return 'Push notifications need a secure (https) connection to the game.';
    case 'disabled': return 'Push notifications are turned off on this server.';
    case 'denied': return 'Notifications are blocked for this site. Allow them in the browser’s site settings, then come back.';
    case 'off': return 'Get a notification when something important happens while the game is closed.';
    case 'on': return 'This device gets push notifications while the game is closed (at most a few an hour).';
  }
}

export const PUSH_LABEL: Record<PushKind, string> = {
  mating: 'Mating offers', death: 'Deaths', market: 'Market', species: 'New species', season: 'Season results', catastrophe: 'Catastrophes',
};

export const PUSH_HINT: Record<PushKind, string> = {
  mating: 'another player asks to mate with one of yours',
  death: 'a founder or a possessed organism dies',
  market: 'outbid, an auction won or sold, a stud fee paid',
  species: 'a new species arises from your lineage',
  season: 'a ranked season or an era ends, with your places',
  catastrophe: 'a catastrophe begins',
};

/** Known kinds only, in the server's order, without duplicates. */
export function normalizePushPrefs(p: Partial<PushPrefs> | null | undefined): PushPrefs {
  const known = new Set<string>(PUSH_KINDS);
  const off = new Set((p?.off ?? []).filter((k): k is PushKind => typeof k === 'string' && known.has(k)));
  return { off: PUSH_KINDS.filter((k) => off.has(k)) };
}

export function wants(p: PushPrefs, kind: PushKind): boolean {
  return !p.off.includes(kind);
}

export function setPushKind(p: PushPrefs, kind: PushKind, on: boolean): PushPrefs {
  return normalizePushPrefs({ off: on ? p.off.filter((k) => k !== kind) : [...p.off, kind] });
}

/** The VAPID public key (base64url) as the bytes `applicationServerKey` takes. */
export function urlBase64ToUint8Array(s: string): Uint8Array<ArrayBuffer> {
  const b64 = s.replace(/-/g, '+').replace(/_/g, '/') + '='.repeat((4 - (s.length % 4)) % 4);
  const raw = atob(b64);
  const out = new Uint8Array(new ArrayBuffer(raw.length));
  for (let i = 0; i < raw.length; i++) out[i] = raw.charCodeAt(i);
  return out;
}

/** Whether a subscription was made with this server key (a server that changed its key needs a new one). */
export function sameKey(applicationServerKey: ArrayBuffer | null | undefined, publicKey: string): boolean {
  if (!applicationServerKey) return true; // not exposed: assume it is
  const a = new Uint8Array(applicationServerKey);
  const b = urlBase64ToUint8Array(publicKey);
  return a.length === b.length && a.every((x, i) => x === b[i]);
}

/** A short name for this device from its user agent ("Firefox on Android"). */
export function deviceLabel(ua: string): string {
  const browser = /Edg\//.test(ua) ? 'Edge' : /Firefox\//.test(ua) ? 'Firefox' : /Chrome\//.test(ua) ? 'Chrome' : /Safari\//.test(ua) ? 'Safari' : 'Browser';
  const os = /Android/.test(ua) ? 'Android' : /iPhone|iPad|iPod/.test(ua) ? 'iOS' : /Mac OS X/.test(ua) ? 'macOS' : /Windows/.test(ua) ? 'Windows' : /Linux/.test(ua) ? 'Linux' : '';
  return os ? `${browser} on ${os}` : browser;
}

/** What `enable` / `disable` need from the browser and the server. */
export interface PushDeps {
  /** The service worker registration (`navigator.serviceWorker.ready`), null without one. */
  registration: () => Promise<Pick<ServiceWorkerRegistration, 'pushManager'> | null>;
  requestPermission: () => Promise<NotificationPermission>;
  subscribe: (sub: PushSubscriptionJSON, label: string) => Promise<PushSubscriptionView>;
  unsubscribe: (endpoint: string) => Promise<unknown>;
  label: string;
}

export class PushError extends Error {
  constructor(public code: 'unsupported' | 'denied' | 'server', message: string) {
    super(message);
  }
}

/** This device's subscription, if any. */
export async function currentSubscription(deps: Pick<PushDeps, 'registration'>): Promise<PushSubscription | null> {
  const reg = await deps.registration();
  return reg ? reg.pushManager.getSubscription() : null;
}

/**
 * Subscribe this device: ask for permission, subscribe with the server's
 * key (replacing a subscription made with another key) and hand the
 * subscription to the server. If the server refuses it, the browser
 * subscription is dropped again so the two agree.
 */
export async function enablePush(deps: PushDeps, publicKey: string): Promise<PushSubscription> {
  const reg = await deps.registration();
  if (!reg) throw new PushError('unsupported', 'no service worker');
  const permission = await deps.requestPermission();
  if (permission !== 'granted') throw new PushError('denied', permission === 'denied' ? 'notifications are blocked' : 'permission not given');
  let sub = await reg.pushManager.getSubscription();
  if (sub && !sameKey(sub.options?.applicationServerKey, publicKey)) {
    await sub.unsubscribe();
    sub = null;
  }
  sub ??= await reg.pushManager.subscribe({ userVisibleOnly: true, applicationServerKey: urlBase64ToUint8Array(publicKey) });
  try {
    await deps.subscribe(sub.toJSON(), deps.label);
  } catch (e) {
    await sub.unsubscribe().catch(() => false);
    throw new PushError('server', (e as Error).message);
  }
  return sub;
}

/** Unsubscribe this device, in the browser and on the server (either may already be gone). */
export async function disablePush(deps: PushDeps): Promise<void> {
  const sub = await currentSubscription(deps);
  if (!sub) return;
  const endpoint = sub.endpoint;
  await sub.unsubscribe().catch(() => false);
  await deps.unsubscribe(endpoint);
}

/** Register the service worker (`sw.js`, built from `sw.ts`) where push can work. */
export async function registerServiceWorker(env: PushEnv = detectEnv()): Promise<ServiceWorkerRegistration | null> {
  // The dev server has no built sw.js; the native shells have no Web Push.
  if (!env.serviceWorker || !env.secure || env.native || import.meta.env.DEV) return null;
  try {
    return await navigator.serviceWorker.register('./sw.js', { scope: './' });
  } catch {
    return null;
  }
}

/** Message the service worker posts to an open game window on a notification click. */
export interface OpenMessage {
  type: 'evo-open';
  world: string;
  tab: string;
}

export function isOpenMessage(m: unknown): m is OpenMessage {
  const o = m as Partial<OpenMessage> | null;
  return !!o && o.type === 'evo-open' && typeof o.world === 'string' && typeof o.tab === 'string';
}

/** `?world=…&tab=…` on the page URL (a notification click opening a new window), and the URL without them. */
export function openParams(href: string): { world: string | null; tab: string | null; clean: string } {
  const u = new URL(href);
  const world = u.searchParams.get('world');
  const tab = u.searchParams.get('tab');
  u.searchParams.delete('world');
  u.searchParams.delete('tab');
  return { world, tab, clean: u.toString() };
}
