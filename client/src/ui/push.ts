// The push notification settings card (Genetics tab, under the alert
// preferences): this device's status with an Enable / Turn off button, the
// per-kind switches (saved to the account) and how many devices are
// subscribed. In a browser the device subscribes with Web Push; in the
// Android and iOS apps it registers its native push token (FCM / APNs).

import { ApiError, api, apiBase } from '../api';
import {
  ANDROID_CHANNEL, disableNative, enableNative, nativeService, nativeStatus, refreshNative, registerToken,
  type NativePermission, type NativePushDeps, type PushPlugin,
} from '../nativePush';
import { PUSH_KINDS, type NativeService, type PushKind, type PushPrefs } from '../protocol';
import {
  PUSH_HINT, PUSH_LABEL, PushError, currentSubscription, detectEnv, deviceLabel, disablePush, enablePush, normalizePushPrefs, pushStatus, sameKey,
  setPushKind, statusText, wants, type PushDeps, type PushEnv, type PushStatus,
} from '../push';
import { h } from './dom';

export interface PushSettingsDeps {
  token: string;
  /** The logged-in player (a native registration belongs to one account). */
  player?: number;
  onUnauthorized: () => void;
  toast: (kind: 'error' | 'info', text: string) => void;
  /** Overrides for tests. */
  env?: () => PushEnv;
  native?: () => Promise<NativePushDeps | null>;
}

export interface PushSettings {
  el: HTMLElement;
  destroy: () => void;
}

function storage(): Storage | null {
  try {
    return window.localStorage;
  } catch {
    return null;
  }
}

/** The app's native push plumbing: `@capacitor/push-notifications` (loaded only in the apps) and the server. */
async function appNativeDeps(token: string, player: number | undefined): Promise<NativePushDeps | null> {
  const cap = (window as unknown as { Capacitor?: { getPlatform?: () => string } }).Capacitor;
  const service = nativeService(cap?.getPlatform?.() ?? 'web');
  if (!service) return null;
  const { PushNotifications } = await import('@capacitor/push-notifications');
  const plugin = PushNotifications as unknown as PushPlugin;
  return {
    service,
    // Registering without the Firebase config would crash the Android app.
    available: service === 'apns' || __EVO_FCM__,
    checkPermissions: async () => (await plugin.checkPermissions()).receive,
    requestPermissions: async () => (await plugin.requestPermissions()).receive,
    register: async () => {
      if (service === 'fcm' && plugin.createChannel) {
        await plugin.createChannel({ id: ANDROID_CHANNEL, name: 'Game events', description: 'Mating offers, deaths, the market, seasons', importance: 4 })
          .catch(() => undefined);
      }
      return registerToken(plugin);
    },
    unregister: () => plugin.unregister(),
    subscribe: (s: NativeService, t: string, label: string) => api.pushNativeSubscribe(token, s, t, label),
    unsubscribe: (s: NativeService, t: string) => api.pushNativeUnsubscribe(token, s, t),
    label: `Evolution MMO app on ${service === 'fcm' ? 'Android' : 'iOS'}`,
    account: `${apiBase() || window.location.origin}|${player ?? ''}`,
    storage: storage(),
  };
}

export function pushSettings(d: PushSettingsDeps): PushSettings {
  const env = d.env ?? (() => detectEnv());
  const body = h('div', { id: 'push-settings-body' });
  const el = h('details', { class: 'chart-card', id: 'push-settings' },
    h('summary', {}, 'Push notifications'),
    body);
  let serverEnabled: boolean | null = null;
  let publicKey = '';
  let services: NativeService[] | null = null;
  let prefs: PushPrefs = { off: [] };
  let subscribed = false;
  let devices: number | null = null;
  let busy = false;
  let alive = true;
  // In the apps: the native plumbing (null until loaded, or in a browser).
  let native: NativePushDeps | null = null;
  let permission: NativePermission | null = null;

  const deps: PushDeps = {
    registration: async () => {
      const e = env();
      if (!e.serviceWorker || !e.secure) return null;
      // A registration that isn't there yet (first load) is waited for, but not forever.
      return Promise.race([navigator.serviceWorker.ready, new Promise<null>((r) => setTimeout(() => r(null), 4000))]);
    },
    requestPermission: () => Notification.requestPermission(),
    subscribe: (sub, label) => api.pushSubscribe(d.token, sub, label),
    unsubscribe: (endpoint) => api.pushUnsubscribe(d.token, endpoint),
    label: deviceLabel(navigator.userAgent),
  };

  function failed(e: unknown, what: string): void {
    if (e instanceof ApiError && e.status === 401) return d.onUnauthorized();
    if (e instanceof PushError && e.code === 'denied') d.toast('error', 'Notifications were not allowed.');
    else d.toast('error', `${what}: ${(e as Error).message}`);
  }

  function status(): PushStatus {
    if (native) return nativeStatus(native.available, native.service, services, serverEnabled, permission, subscribed);
    return pushStatus(env(), serverEnabled, subscribed);
  }

  function render(): void {
    if (!alive) return;
    const s = status();
    const canToggle = s === 'on' || s === 'off';
    const needsKey = !native && !publicKey;
    const toggle = h('button', {
      id: 'push-toggle', class: s === 'on' ? 'secondary' : '', disabled: !canToggle || busy || (s === 'off' && needsKey),
      onclick: () => void (s === 'on' ? turnOff() : turnOn()),
    }, s === 'on' ? 'Turn off on this device' : 'Enable on this device');
    const box = (k: PushKind) => h('input', {
      type: 'checkbox', checked: wants(prefs, k), 'data-push-kind': k, 'aria-label': `push ${PUSH_LABEL[k]}`, disabled: serverEnabled === false || busy,
      onchange: (e: Event) => void savePrefs(setPushKind(prefs, k, (e.target as HTMLInputElement).checked)),
    });
    body.replaceChildren(
      h('p', { class: 'muted small', id: 'push-status', 'data-status': s }, statusText(s, native !== null)),
      toggle,
      h('table', { class: 'prefs-table', id: 'push-kinds' },
        h('thead', {}, h('tr', {}, h('th', {}, 'Notify me about'), h('th', {}, 'Push'))),
        h('tbody', {}, ...PUSH_KINDS.map((k) => h('tr', {},
          h('td', { title: PUSH_HINT[k] }, PUSH_LABEL[k], h('span', { class: 'muted small' }, PUSH_HINT[k])),
          h('td', {}, box(k)))))),
      h('p', { class: 'muted small' },
        'Saved to your account, for every world (a notification names its world). Alert kinds hidden from the feed are not pushed. ',
        devices === null ? '' : `Devices subscribed: ${devices}.`),
    );
  }

  async function refreshDevices(): Promise<void> {
    try {
      const list = await api.pushSubscriptions(d.token);
      devices = list.subscriptions.length + (list.devices?.length ?? 0);
    } catch {
      devices = null;
    }
  }

  async function turnOn(): Promise<void> {
    busy = true;
    render();
    try {
      if (native) await enableNative(native);
      else await enablePush(deps, publicKey);
      subscribed = true;
      d.toast('info', 'Push notifications are on for this device.');
    } catch (e) {
      failed(e, 'Push notifications not enabled');
    }
    if (native) permission = await native.checkPermissions().catch(() => permission);
    await refreshDevices();
    busy = false;
    render();
  }

  async function turnOff(): Promise<void> {
    busy = true;
    render();
    try {
      if (native) await disableNative(native);
      else await disablePush(deps);
      subscribed = false;
      await refreshDevices();
    } catch (e) {
      failed(e, 'Push notifications not turned off');
    }
    busy = false;
    render();
  }

  async function savePrefs(p: PushPrefs): Promise<void> {
    const before = prefs;
    prefs = p;
    render();
    try {
      prefs = normalizePushPrefs(await api.setPushPrefs(d.token, p));
    } catch (e) {
      prefs = before;
      failed(e, 'Push preferences not saved');
    }
    render();
  }

  /** Web Push: hand this browser's subscription over again if it has one. */
  async function refreshWeb(): Promise<void> {
    const e = env();
    if (!serverEnabled || !publicKey || pushStatus(e, true, false) !== 'off') return;
    try {
      const sub = await currentSubscription(deps);
      if (sub && sameKey(sub.options?.applicationServerKey, publicKey) && e.permission === 'granted') {
        // Hand it over again: the server may have dropped it, or the
        // browser rotated it.
        await api.pushSubscribe(d.token, sub.toJSON(), deps.label);
        subscribed = true;
      }
    } catch {
      /* keep "off": Enable subscribes afresh */
    }
  }

  /** The apps: whether this device is registered, and register it again if so. */
  async function refreshApp(n: NativePushDeps): Promise<void> {
    if (!n.available) return;
    permission = await n.checkPermissions().catch(() => null);
    if (!serverEnabled || !services?.includes(n.service)) return;
    try {
      subscribed = await refreshNative(n);
    } catch {
      /* keep "off": Enable registers afresh */
    }
  }

  void (async () => {
    render();
    if (env().native) {
      try {
        native = await (d.native ?? (() => appNativeDeps(d.token, d.player)))();
      } catch {
        native = null;
      }
    }
    try {
      const key = await api.pushKey();
      serverEnabled = key.enabled;
      publicKey = key.public_key ?? '';
      services = key.native ?? [];
      prefs = normalizePushPrefs(await api.pushPrefs(d.token));
    } catch (e) {
      if (e instanceof ApiError && e.status === 401) return d.onUnauthorized();
      /* an older server, or offline: nothing to offer */
      serverEnabled = serverEnabled ?? false;
    }
    if (native) await refreshApp(native);
    else await refreshWeb();
    await refreshDevices();
    render();
  })();

  return { el, destroy: () => { alive = false; } };
}
