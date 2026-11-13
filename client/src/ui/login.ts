import { api, apiBase, ApiError, setApiBase } from '../api';
import { isNativeShell, normalizeOrigin, saveOrigin } from '../serverOrigin';
import type { Session } from '../session';
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
      const s = kind === 'login' ? await api.login(name.value.trim(), password.value) : await api.register(name.value.trim(), password.value);
      onLogin(s);
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
