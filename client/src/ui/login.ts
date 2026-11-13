import { api, apiBase, ApiError, setApiBase } from '../api';
import type { WorldList } from '../protocol';
import { isNativeShell, normalizeOrigin, saveOrigin } from '../serverOrigin';
import type { Session } from '../session';
import { describeWorld, loadMemory, pickWorld, remember, remembered, saveMemory } from '../worldChoice';
import { h } from './dom';

export function loginScreen(onLogin: (s: Session) => void, notice?: string): HTMLElement {
  const name = h('input', { id: 'login-name', name: 'username', autocomplete: 'username', placeholder: 'name', required: true, minlength: 3, maxlength: 24 });
  const password = h('input', { id: 'login-password', name: 'password', type: 'password', autocomplete: 'current-password', placeholder: 'password (8+ characters)', required: true });
  const error = h('p', { class: 'error', role: 'alert' }, notice ?? '');
  const login = h('button', { type: 'submit', id: 'login-submit' }, 'Log in');
  const register = h('button', { type: 'button', id: 'register-submit', class: 'secondary' }, 'Create account');
  // The game server: the page's own origin unless set (a Capacitor app must set it).
  const native = isNativeShell();
  const server = h('input', { id: 'server-origin', name: 'server', type: 'text', inputmode: 'url', autocapitalize: 'off', spellcheck: 'false',
    placeholder: native ? 'https://game.example.org' : `this page (${window.location.host || 'same origin'})`, value: apiBase() });
  const serverBox = h('details', { class: 'server-box', id: 'server-box' }, h('summary', {}, 'Server: ', h('span', { id: 'server-current' }, apiBase() || 'this page')),
    h('label', {}, 'Game server address', server));
  if (native && !apiBase()) serverBox.open = true;

  // The world picker: shown when the server hosts more than one world. It
  // preselects the world this name played last on this server (or the last
  // world chosen here), until the player picks one themselves.
  const worldSelect = h('select', { id: 'world-select', name: 'world' }) as HTMLSelectElement;
  const worldInfo = h('p', { class: 'muted world-info', id: 'world-info' });
  const worldBox = h('div', { class: 'world-box', id: 'world-box', hidden: true },
    h('label', {}, 'World', worldSelect), worldInfo);
  let worlds: WorldList | null = null;
  let picked = false;
  let listFor: string | null = null;

  function showInfo(): void {
    const w = worlds?.worlds.find((x) => x.id === worldSelect.value);
    worldInfo.textContent = w ? describeWorld(w) : '';
  }

  function preselect(): void {
    if (!worlds || picked) return;
    worldSelect.value = pickWorld(worlds, remembered(loadMemory(), apiBase(), name.value));
    showInfo();
  }

  /** Fetch the server's worlds (again when the server address changed). */
  async function loadWorlds(): Promise<void> {
    const origin = apiBase();
    if (listFor === origin && worlds) return;
    listFor = origin;
    try {
      const list = await api.worlds();
      if (apiBase() !== origin) return;
      worlds = list;
      worldSelect.replaceChildren(...list.worlds.map((w) => h('option', { value: w.id }, w.name)));
      worldBox.hidden = list.worlds.length < 2;
      preselect();
    } catch {
      // An older server (no world list) or none reachable: play its default world.
      if (apiBase() !== origin) return;
      worlds = null;
      listFor = null;
      worldBox.hidden = true;
    }
  }
  worldSelect.addEventListener('change', () => {
    picked = true;
    showInfo();
  });
  name.addEventListener('change', preselect);
  server.addEventListener('change', () => {
    if (normalizeOrigin(server.value) !== null && applyServer()) void loadWorlds();
  });
  if (!native || apiBase()) void loadWorlds();

  /** Apply the Server field; false (with a message) when it isn't a usable address. */
  function applyServer(): boolean {
    const origin = normalizeOrigin(server.value);
    if (origin === null) {
      error.textContent = 'The server address must be an http(s) URL, like https://game.example.org:8080.';
      serverBox.open = true;
      return false;
    }
    if (native && !origin) {
      error.textContent = 'Enter the game server address.';
      serverBox.open = true;
      return false;
    }
    setApiBase(origin);
    saveOrigin(origin);
    server.value = origin;
    serverBox.querySelector('#server-current')!.textContent = origin || 'this page';
    return true;
  }

  async function go(kind: 'login' | 'register'): Promise<void> {
    error.textContent = '';
    if (!name.value.trim() || !password.value) {
      error.textContent = 'Enter a name and a password.';
      return;
    }
    if (!applyServer()) return;
    login.disabled = register.disabled = true;
    try {
      await loadWorlds();
      const s = kind === 'login' ? await api.login(name.value.trim(), password.value) : await api.register(name.value.trim(), password.value);
      const world = worlds ? pickWorld(worlds, worldSelect.value) : '';
      if (world) saveMemory(remember(loadMemory(), apiBase(), s.name, world));
      onLogin({ ...s, world });
    } catch (e) {
      error.textContent = e instanceof ApiError ? describe(e) : String(e);
    } finally {
      login.disabled = register.disabled = false;
    }
  }

  const form = h('form', { class: 'card login', onsubmit: ((ev: Event) => { ev.preventDefault(); void go('login'); }) as EventListener },
    h('h1', {}, 'Evolution MMO'),
    h('p', { class: 'muted' }, 'Design a species, guide its lineage, trade genes.'),
    h('label', {}, 'Name', name),
    h('label', {}, 'Password', password),
    worldBox,
    h('div', { class: 'row' }, login, register),
    error,
    serverBox,
  );
  register.addEventListener('click', () => void go('register'));
  return h('div', { class: 'screen center' }, form);
}

function describe(e: ApiError): string {
  switch (e.code) {
    case 'bad_credentials': return 'Wrong name or password.';
    case 'name_taken': return 'That name is taken.';
    default: return e.message;
  }
}
