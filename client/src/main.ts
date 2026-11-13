// Entry point: login → (design editor if the player has no lineage) → play.
// One GameSocket lives from login to logout; screens come and go.

import './style.css';
import { api, setApiBase, setWorld, wsUrl } from './api';
import { initialOrigin } from './serverOrigin';
import { GameModel } from './model';
import { loadSession, saveSession, type Session } from './session';
import { GameSocket } from './socket';
import { designScreen } from './ui/design';
import { gameScreen, type GameScreen } from './ui/game';
import { loginScreen } from './ui/login';
import { isOpenMessage, openParams, registerServiceWorker } from './push';
import { h } from './ui/dom';

const root = document.getElementById('app')!;

interface Running {
  session: Session;
  model: GameModel;
  socket: GameSocket;
  screen: GameScreen | null;
  routed: boolean;
}

let running: Running | null = null;
/** A tab to open once the game screen is up (a notification click). */
let pendingTab: string | null = null;

declare global {
  interface Window {
    /** Handles for debugging and the end-to-end test. */
    evo?: { model: GameModel; socket: GameSocket; screen: () => GameScreen | null };
  }
}

function mount(el: HTMLElement): void {
  running?.screen?.destroy();
  if (running) running.screen = null;
  root.replaceChildren(el);
}

function showLogin(notice?: string): void {
  mount(loginScreen(start, notice));
}

function stop(): void {
  if (!running) return;
  running.socket.stop();
  running.screen?.destroy();
  running = null;
  delete window.evo;
}

function logout(): void {
  const token = running?.session.token;
  stop();
  saveSession(null);
  if (token) void api.logout(token).catch(() => {});
  showLogin();
}

function sessionExpired(): void {
  stop();
  saveSession(null);
  showLogin('Your session has ended (it expired, or the server restarted). Please log in again.');
}

function start(session: Session): void {
  stop();
  saveSession(session);
  setWorld(session.world ?? '');
  const model = new GameModel();
  model.player = session.player;
  model.name = session.name;
  const socket = new GameSocket({
    url: () => wsUrl(session.token),
    onMessage: (m) => model.apply(m),
    onState: (s, d) => model.setConn(s, d.retryInMs),
    // A socket that never opens while the HTTP API answers means the token
    // was refused (browsers hide the 401 of a failed upgrade).
    onFailedAttempt: async (attempt) => {
      if (attempt < 2) return true;
      try {
        await api.world();
      } catch {
        return true; // server down: keep retrying
      }
      sessionExpired();
      return false;
    },
  });
  const r: Running = { session, model, socket, screen: null, routed: false };
  running = r;
  window.evo = { model, socket, screen: () => r.screen };
  mount(h('div', { class: 'screen center' }, h('div', { class: 'card' }, h('p', { class: 'muted', id: 'connecting' }, 'Connecting to the world…'))));
  model.on('session_ended', () => {
    if (running === r) sessionExpired();
  });
  model.on('welcome', () => {
    if (r.routed || running !== r) return;
    r.routed = true;
    if (model.welcomeLineage === null) void showDesign(r);
    else void showGame(r);
  });
  socket.start();
}

async function showDesign(r: Running): Promise<void> {
  const el = await designScreen({
    token: r.session.token,
    onFounded: () => void showGame(r),
    onUnauthorized: sessionExpired,
    onLogout: logout,
  });
  if (running === r) mount(el);
}

async function showGame(r: Running): Promise<void> {
  if (running !== r) return;
  const holder = h('div', { class: 'screen center' });
  mount(holder);
  const screen = await gameScreen({
    model: r.model, send: (m) => r.socket.send(m), onLogout: logout, onUnauthorized: sessionExpired,
    token: r.session.token, mountInto: (el) => root.replaceChildren(el), world: r.session.world ?? '',
  });
  if (running !== r) return screen.destroy();
  r.screen = screen;
  if (pendingTab) {
    screen.showTab(pendingTab);
    pendingTab = null;
  }
}

/** A notification was clicked while the game is open: switch to its world (restarting the session there) and tab. */
async function openFromNotification(world: string, tab: string): Promise<void> {
  const r = running;
  if (!r) return;
  let current = r.session.world ?? '';
  if (!current) {
    try {
      current = (await api.worlds()).default;
    } catch {
      /* unknown: compare with '' */
    }
  }
  if (world && world !== current) {
    pendingTab = tab;
    start({ ...r.session, world });
  } else if (r.screen) {
    r.screen.showTab(tab);
  } else {
    pendingTab = tab;
  }
}

setApiBase(initialOrigin());
void registerServiceWorker();
if ('serviceWorker' in navigator) {
  navigator.serviceWorker.addEventListener('message', (e: MessageEvent) => {
    if (isOpenMessage(e.data)) void openFromNotification(e.data.world, e.data.tab);
  });
}
// A notification click that opened a new window: `?world=…&tab=…`.
const opened = openParams(window.location.href);
if (opened.world !== null || opened.tab !== null) window.history.replaceState(null, '', opened.clean);
pendingTab = opened.tab;
const saved = loadSession();
if (saved) start(opened.world ? { ...saved, world: opened.world } : saved);
else showLogin();
