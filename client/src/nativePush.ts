// Native push notifications in the Capacitor apps: the Android app
// registers with Firebase Cloud Messaging, the iOS app with the Apple Push
// Notification service (`@capacitor/push-notifications`), and the device
// token goes to the server (`POST /api/push/native/subscribe`), which sends
// the same notifications as Web Push to it. Browsers keep Web Push
// (`push.ts`); this module is only loaded inside the apps.
//
// The plugin and the server go through `NativePushDeps`, so the flows are
// tested with fakes (`nativePush.test.ts`).

import type { NativeService } from './protocol';
import { PushError, type PushStatus } from './push';

/** The push service of a Capacitor platform (null on the web). */
export function nativeService(platform: string): NativeService | null {
  return platform === 'android' ? 'fcm' : platform === 'ios' ? 'apns' : null;
}

/** Permission as the plugin reports it (Capacitor's `PermissionState`). */
export type NativePermission = 'prompt' | 'prompt-with-rationale' | 'granted' | 'denied';

/** What this device registered, remembered so the settings know it is on and can turn it off. */
export interface SavedDevice {
  service: NativeService;
  token: string;
  /** The server origin and player it was registered for (another login on this phone is not subscribed). */
  account: string;
}

export const SAVED_KEY = 'evo.nativePush';

export interface NativePushDeps {
  service: NativeService;
  /** Whether this build of the app can register (the Android app needs its Firebase config, `google-services.json`). */
  available: boolean;
  checkPermissions: () => Promise<NativePermission>;
  requestPermissions: () => Promise<NativePermission>;
  /** Register with FCM / APNs; resolves the device token. */
  register: () => Promise<string>;
  unregister: () => Promise<void>;
  subscribe: (service: NativeService, token: string, label: string) => Promise<unknown>;
  unsubscribe: (service: NativeService, token: string) => Promise<unknown>;
  label: string;
  /** `${server origin}|${player}`. */
  account: string;
  storage: Pick<Storage, 'getItem' | 'setItem' | 'removeItem'> | null;
}

export function loadSaved(deps: Pick<NativePushDeps, 'storage' | 'account' | 'service'>): SavedDevice | null {
  try {
    const raw = deps.storage?.getItem(SAVED_KEY);
    if (!raw) return null;
    const s = JSON.parse(raw) as Partial<SavedDevice>;
    if (typeof s.token !== 'string' || !s.token || s.service !== deps.service || typeof s.account !== 'string') return null;
    return { service: s.service, token: s.token, account: s.account };
  } catch {
    return null;
  }
}

function save(deps: NativePushDeps, d: SavedDevice | null): void {
  try {
    if (d) deps.storage?.setItem(SAVED_KEY, JSON.stringify(d));
    else deps.storage?.removeItem(SAVED_KEY);
  } catch {
    /* storage unavailable: the settings show "off" after a restart */
  }
}

/** The device's push status in the app. `services`: what the server offers (null while unknown). */
export function nativeStatus(
  available: boolean, service: NativeService, services: readonly NativeService[] | null, serverEnabled: boolean | null,
  permission: NativePermission | null, subscribed: boolean,
): PushStatus {
  if (!available) return 'unavailable';
  if (serverEnabled === false || (services !== null && !services.includes(service))) return 'disabled';
  if (permission === 'denied') return 'denied';
  return subscribed && permission === 'granted' ? 'on' : 'off';
}

/**
 * Turn push on for this device: ask for permission, register with the
 * platform's push service and hand the token to the server. If the server
 * refuses it, the registration is dropped again so the two agree.
 */
export async function enableNative(deps: NativePushDeps): Promise<string> {
  let permission = await deps.checkPermissions();
  if (permission !== 'granted' && permission !== 'denied') permission = await deps.requestPermissions();
  if (permission !== 'granted') throw new PushError('denied', permission === 'denied' ? 'notifications are blocked' : 'permission not given');
  const token = await deps.register();
  const old = loadSaved(deps);
  try {
    await deps.subscribe(deps.service, token, deps.label);
  } catch (e) {
    await deps.unregister().catch(() => undefined);
    throw new PushError('server', (e as Error).message);
  }
  // A token registered before (another login, or rotated since) is let go.
  if (old && (old.token !== token || old.account !== deps.account)) await deps.unsubscribe(old.service, old.token).catch(() => undefined);
  save(deps, { service: deps.service, token, account: deps.account });
  return token;
}

/** Turn push off for this device, in the app and on the server (either may already be gone). */
export async function disableNative(deps: NativePushDeps): Promise<void> {
  const saved = loadSaved(deps);
  await deps.unregister().catch(() => undefined);
  save(deps, null);
  if (saved && saved.account === deps.account) await deps.unsubscribe(saved.service, saved.token);
}

/**
 * On opening the settings: whether this device is subscribed for this
 * account. If it is, register again and hand the token over again: the
 * server may have dropped it, or the platform rotated it.
 */
export async function refreshNative(deps: NativePushDeps): Promise<boolean> {
  const saved = loadSaved(deps);
  if (!saved || saved.account !== deps.account) return false;
  if ((await deps.checkPermissions()) !== 'granted') return false;
  const token = await deps.register();
  await deps.subscribe(deps.service, token, deps.label);
  if (token !== saved.token) {
    await deps.unsubscribe(saved.service, saved.token).catch(() => undefined);
    save(deps, { ...saved, token });
  }
  return true;
}

/** Where a tapped notification leads: its world and tab (the data the server puts in every native message). */
export function openTarget(data: unknown): { world: string; tab: string } | null {
  const d = data as { world?: unknown; tab?: unknown } | null;
  if (!d || typeof d !== 'object') return null;
  if (typeof d.world !== 'string' || typeof d.tab !== 'string' || !d.tab) return null;
  return { world: d.world, tab: d.tab };
}

/** The part of `@capacitor/push-notifications` used here. */
export interface PushPlugin {
  register: () => Promise<void>;
  unregister: () => Promise<void>;
  checkPermissions: () => Promise<{ receive: NativePermission }>;
  requestPermissions: () => Promise<{ receive: NativePermission }>;
  createChannel?: (c: { id: string; name: string; description?: string; importance?: 1 | 2 | 3 | 4 | 5 }) => Promise<void>;
  addListener: (event: string, fn: (e: never) => void) => Promise<{ remove: () => Promise<void> }>;
}

/** The Android notification channel the server's FCM messages name. */
export const ANDROID_CHANNEL = 'evo';

/** Register and wait for the token (or the error) the plugin reports by event. */
export async function registerToken(plugin: PushPlugin, timeoutMs = 15000): Promise<string> {
  const handles: { remove: () => Promise<void> }[] = [];
  let settle: (r: { token?: string; error?: string }) => void = () => {};
  const result = new Promise<{ token?: string; error?: string }>((resolve) => { settle = resolve; });
  handles.push(await plugin.addListener('registration', ((t: { value: string }) => settle({ token: t.value })) as (e: never) => void));
  handles.push(await plugin.addListener('registrationError', ((e: { error: string }) => settle({ error: e.error })) as (e: never) => void));
  const timer = setTimeout(() => settle({ error: 'no device token from the push service (timed out)' }), timeoutMs);
  try {
    await plugin.register();
    const r = await result;
    if (r.token) return r.token;
    throw new Error(r.error ?? 'registration failed');
  } finally {
    clearTimeout(timer);
    for (const h of handles) void h.remove().catch(() => undefined);
  }
}
