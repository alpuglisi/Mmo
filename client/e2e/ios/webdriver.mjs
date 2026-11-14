// A minimal W3C WebDriver client for Appium's XCUITest driver, and a
// DevTools-shaped session over it, so the app scenario written against CDP
// (e2e/android/scenario.mjs) can drive the iOS app's WKWebView too.
//
// iOS has no DevTools protocol: Appium reaches the WKWebView through the
// simulator's Web Inspector service (webinspectord; the Capacitor debug
// build marks the web view inspectable) and runs JavaScript in it from a
// WEBVIEW_* context. WebViewSession maps the handful of CDP calls the
// scenario makes onto that:
//
//   evaluate(fn | expr, ...args)   → execute/sync (a promise result is
//                                    parked in the page and polled)
//   Runtime.enable, Page.enable    → nothing to do
//   Page.addScriptToEvaluateOnNewDocument → nothing (WebKit over WebDriver
//                                    has no such hook; the scenario also
//                                    evaluates its probe right away, and the
//                                    socket opens only after login)
//   Input.dispatchMouseEvent       → a tap on mouseReleased: the element at
//                                    the point gets mouse events and click()
//                                    in the page, or (nativeTap) a real tap
//                                    through XCUITest at those coordinates
//   Input.insertText               → execCommand('insertText') on the focused field
//   Page.captureScreenshot         → the `screenshot` option (the simulator's
//                                    own screenshot: the web view fills the screen)
//   Runtime.consoleAPICalled, Runtime.exceptionThrown → from a console and
//                                    error hook installed in the page,
//                                    drained with every evaluate
//
// No dependencies (node:http).

import http from 'node:http';
import https from 'node:https';

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

/**
 * One HTTP request with only our own deadline. Not fetch: its client gives
 * up after 300 s without response headers whatever the abort signal says,
 * and creating the session (building WebDriverAgent on a cold cache) takes
 * longer than that (CI: "fetch failed" after 5 min while Appium went on).
 */
function httpRequest(url, method, body, timeoutMs) {
  return new Promise((resolve, reject) => {
    const u = new URL(url);
    const req = (u.protocol === 'https:' ? https : http).request(u, {
      method,
      headers: body === undefined ? {} : { 'content-type': 'application/json', 'content-length': Buffer.byteLength(body) },
    }, (res) => {
      const chunks = [];
      res.on('data', (c) => chunks.push(c));
      res.on('end', () => { clearTimeout(timer); resolve({ status: res.statusCode, text: Buffer.concat(chunks).toString('utf8') }); });
      res.on('error', (e) => { clearTimeout(timer); reject(e); });
    });
    const timer = setTimeout(() => req.destroy(new Error(`${method} ${u.pathname}: no answer within ${timeoutMs} ms`)), timeoutMs);
    req.on('error', (e) => { clearTimeout(timer); reject(e); });
    if (body !== undefined) req.write(body);
    req.end();
  });
}

export class WebDriverError extends Error {
  constructor(method, path, status, value) {
    super(`${method} ${path}: HTTP ${status} ${value?.error ?? ''} ${value?.message?.split('\n')[0] ?? ''}`.trim());
    this.status = status;
    this.error = value?.error;
  }
}

export class WebDriver {
  /** @param {string} base e.g. http://127.0.0.1:4723 */
  constructor(base) {
    this.base = base.replace(/\/+$/, '');
    this.sessionId = null;
  }

  async request(method, path, body, timeoutMs = 60000) {
    const res = await httpRequest(`${this.base}${path}`, method, body === undefined ? undefined : JSON.stringify(body), timeoutMs);
    const text = res.text;
    let json;
    try {
      json = text ? JSON.parse(text) : {};
    } catch {
      throw new WebDriverError(method, path, res.status, { message: text.slice(0, 200) });
    }
    if (res.status >= 400 || (json.value && json.value.error)) throw new WebDriverError(method, path, res.status, json.value);
    return json.value;
  }

  /** The session's own path prefix. */
  s(path) {
    if (!this.sessionId) throw new Error('no WebDriver session');
    return `/session/${this.sessionId}${path}`;
  }

  async status() {
    return this.request('GET', '/status', undefined, 5000);
  }

  /** Start a session with these capabilities (`alwaysMatch`); can take minutes (WebDriverAgent build). */
  async createSession(capabilities, timeoutMs = 15 * 60000) {
    const v = await this.request('POST', '/session', { capabilities: { alwaysMatch: capabilities, firstMatch: [{}] } }, timeoutMs);
    this.sessionId = v.sessionId;
    return v;
  }

  async deleteSession() {
    if (!this.sessionId) return;
    const id = this.sessionId;
    this.sessionId = null;
    await this.request('DELETE', `/session/${id}`, undefined, 60000).catch(() => {});
  }

  /** Context names, or objects ({id, title, url}) with `appium:fullContextList`. */
  contexts() {
    return this.request('GET', this.s('/contexts'), undefined, 60000);
  }

  setContext(name, timeoutMs = 60000) {
    return this.request('POST', this.s('/context'), { name }, timeoutMs);
  }

  currentContext() {
    return this.request('GET', this.s('/context'));
  }

  execute(script, args = [], timeoutMs = 30000) {
    return this.request('POST', this.s('/execute/sync'), { script, args }, timeoutMs);
  }
}

/** Installed in the page with every evaluate (once per document): console and error capture. */
const HOOK = `if (!window.__e2eCon) {
  var q = window.__e2eCon = [];
  var push = function (kind, type, text) { q.push([kind, type, String(text)]); if (q.length > 500) q.shift(); };
  ['log', 'info', 'warn', 'error', 'debug'].forEach(function (t) {
    var orig = console[t];
    console[t] = function () {
      try { push('console', t, Array.prototype.map.call(arguments, function (a) {
        if (typeof a === 'string') return a;
        try { return a instanceof Error ? (a.stack || String(a)) : JSON.stringify(a); } catch (e) { return String(a); }
      }).join(' ')); } catch (e) {}
      return orig.apply(console, arguments);
    };
  });
  window.addEventListener('error', function (e) { push('exception', 'error', (e.error && e.error.stack) || e.message); });
  window.addEventListener('unhandledrejection', function (e) { var r = e.reason; push('exception', 'rejection', 'Uncaught (in promise) ' + ((r && r.stack) || r)); });
}
var __drain = function () { var q = window.__e2eCon || []; return q.splice(0, q.length); };`;

/**
 * The script for execute/sync that runs `fnSrc` (a function's source, or
 * null for the expression in args[0]) and reports {t: 'ok'|'err'|'pending', ...}.
 */
export function evalScript(fnSrc, pendingId) {
  const call = fnSrc === null
    ? 'r = (0, eval)(arguments[0]);'
    : `r = (${fnSrc}).apply(null, Array.prototype.slice.call(arguments));`;
  return `${HOOK}
var r;
try { ${call} } catch (e) { return { t: 'err', e: (e && e.stack) || String(e), logs: __drain() }; }
if (r && typeof r.then === 'function') {
  var box = (window.__e2eEval = window.__e2eEval || {});
  box[${JSON.stringify(pendingId)}] = { done: false };
  r.then(function (v) { box[${JSON.stringify(pendingId)}] = { done: true, t: 'ok', v: v === undefined ? null : v }; },
         function (e) { box[${JSON.stringify(pendingId)}] = { done: true, t: 'err', e: (e && e.stack) || String(e) }; });
  return { t: 'pending', logs: __drain() };
}
return { t: 'ok', v: r === undefined ? null : r, logs: __drain() };`;
}

/** The script that collects a parked promise result. */
export function pollScript(pendingId) {
  return `${HOOK}
var box = window.__e2eEval || {};
var x = box[${JSON.stringify(pendingId)}];
if (!x) return { t: 'err', e: 'the page lost a pending evaluation (reloaded?)', logs: __drain() };
if (!x.done) return { t: 'pending', logs: __drain() };
delete box[${JSON.stringify(pendingId)}];
return { t: x.t, v: x.v, e: x.e, logs: __drain() };`;
}

/**
 * The first WEBVIEW_* context whose url matches `pageRe` (when urls are
 * known), waiting up to `timeoutMs` for it to show up.
 */
export async function findWebView(driver, { pageRe = /./, timeoutMs = 90000 } = {}) {
  const end = Date.now() + timeoutMs;
  let last = 'no contexts yet';
  while (Date.now() < end) {
    try {
      const list = await driver.contexts();
      const views = list
        .map((c) => (typeof c === 'string' ? { id: c } : c))
        .filter((c) => String(c.id).startsWith('WEBVIEW'));
      const pick = views.find((c) => c.url === undefined || pageRe.test(c.url));
      if (pick) return pick;
      last = `contexts: ${JSON.stringify(list)}`;
    } catch (e) {
      last = e.message;
    }
    await sleep(1000);
  }
  throw new Error(`no web view context after ${timeoutMs} ms (${last})`);
}

let pendingSeq = 0;

export class WebViewSession {
  /**
   * @param {WebDriver} driver with a session, already switched to `context`
   * @param {{ context: string, screenshot?: () => Promise<string>, nativeTap?: boolean }} opts
   *        screenshot() returns a PNG as base64
   */
  constructor(driver, { context, screenshot, nativeTap = false }) {
    this.driver = driver;
    this.context = context;
    this.screenshot = screenshot;
    this.nativeTap = nativeTap;
    this.listeners = new Map();
    this.closed = false;
  }

  /** Switch the driver to the first matching web view and wrap it. */
  static async attach(driver, { pageRe, timeoutMs, ...opts } = {}) {
    const view = await findWebView(driver, { pageRe, timeoutMs });
    await driver.setContext(view.id);
    const session = new WebViewSession(driver, { context: view.id, ...opts });
    session.url = view.url;
    await session.evaluate('1');
    return session;
  }

  on(method, fn) {
    if (!this.listeners.has(method)) this.listeners.set(method, []);
    this.listeners.get(method).push(fn);
  }

  emit(method, params) {
    for (const fn of this.listeners.get(method) ?? []) fn(params);
  }

  /** Hand the page's captured console lines and errors to the CDP-style listeners. */
  deliver(logs = []) {
    for (const [kind, type, text] of logs) {
      if (kind === 'exception') this.emit('Runtime.exceptionThrown', { exceptionDetails: { text, exception: { description: text } } });
      else this.emit('Runtime.consoleAPICalled', { type, args: [{ type: 'string', value: text }] });
    }
  }

  async run(script, args, timeoutMs) {
    if (this.closed) throw new Error('web view session closed');
    try {
      return await this.driver.execute(script, args, timeoutMs);
    } catch (e) {
      // A dead session or a web view that went away: the scenario attaches again.
      if (e.status === 404 || /no such (window|context|session)|invalid session/i.test(e.message)) this.closed = true;
      throw e;
    }
  }

  /** Evaluate `fn` (a function, run with `args`; or an expression string) and return its JSON value. */
  async evaluate(fn, ...args) {
    const timeoutMs = 30000;
    const id = `p${++pendingSeq}_${Date.now().toString(36)}`;
    const isFn = typeof fn === 'function';
    let r = await this.run(evalScript(isFn ? fn.toString() : null, id), isFn ? args : [fn], timeoutMs);
    this.deliver(r?.logs);
    const end = Date.now() + timeoutMs;
    while (r?.t === 'pending') {
      if (Date.now() > end) throw new Error(`page promise did not settle within ${timeoutMs} ms`);
      await sleep(100);
      r = await this.run(pollScript(id), [], timeoutMs);
      this.deliver(r?.logs);
    }
    if (!r || r.t === 'err') throw new Error(`page threw: ${r?.e ?? 'no result'}`);
    return r.v;
  }

  /** XCUITest tap at page coordinates (points; the web view fills the screen from its top left). */
  async tapNative(x, y) {
    await this.driver.setContext('NATIVE_APP');
    try {
      await this.driver.execute('mobile: tap', [{ x, y }]);
    } finally {
      await this.driver.setContext(this.context);
    }
  }

  async send(method, params = {}, timeoutMs = 15000) {
    switch (method) {
      case 'Runtime.enable':
      case 'Page.enable':
      case 'Page.addScriptToEvaluateOnNewDocument':
        return {};
      case 'Input.dispatchMouseEvent': {
        if (params.type !== 'mouseReleased') return {};
        if (this.nativeTap) {
          await this.tapNative(params.x, params.y);
          return {};
        }
        const hit = await this.evaluate((x, y) => {
          const el = document.elementFromPoint(x, y);
          if (!el) return false;
          const opts = { bubbles: true, cancelable: true, view: window, clientX: x, clientY: y, button: 0 };
          el.dispatchEvent(new MouseEvent('mousedown', opts));
          el.focus?.();
          el.dispatchEvent(new MouseEvent('mouseup', opts));
          el.click();
          const t = el.closest('button, a, input, select, label') ?? el;
          return `${t.tagName.toLowerCase()}${t.id ? `#${t.id}` : ''}${t.disabled ? ' (disabled)' : ''}`;
        }, params.x, params.y);
        if (!hit) throw new Error(`nothing at (${params.x}, ${params.y}) to tap`);
        return { hit };
      }
      case 'Input.insertText': {
        await this.evaluate((text) => {
          const el = document.activeElement;
          if (!el) return;
          if (!document.execCommand('insertText', false, text)) {
            el.value = text;
            el.dispatchEvent(new Event('input', { bubbles: true }));
          }
        }, params.text);
        return {};
      }
      case 'Page.captureScreenshot': {
        if (!this.screenshot) throw new Error('no screenshot source');
        return { data: await this.screenshot(timeoutMs) };
      }
      default:
        throw new Error(`${method} is not available over WebDriver`);
    }
  }

  close() {
    this.closed = true;
  }
}
