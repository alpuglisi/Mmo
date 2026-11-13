// The push notification settings card (Genetics tab, under the alert
// preferences): this device's status with an Enable / Turn off button, the
// per-kind switches (saved to the account) and how many devices are
// subscribed.

import { ApiError, api } from '../api';
import { PUSH_KINDS, type PushKind, type PushPrefs } from '../protocol';
import {
  PUSH_HINT, PUSH_LABEL, PushError, currentSubscription, detectEnv, deviceLabel, disablePush, enablePush, normalizePushPrefs, pushStatus, sameKey,
  setPushKind, statusText, wants, type PushDeps, type PushEnv,
} from '../push';
import { h } from './dom';

export interface PushSettingsDeps {
  token: string;
  onUnauthorized: () => void;
  toast: (kind: 'error' | 'info', text: string) => void;
  /** Overrides for tests. */
  env?: () => PushEnv;
}

export interface PushSettings {
  el: HTMLElement;
  destroy: () => void;
}

export function pushSettings(d: PushSettingsDeps): PushSettings {
  const env = d.env ?? (() => detectEnv());
  const body = h('div', { id: 'push-settings-body' });
  const el = h('details', { class: 'chart-card', id: 'push-settings' },
    h('summary', {}, 'Push notifications'),
    body);
  let serverEnabled: boolean | null = null;
  let publicKey = '';
  let prefs: PushPrefs = { off: [] };
  let subscribed = false;
  let devices: number | null = null;
  let busy = false;
  let alive = true;

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

  function render(): void {
    if (!alive) return;
    const status = pushStatus(env(), serverEnabled, subscribed);
    const canToggle = status === 'on' || status === 'off';
    const toggle = h('button', {
      id: 'push-toggle', class: status === 'on' ? 'secondary' : '', disabled: !canToggle || busy || (status === 'off' && !publicKey),
      onclick: () => void (status === 'on' ? turnOff() : turnOn()),
    }, status === 'on' ? 'Turn off on this device' : 'Enable on this device');
    const box = (k: PushKind) => h('input', {
      type: 'checkbox', checked: wants(prefs, k), 'data-push-kind': k, 'aria-label': `push ${PUSH_LABEL[k]}`, disabled: serverEnabled === false || busy,
      onchange: (e: Event) => void savePrefs(setPushKind(prefs, k, (e.target as HTMLInputElement).checked)),
    });
    body.replaceChildren(
      h('p', { class: 'muted small', id: 'push-status', 'data-status': status }, statusText(status)),
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
      devices = (await api.pushSubscriptions(d.token)).subscriptions.length;
    } catch {
      devices = null;
    }
  }

  async function turnOn(): Promise<void> {
    busy = true;
    render();
    try {
      await enablePush(deps, publicKey);
      subscribed = true;
      await refreshDevices();
      d.toast('info', 'Push notifications are on for this device.');
    } catch (e) {
      failed(e, 'Push notifications not enabled');
    }
    busy = false;
    render();
  }

  async function turnOff(): Promise<void> {
    busy = true;
    render();
    try {
      await disablePush(deps);
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

  void (async () => {
    render();
    try {
      const key = await api.pushKey();
      serverEnabled = key.enabled;
      publicKey = key.public_key ?? '';
      prefs = normalizePushPrefs(await api.pushPrefs(d.token));
    } catch (e) {
      if (e instanceof ApiError && e.status === 401) return d.onUnauthorized();
      /* an older server, or offline: nothing to offer */
      serverEnabled = serverEnabled ?? false;
    }
    const e = env();
    if (serverEnabled && publicKey && pushStatus(e, true, false) === 'off') {
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
    await refreshDevices();
    render();
  })();

  return { el, destroy: () => { alive = false; } };
}
