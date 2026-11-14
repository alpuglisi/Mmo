import { describe, expect, it, vi } from 'vitest';
import {
  SAVED_KEY, disableNative, enableNative, loadSaved, nativeService, nativeStatus, openTarget, refreshNative, registerToken,
  type NativePermission, type NativePushDeps, type PushPlugin,
} from './nativePush';
import { PushError } from './push';

function memoryStorage(): Pick<Storage, 'getItem' | 'setItem' | 'removeItem'> & { data: Map<string, string> } {
  const data = new Map<string, string>();
  return {
    data,
    getItem: (k) => data.get(k) ?? null,
    setItem: (k, v) => { data.set(k, v); },
    removeItem: (k) => { data.delete(k); },
  };
}

/** A fake plugin and server for the flows. */
function fakes(opts: { permission?: NativePermission; grant?: NativePermission; tokens?: string[]; serverFails?: boolean; account?: string } = {}) {
  let permission = opts.permission ?? 'prompt';
  const tokens = [...(opts.tokens ?? ['tok-1'])];
  const server = { subscribe: vi.fn(), unsubscribe: vi.fn() };
  const storage = memoryStorage();
  const deps: NativePushDeps = {
    service: 'fcm',
    available: true,
    checkPermissions: vi.fn(async () => permission),
    requestPermissions: vi.fn(async () => { permission = opts.grant ?? 'granted'; return permission; }),
    register: vi.fn(async () => tokens.length > 1 ? tokens.shift()! : tokens[0]),
    unregister: vi.fn(async () => undefined),
    subscribe: async (service, token, label) => {
      server.subscribe(service, token, label);
      if (opts.serverFails) throw new Error('bad_subscription');
      return { service, created_unix: 1 };
    },
    unsubscribe: async (service, token) => server.unsubscribe(service, token),
    label: 'Evolution MMO app on Android',
    account: opts.account ?? 'https://game.example|7',
    storage,
  };
  return { deps, server, storage };
}

describe('native push', () => {
  it('maps platforms to their push service', () => {
    expect(nativeService('android')).toBe('fcm');
    expect(nativeService('ios')).toBe('apns');
    expect(nativeService('web')).toBeNull();
  });

  it('explains what the app can do, most basic problem first', () => {
    expect(nativeStatus(false, 'fcm', ['fcm'], true, 'granted', true)).toBe('unavailable');
    expect(nativeStatus(true, 'fcm', ['apns'], true, 'granted', false)).toBe('disabled');
    expect(nativeStatus(true, 'fcm', [], false, 'granted', false)).toBe('disabled');
    expect(nativeStatus(true, 'apns', ['apns'], true, 'denied', true)).toBe('denied');
    expect(nativeStatus(true, 'fcm', null, null, 'prompt', false)).toBe('off');
    expect(nativeStatus(true, 'fcm', ['fcm'], true, 'granted', false)).toBe('off');
    expect(nativeStatus(true, 'fcm', ['fcm'], true, 'granted', true)).toBe('on');
    expect(nativeStatus(true, 'fcm', ['fcm'], true, 'prompt', true)).toBe('off');
  });

  it('enable asks for permission, registers and hands the token to the server', async () => {
    const { deps, server, storage } = fakes();
    expect(await enableNative(deps)).toBe('tok-1');
    expect(deps.requestPermissions).toHaveBeenCalledOnce();
    expect(server.subscribe).toHaveBeenCalledWith('fcm', 'tok-1', 'Evolution MMO app on Android');
    expect(JSON.parse(storage.data.get(SAVED_KEY)!)).toEqual({ service: 'fcm', token: 'tok-1', account: 'https://game.example|7' });
    expect(loadSaved(deps)?.token).toBe('tok-1');
  });

  it('enable fails without permission, and undoes the registration when the server refuses', async () => {
    const denied = fakes({ grant: 'denied' });
    await expect(enableNative(denied.deps)).rejects.toMatchObject({ code: 'denied' });
    expect(denied.deps.register).not.toHaveBeenCalled();
    const blocked = fakes({ permission: 'denied' });
    await expect(enableNative(blocked.deps)).rejects.toBeInstanceOf(PushError);
    expect(blocked.deps.requestPermissions).not.toHaveBeenCalled();
    const refused = fakes({ permission: 'granted', serverFails: true });
    await expect(enableNative(refused.deps)).rejects.toMatchObject({ code: 'server' });
    expect(refused.deps.unregister).toHaveBeenCalledOnce();
    expect(refused.storage.data.has(SAVED_KEY)).toBe(false);
  });

  it('a device enabled under another account is moved, and disable forgets it on both sides', async () => {
    const { deps, server, storage } = fakes({ permission: 'granted' });
    storage.setItem(SAVED_KEY, JSON.stringify({ service: 'fcm', token: 'old', account: 'https://game.example|3' }));
    // Another login on this phone: not subscribed for this account.
    expect(await refreshNative(deps)).toBe(false);
    await enableNative(deps);
    expect(server.unsubscribe).toHaveBeenCalledWith('fcm', 'old');
    await disableNative(deps);
    expect(deps.unregister).toHaveBeenCalledOnce();
    expect(server.unsubscribe).toHaveBeenLastCalledWith('fcm', 'tok-1');
    expect(storage.data.has(SAVED_KEY)).toBe(false);
  });

  it('refresh hands a rotated token over and lets the old one go', async () => {
    const { deps, server, storage } = fakes({ permission: 'granted', tokens: ['tok-1', 'tok-2'] });
    await enableNative(deps);
    expect(await refreshNative(deps)).toBe(true);
    expect(server.subscribe).toHaveBeenLastCalledWith('fcm', 'tok-2', 'Evolution MMO app on Android');
    expect(server.unsubscribe).toHaveBeenCalledWith('fcm', 'tok-1');
    expect(JSON.parse(storage.data.get(SAVED_KEY)!).token).toBe('tok-2');
    // Permission revoked since: off, nothing registered.
    const revoked = fakes({ permission: 'denied' });
    revoked.storage.setItem(SAVED_KEY, JSON.stringify({ service: 'fcm', token: 't', account: 'https://game.example|7' }));
    expect(await refreshNative(revoked.deps)).toBe(false);
    expect(revoked.deps.register).not.toHaveBeenCalled();
    // A saved record for the other platform, or garbage, is ignored.
    revoked.storage.setItem(SAVED_KEY, JSON.stringify({ service: 'apns', token: 't', account: 'x' }));
    expect(loadSaved(revoked.deps)).toBeNull();
    revoked.storage.setItem(SAVED_KEY, '{');
    expect(loadSaved(revoked.deps)).toBeNull();
  });

  it('a tapped notification opens its world and tab', () => {
    expect(openTarget({ world: 'isle', tab: 'matings', kind: 'mating', count: '1' })).toEqual({ world: 'isle', tab: 'matings' });
    expect(openTarget({ world: '', tab: 'market' })).toEqual({ world: '', tab: 'market' });
    expect(openTarget({ world: 'isle' })).toBeNull();
    expect(openTarget({ world: 1, tab: 'x' })).toBeNull();
    expect(openTarget(null)).toBeNull();
    expect(openTarget('x')).toBeNull();
  });

  it('registerToken waits for the token or the error event', async () => {
    function plugin(fire: (listeners: Map<string, (e: unknown) => void>) => void): PushPlugin & { removed: number } {
      const listeners = new Map<string, (e: unknown) => void>();
      const p = {
        removed: 0,
        register: async () => { setTimeout(() => fire(listeners), 1); },
        unregister: async () => undefined,
        checkPermissions: async () => ({ receive: 'granted' as const }),
        requestPermissions: async () => ({ receive: 'granted' as const }),
        addListener: async (event: string, fn: (e: never) => void) => {
          listeners.set(event, fn as (e: unknown) => void);
          return { remove: async () => { p.removed++; } };
        },
      };
      return p;
    }
    const ok = plugin((l) => l.get('registration')!({ value: 'abc' }));
    expect(await registerToken(ok)).toBe('abc');
    await Promise.resolve();
    expect(ok.removed).toBe(2);
    const bad = plugin((l) => l.get('registrationError')!({ error: 'no aps-environment' }));
    await expect(registerToken(bad)).rejects.toThrow('no aps-environment');
    const silent = plugin(() => {});
    await expect(registerToken(silent, 20)).rejects.toThrow('timed out');
  });
});
