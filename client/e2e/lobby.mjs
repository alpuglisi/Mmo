// End-to-end: where the client plays, and how its socket is compressed.
//
// Two evo-servers (A with worlds `a1` and `a2`, B with `b1`; A serves the
// built client) behind one evo-lobby. In headless Chromium:
//   - a new player on A's page with `?lobby=` is sent by the lobby to the
//     busiest world with room (`a1` on A) and opens its socket there;
//   - a player whose lineage lives on B, logging in on A's page, is sent to
//     B and plays in `b1` there (the session then belongs to B);
//   - without a lobby, A's own `/api/assign` keeps a player with their
//     lineage in `a2`;
//   - every socket negotiated permessage-deflate (the browser offers it, the
//     server accepts), read from the DevTools protocol's handshake events.
//
//   npm run build && node e2e/lobby.mjs
//
// Env: EVO_SERVER_BIN, EVO_LOBBY_BIN (default ../target/release/evo-server and
// evo-lobby), HEADED=1 to watch.

import { spawn } from 'node:child_process';
import { createRequire } from 'node:module';
import { existsSync } from 'node:fs';
import { createServer } from 'node:net';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { cappedBrowser } from './frames.mjs';

const require = createRequire(import.meta.url);
const { chromium } = require('playwright');

const here = path.dirname(fileURLToPath(import.meta.url));
const clientDir = path.resolve(here, '..');
const bin = process.env.EVO_SERVER_BIN ?? path.resolve(clientDir, '../target/release/evo-server');
const lobbyBin = process.env.EVO_LOBBY_BIN ?? path.resolve(clientDir, '../target/release/evo-lobby');
const dist = path.join(clientDir, 'dist');

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

// (60 s: three servers and a lobby start at once, and on a loaded machine with other scripts beside it.)
async function waitFor(what, fn, timeoutMs = 60000) {
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

const started = Date.now();
const step = (msg) => console.log(`  ✓ ${msg} [${((Date.now() - started) / 1000).toFixed(0)} s]`);

// The lobby and the backends share an operator token: backends tell only it
// (not anonymous callers) that a name has a lineage there.
const ADMIN = 'e2e-operator-token';

function proc(file, args, log) {
  const p = spawn(file, args, { stdio: ['ignore', 'pipe', 'pipe'], env: { ...process.env, EVO_ADMIN_TOKEN: ADMIN } });
  p.stdout.on('data', (d) => log.push(String(d)));
  p.stderr.on('data', (d) => log.push(String(d)));
  return p;
}

async function post(base, path, body, token) {
  const headers = { 'content-type': 'application/json' };
  if (token) headers.authorization = `Bearer ${token}`;
  const res = await fetch(`${base}${path}`, { method: 'POST', headers, body: JSON.stringify(body) });
  return { status: res.status, body: await res.json().catch(() => null) };
}

const design = { name: 'Lobby test', thermo: 'warm', birth: 'live', litter_size: 2, traits: {}, founders: 6 };

/** Log in (or register) on `url` and return the game socket the page opens, with its handshake headers. */
async function play(browser, url, name, how) {
  const page = await browser.newPage({ viewport: { width: 1100, height: 760 }, deviceScaleFactor: 1 });
  const cdp = await page.context().newCDPSession(page);
  await cdp.send('Network.enable');
  const sockets = new Map();
  cdp.on('Network.webSocketCreated', (e) => sockets.set(e.requestId, { url: e.url }));
  cdp.on('Network.webSocketHandshakeResponseReceived', (e) => {
    const s = sockets.get(e.requestId);
    if (s) s.headers = Object.fromEntries(Object.entries(e.response.headers).map(([k, v]) => [k.toLowerCase(), v]));
  });
  await page.goto(url);
  await page.fill('#login-name', name);
  await page.fill('#login-password', 'a long password');
  await page.click(how === 'register' ? '#register-submit' : '#login-submit');
  const ws = await waitFor(`${name}'s game socket`, () => [...sockets.values()].find((s) => s.url.includes('/ws?') && s.headers));
  await page.close();
  return ws;
}

async function main() {
  for (const f of [bin, lobbyBin, path.join(dist, 'index.html')]) {
    if (!existsSync(f)) throw new Error(`missing ${f} (cargo build --release -p evo-server; npm run build)`);
  }
  const [pa, pb, pl] = [await freePort(), await freePort(), await freePort()];
  const [A, B, L] = [`http://127.0.0.1:${pa}`, `http://127.0.0.1:${pb}`, `http://127.0.0.1:${pl}`];
  const logs = [];
  const common = ['--fast', '--memory', '--size', '48', '--no-wild', '--no-rate-limits'];
  const procs = [
    proc(bin, [...common, '--bind', `127.0.0.1:${pa}`, '--static', dist, '--world', 'a1,capacity=5', '--world', 'a2,seed=2,capacity=5'], logs),
    proc(bin, [...common, '--bind', `127.0.0.1:${pb}`, '--world', 'b1,capacity=5'], logs),
    proc(lobbyBin, ['--bind', `127.0.0.1:${pl}`, '--backend', A, '--backend', B], logs),
  ];
  const browser = cappedBrowser(await chromium.launch({ headless: !process.env.HEADED }));
  try {
    await waitFor('servers', async () => (await fetch(`${A}/api/world`)).ok && (await fetch(`${B}/api/world`)).ok);
    await waitFor('lobby', async () => (await fetch(`${L}/lobby/servers`)).ok);
    step(`servers on ${A} and ${B}, lobby on ${L}`);

    const deflate = (ws) => (ws.headers['sec-websocket-extensions'] ?? '').includes('permessage-deflate');

    // A new player: the lobby fills A's first world.
    const fresh = await play(browser, `${A}/?tour=off&lobby=${encodeURIComponent(L)}`, 'newcomer', 'register');
    const u1 = new URL(fresh.url);
    if (u1.port !== String(pa) || u1.searchParams.get('world') !== 'a1') throw new Error(`newcomer went to ${fresh.url}`);
    if (!deflate(fresh)) throw new Error(`no permessage-deflate: ${JSON.stringify(fresh.headers)}`);
    step(`a new player is sent to a1 on A; the socket is compressed (${fresh.headers['sec-websocket-extensions']})`);

    // A player with a lineage on B, opening A's page: sent to B.
    const reg = await post(B, '/api/register', { name: 'westerner', password: 'a long password' });
    if (reg.status !== 201) throw new Error(`register on B: ${JSON.stringify(reg)}`);
    const found = await post(B, '/api/lineage?world=b1', design, reg.body.token);
    if (found.status !== 201) throw new Error(`found on B: ${JSON.stringify(found)}`);
    // Routed to B; the public answer says no more (no account or lineage).
    await waitFor('the lineage on B to show', async () => {
      const v = await (await fetch(`${L}/lobby/assign?name=westerner`)).json();
      return v.server === B && v.world === 'b1' && v.reason !== 'lineage' && v.has_account === false;
    });
    const west = await play(browser, `${A}/?tour=off&lobby=${encodeURIComponent(L)}`, 'westerner', 'login');
    const u2 = new URL(west.url);
    if (u2.port !== String(pb) || u2.searchParams.get('world') !== 'b1') throw new Error(`westerner went to ${west.url}`);
    if (!deflate(west)) throw new Error('no permessage-deflate on B');
    step('a player with a lineage on B is sent there from A\'s page');

    // No lobby: A's own assignment keeps a lineage in a2.
    const reg2 = await post(A, '/api/register', { name: 'islander', password: 'a long password' });
    const found2 = await post(A, '/api/lineage?world=a2', design, reg2.body.token);
    if (found2.status !== 201) throw new Error(`found on A: ${JSON.stringify(found2)}`);
    await waitFor('the lineage in a2 to show', async () => (await (await fetch(`${A}/api/assign?name=islander`)).json()).world === 'a2');
    const told = await (await fetch(`${A}/api/assign?name=islander`, { headers: { 'x-admin-token': ADMIN } })).json();
    if (told.reason !== 'lineage') throw new Error(`the operator should be told: ${JSON.stringify(told)}`);
    // A fresh browser profile: no lobby remembered from the pages above.
    const plain = cappedBrowser(await chromium.launch({ headless: !process.env.HEADED }));
    try {
      const isl = await play(plain, `${A}/?tour=off`, 'islander', 'login');
      const u3 = new URL(isl.url);
      if (u3.port !== String(pa) || u3.searchParams.get('world') !== 'a2') throw new Error(`islander went to ${isl.url}`);
      step('without a lobby, the server keeps a player with their lineage (a2)');
    } finally {
      await plain.close();
    }
    console.log('lobby e2e passed');
  } catch (e) {
    console.error(logs.join('').slice(-3000));
    throw e;
  } finally {
    await browser.close();
    for (const p of procs) p.kill('SIGTERM');
  }
}

main().catch((e) => {
  console.error(e);
  process.exit(1);
});
