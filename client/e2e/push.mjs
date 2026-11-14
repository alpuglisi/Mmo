// End-to-end: push notification settings. Starts its own evo-server like
// smoke.mjs, with --push-allow-any-endpoint so a local endpoint is accepted.
//
// Headless Chromium can grant the notifications permission, but a real
// `pushManager.subscribe()` would register with the browser's push service
// (FCM) over the network, which tests must never contact. So the page's
// PushManager is stubbed: `subscribe()` returns a subscription with a real
// P-256 key (WebCrypto) and an endpoint on 127.0.0.1's discard port, and
// the test checks everything from the settings card to the server's stored
// subscription. The service worker itself registers for real.
//
// Checks: the card shows in the Genetics tab with every push kind; Enable
// asks for nothing more (permission granted), subscribes with the server's
// VAPID key and POSTs the subscription, which the server then lists; a kind
// switched off is saved to the account; a reload finds the device still
// subscribed; Turn off unsubscribes it on the server. A browser that blocks
// notifications, and one without the Push API, get an explanation and no
// button to press.
//
//   npm run build && node e2e/push.mjs     (env as for smoke.mjs)

import { spawn } from 'node:child_process';
import { createRequire } from 'node:module';
import { existsSync, mkdirSync } from 'node:fs';
import { createServer } from 'node:net';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const require = createRequire(import.meta.url);
const { chromium } = require('playwright');

const here = path.dirname(fileURLToPath(import.meta.url));
const clientDir = path.resolve(here, '..');
const bin = process.env.EVO_SERVER_BIN ?? path.resolve(clientDir, '../target/release/evo-server');
const dist = path.join(clientDir, 'dist');
const shots = path.resolve(process.env.SCREENSHOT_DIR ?? path.join(here, 'out'));

function freePort() {
  return new Promise((resolve, reject) => {
    const s = createServer();
    s.listen(0, '127.0.0.1', () => {
      const { port } = s.address();
      s.close(() => resolve(port));
    });
    s.on('error', reject);
  });
}

async function waitFor(what, fn, timeoutMs = 20000) {
  const end = Date.now() + timeoutMs;
  let last;
  while (Date.now() < end) {
    try {
      last = await fn();
      if (last) return last;
    } catch (e) {
      last = e;
    }
    await new Promise((r) => setTimeout(r, 100));
  }
  throw new Error(`timed out waiting for ${what} (last: ${last})`);
}

function step(msg) {
  console.log(`  ✓ ${msg}`);
}

/** Replace the Push API's network side (see the file comment). The fake
 * subscription outlives a reload, as a real one does (in localStorage). */
function stubPushManager() {
  const KEY = 'e2e.push-subscription';
  const b64url = (buf) => btoa(String.fromCharCode(...new Uint8Array(buf))).replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/, '');
  const unb64 = (s) => Uint8Array.from(atob(s.replace(/-/g, '+').replace(/_/g, '/')), (c) => c.charCodeAt(0));
  const make = (saved) => ({
    endpoint: saved.endpoint,
    expirationTime: null,
    options: { userVisibleOnly: true, applicationServerKey: unb64(saved.serverKey).buffer },
    toJSON: () => ({ endpoint: saved.endpoint, expirationTime: null, keys: { p256dh: saved.p256dh, auth: saved.auth } }),
    unsubscribe: async () => {
      localStorage.removeItem(KEY);
      return true;
    },
  });
  window.__pushCalls = [];
  PushManager.prototype.getSubscription = async function () {
    const raw = localStorage.getItem(KEY);
    return raw ? make(JSON.parse(raw)) : null;
  };
  PushManager.prototype.subscribe = async function (options) {
    const serverKey = new Uint8Array(options.applicationServerKey);
    window.__pushCalls.push({ userVisibleOnly: options.userVisibleOnly, keyLength: serverKey.length });
    const pair = await crypto.subtle.generateKey({ name: 'ECDH', namedCurve: 'P-256' }, true, ['deriveBits']);
    const saved = {
      endpoint: `http://127.0.0.1:9/push/e2e-${Math.random().toString(36).slice(2)}`,
      p256dh: b64url(await crypto.subtle.exportKey('raw', pair.publicKey)),
      auth: b64url(crypto.getRandomValues(new Uint8Array(16))),
      serverKey: b64url(serverKey),
    };
    localStorage.setItem(KEY, JSON.stringify(saved));
    return make(saved);
  };
}

async function register(page, base, name) {
  await page.goto(`${base}/?tour=off`);
  await page.fill('#login-name', name);
  await page.fill('#login-password', 'a long password');
  await page.click('#register-submit');
  await page.waitForSelector('#design-name', { timeout: 15000 });
  await page.fill('#design-name', `${name} line`);
  await page.click('#design-submit');
  await page.waitForSelector('#world-canvas', { timeout: 15000 });
}

async function login(page, base, name) {
  await page.goto(`${base}/?tour=off`);
  await page.fill('#login-name', name);
  await page.fill('#login-password', 'a long password');
  await page.click('#login-submit');
  await page.waitForSelector('#world-canvas', { timeout: 15000 });
}

async function openSettings(page) {
  await page.click('#tab-genetics');
  await page.waitForSelector('#push-settings', { state: 'attached', timeout: 20000 });
  await page.$eval('#push-settings', (d) => {
    d.open = true;
    d.scrollIntoView();
  });
}

const status = (page) => page.$eval('#push-status', (el) => el.dataset.status).catch(() => null);

async function api(base, token, method, p, body) {
  const res = await fetch(`${base}${p}`, {
    method,
    headers: { authorization: `Bearer ${token}`, ...(body ? { 'content-type': 'application/json' } : {}) },
    body: body ? JSON.stringify(body) : undefined,
  });
  return res.json();
}

async function main() {
  if (!existsSync(bin)) throw new Error(`no server binary at ${bin}; run cargo build --release -p evo-server`);
  if (!existsSync(path.join(dist, 'index.html'))) throw new Error('no client/dist; run npm run build');
  if (!existsSync(path.join(dist, 'sw.js'))) throw new Error('no client/dist/sw.js; run npm run build');
  mkdirSync(shots, { recursive: true });
  const port = await freePort();
  const base = `http://127.0.0.1:${port}`;
  const server = spawn(bin, ['--fast', '--memory', '--size', '64', '--seed', '5', '--no-wild', '--push-allow-any-endpoint', '--bind', `127.0.0.1:${port}`, '--static', dist], {
    stdio: ['ignore', 'pipe', 'pipe'],
  });
  let log = '';
  server.stdout.on('data', (d) => (log += d));
  server.stderr.on('data', (d) => (log += d));
  // The full Chromium in new headless mode: the headless shell reports
  // Notification.permission as "denied" whatever is granted. Without it,
  // fall back to the shell and pretend the grant worked (see grantedQuirk).
  let browser;
  let shell = false;
  try {
    browser = await chromium.launch({ headless: !process.env.HEADED, channel: 'chromium' });
  } catch {
    browser = await chromium.launch({ headless: !process.env.HEADED });
    shell = true;
  }
  const errors = [];
  try {
    await waitFor('server', async () => (await fetch(`${base}/api/world`)).ok);
    step(`server up on ${base}`);
    const key = await (await fetch(`${base}/api/push/key`)).json();
    if (!key.enabled || typeof key.public_key !== 'string') throw new Error(`push key: ${JSON.stringify(key)}`);

    // --- A browser that allows notifications.
    const context = await browser.newContext({ viewport: { width: 1280, height: 800 }, deviceScaleFactor: 1 });
    await context.grantPermissions(['notifications'], { origin: base });
    await context.addInitScript(stubPushManager);
    if (shell) {
      console.log('  (headless shell: Notification.permission follows the granted permission by a stub)');
      await context.addInitScript(() => {
        Object.defineProperty(Notification, 'permission', { get: () => 'granted' });
        Notification.requestPermission = async () => 'granted';
      });
    }
    const page = await context.newPage();
    page.on('pageerror', (e) => errors.push(e.message));
    const name = `push_${Date.now().toString(36)}`;
    await register(page, base, name);
    const token = await page.evaluate(() => JSON.parse(localStorage.getItem('evo.session')).token);
    await waitFor('the service worker', () => page.evaluate(() => navigator.serviceWorker.getRegistration().then((r) => !!r?.active)), 30000);
    step('registered and founded; the service worker is active');

    await openSettings(page);
    await waitFor('status off', async () => (await status(page)) === 'off');
    const kinds = await page.$$eval('#push-kinds input[data-push-kind]', (els) => els.map((e) => [e.dataset.pushKind, e.checked]));
    if (kinds.length !== key.kinds.length || !kinds.every(([, on]) => on)) throw new Error(`push kinds: ${JSON.stringify(kinds)}`);
    if (await page.$eval('#push-toggle', (b) => b.disabled)) throw new Error('Enable is disabled');
    step(`the settings card shows in the Genetics tab with ${kinds.length} kinds, all on`);

    const [req] = await Promise.all([
      page.waitForRequest((r) => r.url().endsWith('/api/push/subscribe') && r.method() === 'POST'),
      page.waitForResponse((r) => r.url().endsWith('/api/push/subscribe') && r.status() === 200),
      page.click('#push-toggle'),
    ]);
    const sent = req.postDataJSON();
    await waitFor('status on', async () => (await status(page)) === 'on');
    const calls = await page.evaluate(() => window.__pushCalls);
    if (calls.length !== 1 || calls[0].userVisibleOnly !== true || calls[0].keyLength !== 65) throw new Error(`subscribe calls: ${JSON.stringify(calls)}`);
    const listed = (await api(base, token, 'GET', '/api/push/subscriptions')).subscriptions;
    if (listed.length !== 1 || listed[0].endpoint !== sent.endpoint || !sent.keys?.p256dh || !/Chrome/.test(listed[0].label ?? '')) {
      throw new Error(`server lists ${JSON.stringify(listed)} after sending ${JSON.stringify(sent)}`);
    }
    await page.locator('#push-settings').screenshot({ path: path.join(shots, 'push-settings.png') });
    step(`Enable subscribed with the server key and the server stored it (${listed[0].label})`);

    await Promise.all([
      page.waitForResponse((r) => r.url().endsWith('/api/push/prefs') && r.request().method() === 'POST' && r.status() === 200),
      page.click('#push-kinds input[data-push-kind="season"]'),
    ]);
    const prefs = await api(base, token, 'GET', '/api/push/prefs');
    if (JSON.stringify(prefs) !== JSON.stringify({ off: ['season'] })) throw new Error(`prefs ${JSON.stringify(prefs)}`);
    step('switching a kind off saves it to the account');

    await page.reload();
    await page.waitForSelector('#world-canvas', { timeout: 15000 });
    await openSettings(page);
    await waitFor('status on after a reload', async () => (await status(page)) === 'on');
    if (await page.$eval('#push-kinds input[data-push-kind="season"]', (e) => e.checked)) throw new Error('season is on again after a reload');
    step('a reload finds the device subscribed and the kind still off');

    await Promise.all([
      page.waitForResponse((r) => r.url().endsWith('/api/push/unsubscribe') && r.status() === 200),
      page.click('#push-toggle'),
    ]);
    await waitFor('status off again', async () => (await status(page)) === 'off');
    const after = (await api(base, token, 'GET', '/api/push/subscriptions')).subscriptions;
    if (after.length !== 0) throw new Error(`still subscribed: ${JSON.stringify(after)}`);
    step('Turn off unsubscribed the device on the server');
    await context.close();

    // --- Notifications blocked; then no Push API at all.
    const blocked = await browser.newContext({ viewport: { width: 1280, height: 800 } });
    await blocked.addInitScript(() => {
      Object.defineProperty(Notification, 'permission', { get: () => 'denied' });
      Notification.requestPermission = async () => 'denied';
    });
    const p2 = await blocked.newPage();
    p2.on('pageerror', (e) => errors.push(e.message));
    await login(p2, base, name);
    await openSettings(p2);
    await waitFor('status denied', async () => (await status(p2)) === 'denied');
    if (!(await p2.$eval('#push-toggle', (b) => b.disabled))) throw new Error('Enable is offered while notifications are blocked');
    if (!/blocked/.test(await p2.textContent('#push-status'))) throw new Error('no explanation for blocked notifications');
    await blocked.close();

    const bare = await browser.newContext({ viewport: { width: 1280, height: 800 } });
    await bare.addInitScript(() => {
      delete window.PushManager;
    });
    const p3 = await bare.newPage();
    p3.on('pageerror', (e) => errors.push(e.message));
    await login(p3, base, name);
    await openSettings(p3);
    await waitFor('status unsupported', async () => (await status(p3)) === 'unsupported');
    if (!(await p3.$eval('#push-toggle', (b) => b.disabled))) throw new Error('Enable is offered without the Push API');
    await bare.close();
    step('blocked notifications and a browser without the Push API get an explanation and no button');

    if (errors.length) throw new Error(`page errors: ${errors.join('; ')}`);
    console.log(`push test passed; screenshot in ${shots}`);
  } catch (e) {
    for (const c of browser.contexts()) for (const [k, p] of c.pages().entries()) await p.screenshot({ path: path.join(shots, `push-failure-${k}.png`) }).catch(() => {});
    console.error(log.split('\n').slice(-20).join('\n'));
    throw e;
  } finally {
    await browser.close();
    server.kill('SIGINT');
  }
}

main().catch((e) => {
  console.error(e);
  process.exit(1);
});
