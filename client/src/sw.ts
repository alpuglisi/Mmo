// The service worker (built to `dist/sw.js`, registered by `push.ts`): shows
// push notifications and opens or focuses the game on a click, on the
// notification's world and tab. It caches nothing: the game needs the
// server anyway.
//
// The worker types are declared here (the project's lib is DOM, which
// conflicts with the WebWorker lib in one program).

import { notificationFor, parsePayload, pickClient, targetUrl, type ClickData } from './swLogic';

interface ExtendableEvt extends Event {
  waitUntil(p: Promise<unknown>): void;
}
interface PushEvt extends ExtendableEvt {
  data: { json(): unknown } | null;
}
interface NotificationEvt extends ExtendableEvt {
  notification: Notification;
}
interface WindowClientLike {
  url: string;
  focused: boolean;
  visibilityState: string;
  focus(): Promise<WindowClientLike>;
  postMessage(message: unknown): void;
}
interface WorkerScope {
  registration: ServiceWorkerRegistration;
  clients: {
    matchAll(o: { type: 'window'; includeUncontrolled: boolean }): Promise<WindowClientLike[]>;
    openWindow(url: string): Promise<WindowClientLike | null>;
    claim(): Promise<void>;
  };
  skipWaiting(): Promise<void>;
  addEventListener(type: string, listener: (e: never) => void): void;
}

const sw = globalThis as unknown as WorkerScope;

sw.addEventListener('install', () => {
  void sw.skipWaiting();
});

sw.addEventListener('activate', (e: ExtendableEvt) => {
  e.waitUntil(sw.clients.claim());
});

sw.addEventListener('push', (e: PushEvt) => {
  let data: unknown = null;
  try {
    data = e.data?.json() ?? null;
  } catch {
    /* not JSON: a generic notification */
  }
  const { title, options } = notificationFor(parsePayload(data));
  e.waitUntil(sw.registration.showNotification(title, options));
});

async function openGame(data: ClickData | null): Promise<void> {
  const scope = sw.registration.scope;
  const windows = await sw.clients.matchAll({ type: 'window', includeUncontrolled: true });
  const i = pickClient(windows, scope);
  if (i >= 0) {
    const w = windows[i];
    // The page switches world and tab (main.ts).
    if (data) w.postMessage({ type: 'evo-open', world: data.world, tab: data.tab });
    await w.focus();
    return;
  }
  await sw.clients.openWindow(targetUrl(scope, data));
}

sw.addEventListener('notificationclick', (e: NotificationEvt) => {
  e.notification.close();
  e.waitUntil(openGame((e.notification.data as ClickData | null) ?? null));
});
