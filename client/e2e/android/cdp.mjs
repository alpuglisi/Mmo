// A minimal Chrome DevTools Protocol client: list the targets of a DevTools
// HTTP endpoint (an Android WebView forwarded with `adb forward`, or a
// desktop Chromium started with --remote-debugging-port) and talk to one
// page over its WebSocket. No dependencies: Node 22's global WebSocket and
// fetch. Playwright's connectOverCDP expects a whole browser behind the
// endpoint, which a WebView's DevTools socket is not, so the Android run
// speaks CDP directly.

/** The page targets behind `endpoint` (e.g. http://127.0.0.1:9222). */
export async function listPages(endpoint) {
  const res = await fetch(`${endpoint}/json/list`, { signal: AbortSignal.timeout(5000) });
  if (!res.ok) throw new Error(`${endpoint}/json/list: HTTP ${res.status}`);
  const targets = await res.json();
  return targets.filter((t) => t.type === 'page' && t.webSocketDebuggerUrl);
}

/**
 * The WebSocket URL of a target, pointed at `endpoint`'s host and port: a
 * WebView reports its own socket address, not the forwarded one.
 */
export function wsUrlFor(endpoint, target) {
  const u = new URL(target.webSocketDebuggerUrl);
  const e = new URL(endpoint);
  u.host = e.host;
  return u.toString();
}

export class CdpSession {
  /** @param {string} wsUrl */
  static async connect(wsUrl, { timeoutMs = 10000 } = {}) {
    const ws = new WebSocket(wsUrl);
    await new Promise((resolve, reject) => {
      const timer = setTimeout(() => reject(new Error(`CDP connect timed out: ${wsUrl}`)), timeoutMs);
      ws.addEventListener('open', () => { clearTimeout(timer); resolve(); }, { once: true });
      ws.addEventListener('error', () => { clearTimeout(timer); reject(new Error(`CDP connect failed: ${wsUrl}`)); }, { once: true });
    });
    return new CdpSession(ws);
  }

  constructor(ws) {
    this.ws = ws;
    this.nextId = 1;
    this.pending = new Map();
    this.listeners = new Map();
    this.closed = false;
    ws.addEventListener('message', (ev) => this.onMessage(ev.data));
    ws.addEventListener('close', () => {
      this.closed = true;
      for (const p of this.pending.values()) p.reject(new Error(`CDP socket closed during ${p.method}`));
      this.pending.clear();
    });
  }

  onMessage(data) {
    const msg = JSON.parse(typeof data === 'string' ? data : Buffer.from(data).toString('utf8'));
    if (msg.id !== undefined) {
      const p = this.pending.get(msg.id);
      if (!p) return;
      this.pending.delete(msg.id);
      clearTimeout(p.timer);
      if (msg.error) p.reject(new Error(`${p.method}: ${msg.error.message}${msg.error.data ? ` (${msg.error.data})` : ''}`));
      else p.resolve(msg.result ?? {});
      return;
    }
    for (const fn of this.listeners.get(msg.method) ?? []) fn(msg.params ?? {});
  }

  /** Send a command; rejects on a protocol error, a closed socket or no reply within `timeoutMs`. */
  send(method, params = {}, timeoutMs = 15000) {
    if (this.closed) return Promise.reject(new Error(`CDP socket closed (${method})`));
    const id = this.nextId++;
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => {
        this.pending.delete(id);
        reject(new Error(`CDP ${method} timed out after ${timeoutMs} ms`));
      }, timeoutMs);
      this.pending.set(id, { resolve, reject, timer, method });
      this.ws.send(JSON.stringify({ id, method, params }));
    });
  }

  on(method, fn) {
    if (!this.listeners.has(method)) this.listeners.set(method, []);
    this.listeners.get(method).push(fn);
  }

  /** Evaluate `fn` (a function or an expression string) in the page and return its JSON value. */
  async evaluate(fn, ...args) {
    const expression = typeof fn === 'function' ? `(${fn.toString()})(...${JSON.stringify(args)})` : fn;
    const r = await this.send('Runtime.evaluate', { expression, awaitPromise: true, returnByValue: true, userGesture: true });
    if (r.exceptionDetails) {
      const d = r.exceptionDetails;
      throw new Error(`page threw: ${d.exception?.description ?? d.text}`);
    }
    return r.result?.value;
  }

  close() {
    try {
      this.ws.close();
    } catch {
      /* already closed */
    }
  }
}

/**
 * Connect to the first page target behind `endpoint` that `match` accepts,
 * retrying while the endpoint or the page is not there yet.
 */
export async function connectPage(endpoint, { match = () => true, timeoutMs = 60000 } = {}) {
  const end = Date.now() + timeoutMs;
  let last = 'no page target';
  while (Date.now() < end) {
    try {
      const page = (await listPages(endpoint)).find(match);
      if (page) return await CdpSession.connect(wsUrlFor(endpoint, page));
      last = 'no matching page target';
    } catch (e) {
      last = e.message;
    }
    await new Promise((r) => setTimeout(r, 500));
  }
  throw new Error(`no DevTools page at ${endpoint} after ${timeoutMs} ms (${last})`);
}
