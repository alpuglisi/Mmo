import { describe, expect, it, vi } from 'vitest';
import {
  PushError, deviceLabel, disablePush, enablePush, isOpenMessage, normalizePushPrefs, openParams, pushStatus, sameKey, setPushKind, statusText,
  urlBase64ToUint8Array, wants, type PushDeps, type PushEnv,
} from './push';
import type { PushKind } from './protocol';

const env = (over: Partial<PushEnv> = {}): PushEnv => ({
  secure: true, serviceWorker: true, pushManager: true, notification: true, permission: 'default', native: false, ...over,
});

// A 65-byte "public key" (base64url of 0x04 then 64 bytes of 1..64).
const keyBytes = new Uint8Array([4, ...Array.from({ length: 64 }, (_, i) => i + 1)]);
const KEY = Buffer.from(keyBytes).toString('base64url');

/** A fake PushSubscription. */
function fakeSub(endpoint: string, key: Uint8Array | null = keyBytes) {
  return {
    endpoint,
    options: { applicationServerKey: key ? key.buffer.slice(0) : null, userVisibleOnly: true },
    toJSON: () => ({ endpoint, keys: { p256dh: 'BPK', auth: 'AUTH' } }),
    unsubscribe: vi.fn(async () => true),
  };
}

/** Fake browser and server for enable/disable. */
function fakes(opts: { existing?: ReturnType<typeof fakeSub> | null; permission?: NotificationPermission; serverFails?: boolean } = {}) {
  let current: ReturnType<typeof fakeSub> | null = opts.existing ?? null;
  const subscribeBrowser = vi.fn(async (o: PushSubscriptionOptionsInit) => {
    current = fakeSub('https://push.example/new', new Uint8Array(o.applicationServerKey as Uint8Array));
    return current;
  });
  const pushManager = { getSubscription: vi.fn(async () => current), subscribe: subscribeBrowser };
  const server = { subscribe: vi.fn(), unsubscribe: vi.fn() };
  const deps: PushDeps = {
    registration: async () => ({ pushManager }) as unknown as ServiceWorkerRegistration,
    requestPermission: vi.fn(async () => opts.permission ?? 'granted'),
    subscribe: async (sub, label) => {
      server.subscribe(sub, label);
      if (opts.serverFails) throw new Error('bad_subscription');
      return { endpoint: sub.endpoint!, created_unix: 1 };
    },
    unsubscribe: async (endpoint) => server.unsubscribe(endpoint),
    label: 'Chrome on Linux',
  };
  return { deps, pushManager, server, subscribeBrowser, current: () => current };
}

describe('push status', () => {
  it('explains what the device can do, most basic problem first', () => {
    expect(pushStatus(env({ native: true }), true, false)).toBe('unsupported');
    expect(pushStatus(env({ pushManager: false }), true, false)).toBe('unsupported');
    expect(pushStatus(env({ serviceWorker: false, secure: false }), true, false)).toBe('insecure');
    expect(pushStatus(env({ secure: false }), true, false)).toBe('insecure');
    expect(pushStatus(env(), false, false)).toBe('disabled');
    expect(pushStatus(env({ permission: 'denied' }), true, true)).toBe('denied');
    expect(pushStatus(env(), null, false)).toBe('off');
    expect(pushStatus(env({ permission: 'granted' }), true, false)).toBe('off');
    expect(pushStatus(env({ permission: 'granted' }), true, true)).toBe('on');
    // A subscription without permission (revoked since) is not "on".
    expect(pushStatus(env({ permission: 'default' }), true, true)).toBe('off');
    for (const s of ['unsupported', 'insecure', 'disabled', 'denied', 'off', 'on'] as const) expect(statusText(s).length).toBeGreaterThan(10);
  });
});

describe('push preferences', () => {
  it('normalizes like the server and switches kinds', () => {
    expect(normalizePushPrefs({ off: ['season', 'bogus' as PushKind, 'mating', 'season'] })).toEqual({ off: ['mating', 'season'] });
    expect(normalizePushPrefs(null)).toEqual({ off: [] });
    let p = normalizePushPrefs({});
    expect(wants(p, 'death')).toBe(true);
    p = setPushKind(p, 'death', false);
    p = setPushKind(p, 'catastrophe', false);
    expect(p).toEqual({ off: ['death', 'catastrophe'] });
    expect(wants(p, 'death')).toBe(false);
    expect(setPushKind(p, 'death', true)).toEqual({ off: ['catastrophe'] });
  });
});

describe('keys and labels', () => {
  it('decodes the VAPID key and compares it with a subscription', () => {
    expect(Array.from(urlBase64ToUint8Array(KEY))).toEqual(Array.from(keyBytes));
    expect(Array.from(urlBase64ToUint8Array('-_8'))).toEqual([0xfb, 0xff]);
    expect(sameKey(keyBytes.buffer.slice(0), KEY)).toBe(true);
    expect(sameKey(new Uint8Array(65).buffer, KEY)).toBe(false);
    expect(sameKey(null, KEY)).toBe(true);
  });

  it('names devices from their user agent', () => {
    expect(deviceLabel('Mozilla/5.0 (Linux; Android 14) AppleWebKit/537.36 Chrome/124.0 Mobile Safari/537.36')).toBe('Chrome on Android');
    expect(deviceLabel('Mozilla/5.0 (X11; Linux x86_64; rv:126.0) Gecko/20100101 Firefox/126.0')).toBe('Firefox on Linux');
    expect(deviceLabel('Mozilla/5.0 (iPhone; CPU iPhone OS 17_4 like Mac OS X) AppleWebKit/605.1.15 Version/17.4 Mobile/15E148 Safari/604.1')).toBe('Safari on iOS');
    expect(deviceLabel('Mozilla/5.0 (Windows NT 10.0) AppleWebKit/537.36 Chrome/124.0 Safari/537.36 Edg/124.0')).toBe('Edge on Windows');
    expect(deviceLabel('curl/8')).toBe('Browser');
  });
});

describe('enable and disable', () => {
  it('asks permission, subscribes with the server key and tells the server', async () => {
    const f = fakes();
    const sub = await enablePush(f.deps, KEY);
    expect(f.deps.requestPermission).toHaveBeenCalledOnce();
    const opts = f.subscribeBrowser.mock.calls[0][0];
    expect(opts.userVisibleOnly).toBe(true);
    expect(Array.from(opts.applicationServerKey as Uint8Array)).toEqual(Array.from(keyBytes));
    expect(f.server.subscribe).toHaveBeenCalledWith({ endpoint: 'https://push.example/new', keys: { p256dh: 'BPK', auth: 'AUTH' } }, 'Chrome on Linux');
    expect(sub.endpoint).toBe('https://push.example/new');
  });

  it('reuses a subscription made with the same key and replaces one made with another', async () => {
    const same = fakeSub('https://push.example/same');
    let f = fakes({ existing: same });
    await enablePush(f.deps, KEY);
    expect(f.subscribeBrowser).not.toHaveBeenCalled();
    expect(f.server.subscribe.mock.calls[0][0].endpoint).toBe('https://push.example/same');

    const stale = fakeSub('https://push.example/stale', new Uint8Array(65));
    f = fakes({ existing: stale });
    await enablePush(f.deps, KEY);
    expect(stale.unsubscribe).toHaveBeenCalledOnce();
    expect(f.subscribeBrowser).toHaveBeenCalledOnce();
    expect(f.server.subscribe.mock.calls[0][0].endpoint).toBe('https://push.example/new');
  });

  it('stops when permission is refused, and undoes the browser subscription when the server refuses', async () => {
    const f = fakes({ permission: 'denied' });
    await expect(enablePush(f.deps, KEY)).rejects.toMatchObject({ code: 'denied' });
    expect(f.subscribeBrowser).not.toHaveBeenCalled();
    expect(f.server.subscribe).not.toHaveBeenCalled();

    const g = fakes({ serverFails: true });
    const err = await enablePush(g.deps, KEY).catch((e) => e);
    expect(err).toBeInstanceOf(PushError);
    expect(err.code).toBe('server');
    expect(g.current()!.unsubscribe).toHaveBeenCalledOnce();

    const none: PushDeps = { ...f.deps, registration: async () => null };
    await expect(enablePush(none, KEY)).rejects.toMatchObject({ code: 'unsupported' });
  });

  it('disables in the browser and on the server', async () => {
    const sub = fakeSub('https://push.example/x');
    const f = fakes({ existing: sub });
    await disablePush(f.deps);
    expect(sub.unsubscribe).toHaveBeenCalledOnce();
    expect(f.server.unsubscribe).toHaveBeenCalledWith('https://push.example/x');
    // Nothing subscribed: nothing to do.
    const g = fakes();
    await disablePush(g.deps);
    expect(g.server.unsubscribe).not.toHaveBeenCalled();
  });
});

describe('notification clicks', () => {
  it('reads the world and tab a new window was opened with', () => {
    const o = openParams('https://game.example/play/?server=x&world=isle&tab=market#top');
    expect(o).toEqual({ world: 'isle', tab: 'market', clean: 'https://game.example/play/?server=x#top' });
    expect(openParams('https://game.example/')).toEqual({ world: null, tab: null, clean: 'https://game.example/' });
  });

  it('recognizes the service worker message', () => {
    expect(isOpenMessage({ type: 'evo-open', world: 'main', tab: 'matings' })).toBe(true);
    expect(isOpenMessage({ type: 'evo-open', world: 'main' })).toBe(false);
    expect(isOpenMessage(null)).toBe(false);
  });
});
