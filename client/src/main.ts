// Entry point: login → (design editor if the player has no lineage) → play.
// One GameSocket lives from login to logout; screens come and go.

import './style.css';
import { api, wsUrl } from './api';
import { GameModel } from './model';
import { loadSession, saveSession, type Session } from './session';
import { GameSocket } from './socket';
import { designScreen } from './ui/design';
import { gameScreen, type GameScreen } from './ui/game';
import { loginScreen } from './ui/login';
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
  showLogin('Your session has ended (the server may have restarted). Please log in again.');
}

function start(session: Session): void {
  stop();
  saveSession(session);
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
  const screen = await gameScreen({ model: r.model, send: (m) => r.socket.send(m), onLogout: logout, mountInto: (el) => root.replaceChildren(el) });
  if (running !== r) return screen.destroy();
  r.screen = screen;
}

const saved = loadSession();
if (saved) start(saved);
else showLogin();
