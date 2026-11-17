// End-to-end: founding again after the lineage dies out. Starts its own
// evo-server (fast, in memory, no wild ecosystem) with EVO_E2E_HOOKS=1, which
// mounts the test-only `POST /api/e2e/wipe-out` (kills the caller's lineage
// at once, so the test doesn't wait for a real extinction).
//
// The design screen shows the winter notice (eight-day years keep winter
// near). A new player founds the starting design; a second founding is refused
// while the lineage lives (409 already_founded); the lineage is wiped out;
// the Lineage tab and the inspector say it died out and offer "Start a new
// lineage"; the design screen says it is a new lineage and can go back to
// the map; founding again works (new lineage id, living founders, renown
// kept) and the died-out notice is gone; a third founding is refused again;
// the living lineage is retired from the Lineage tab ("Retire this
// lineage", confirmed) and a third founded.
// Screenshots: e2e/out/refound-*.png.
//
//   npm run build && node e2e/refound.mjs     (env as for smoke.mjs)

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
const UI_MS = 20000;

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

async function waitFor(what, fn, timeoutMs = UI_MS) {
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

function check(cond, msg, detail) {
  if (!cond) throw new Error(`${msg}${detail === undefined ? '' : `: ${JSON.stringify(detail)}`}`);
}

/** `fetch` from the page with the session's token. */
const api = (page, method, url, body) => page.evaluate(async ([method, url, body]) => {
  const s = JSON.parse(localStorage.getItem('evo.session'));
  const sep = url.includes('?') ? '&' : '?';
  const res = await fetch(`${url}${s.world ? `${sep}world=${encodeURIComponent(s.world)}` : ''}`, {
    method,
    headers: { authorization: `Bearer ${s.token}`, 'content-type': 'application/json' },
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  let json = null;
  try {
    json = await res.json();
  } catch {
    /* no body */
  }
  return { status: res.status, json };
}, [method, url, body]);

const lineageNow = (page) => page.evaluate(() => {
  const s = window.evo.model.state;
  return s ? { lineage: s.lineage, population: s.population, balance: s.balance ?? null } : null;
});

async function main() {
  if (!existsSync(bin)) throw new Error(`no server binary at ${bin} (cargo build --release -p evo-server)`);
  if (!existsSync(path.join(dist, 'index.html'))) throw new Error('no client build (npm run build)');
  mkdirSync(shots, { recursive: true });
  const port = await freePort();
  const base = `http://127.0.0.1:${port}`;
  const server = spawn(bin, ['--fast', '--memory', '--size', '64', '--seed', '3', '--no-wild', '--days-per-year', '8', '--bind', `127.0.0.1:${port}`, '--static', dist], {
    stdio: ['ignore', 'pipe', 'pipe'],
    env: { ...process.env, EVO_E2E_HOOKS: '1' },
  });
  let log = '';
  server.stdout.on('data', (d) => (log += d));
  server.stderr.on('data', (d) => (log += d));
  const browser = await chromium.launch({ headless: !process.env.HEADED });
  const errors = [];
  try {
    await waitFor('server', async () => (await fetch(`${base}/api/world`)).ok);
    step(`server up on ${base}`);
    const context = await browser.newContext({ viewport: { width: 1280, height: 800 } });
    const page = await context.newPage();
    page.on('pageerror', (e) => errors.push(e.message));
    await page.goto(`${base}/?tour=off`);
    await page.fill('#login-name', `refound_${Date.now().toString(36)}`);
    await page.fill('#login-password', 'a long password');
    await page.click('#register-submit');
    await page.waitForSelector('#design-name', { timeout: UI_MS });
    // Eight-day years: winter is never more than a few days off, so the season notice shows.
    const season = await page.textContent('#design-season');
    check(/winter/i.test(season) && season.includes('Fur, fat stores and hibernation') && season.includes('start fed'), 'season notice', season);
    step(`the design screen warns about winter: "${season}"`);
    await page.fill('#design-name', 'First try');
    await page.click('#design-submit');
    await page.waitForSelector('#world-canvas', { timeout: UI_MS });
    const first = await waitFor('the first lineage', async () => {
      const s = await lineageNow(page);
      return s && s.lineage !== null && s.population > 0 ? s : null;
    });
    step(`founded lineage #${first.lineage} with ${first.population} organisms`);

    // A second lineage while this one lives: refused.
    const again = await api(page, 'POST', '/api/lineage', { name: 'Too soon', thermo: 'warm', birth: 'live', litter_size: 2, traits: {} });
    check(again.status === 409 && again.json?.code === 'already_founded', 'a second founding while alive should be refused', again);
    step(`a second founding while the lineage lives is refused (409: ${again.json.message})`);

    // The lineage dies out.
    const wiped = await api(page, 'POST', '/api/e2e/wipe-out');
    check(wiped.status === 200 && wiped.json.killed > 0, 'wipe-out hook', wiped);
    await page.click('#tab-lineage');
    await page.waitForSelector('#lineage-extinct', { state: 'visible', timeout: UI_MS });
    const notice = await page.textContent('#lineage-extinct');
    check(notice.includes('died out') && notice.includes('Start a new lineage'), 'died-out notice', notice);
    await page.screenshot({ path: path.join(shots, 'refound-extinct.png') });
    await page.click('#tab-organism');
    await page.waitForSelector('#inspector-extinct-new', { timeout: UI_MS });
    step(`after ${wiped.json.killed} deaths the Lineage tab and the inspector say it died out and offer a new lineage`);

    // The design screen for a new lineage, and back to the map.
    await page.click('#inspector-extinct-new');
    await page.waitForSelector('#design-name', { timeout: UI_MS });
    check((await page.textContent('.topbar')).includes('Design a new lineage'), 'the design screen should say it is a new lineage');
    await page.click('#design-cancel');
    await page.waitForSelector('#world-canvas', { timeout: UI_MS });
    await page.click('#tab-lineage');
    await page.click('#lineage-extinct-new');
    await page.waitForSelector('#design-name', { timeout: UI_MS });
    await page.screenshot({ path: path.join(shots, 'refound-design.png') });
    step('"Start a new lineage" opens the design screen for a new lineage; "Back to the map" returns');

    // Found again.
    await page.fill('#design-name', 'Second try');
    await page.click('#design-submit');
    await page.waitForSelector('#world-canvas', { timeout: UI_MS });
    const second = await waitFor('the second lineage', async () => {
      const s = await lineageNow(page);
      return s && s.lineage !== null && s.lineage !== first.lineage && s.population > 0 ? s : null;
    });
    if (first.balance !== null && second.balance !== null) check(second.balance >= first.balance, 'renown should be kept', { first, second });
    await page.click('#tab-lineage');
    await waitFor('the died-out notice to go', async () => page.locator('#lineage-extinct').isHidden());
    await page.screenshot({ path: path.join(shots, 'refound-again.png') });
    step(`founded again: lineage #${second.lineage} with ${second.population} organisms, renown ${first.balance} → ${second.balance}`);

    const third = await api(page, 'POST', '/api/lineage', { name: 'Third', thermo: 'warm', birth: 'live', litter_size: 2, traits: {} });
    check(third.status === 409 && third.json?.code === 'already_founded', 'a further founding while the new lineage lives should be refused', third);
    step('a further founding while the new lineage lives is refused again');

    // Retire the living lineage from the Lineage tab (no drowning needed),
    // then found a third.
    await page.click('#tab-lineage');
    await page.waitForSelector('#lineage-retire', { state: 'visible', timeout: UI_MS });
    await page.click('#lineage-retire summary');
    page.once('dialog', (d) => void d.accept());
    await page.click('#retire-lineage');
    await page.waitForSelector('#lineage-extinct', { state: 'visible', timeout: UI_MS });
    check(await page.locator('#lineage-retire').isHidden(), 'the retire section should hide once retired');
    const retired = await lineageNow(page);
    check(retired && retired.lineage === second.lineage && retired.population === 0, 'retired lineage', retired);
    await page.screenshot({ path: path.join(shots, 'refound-retired.png') });
    await page.click('#lineage-extinct-new');
    await page.waitForSelector('#design-name', { timeout: UI_MS });
    await page.fill('#design-name', 'Third try');
    await page.click('#design-submit');
    await page.waitForSelector('#world-canvas', { timeout: UI_MS });
    const fresh = await waitFor('the third lineage', async () => {
      const s = await lineageNow(page);
      return s && s.lineage !== null && s.lineage !== second.lineage && s.population > 0 ? s : null;
    });
    step(`retired lineage #${second.lineage} from the Lineage tab and founded #${fresh.lineage}`);
    check(errors.length === 0, 'page errors', errors);
    await context.close();
    console.log('refound e2e passed');
  } catch (e) {
    if (process.env.SERVER_LOG_FILE) (await import('node:fs')).writeFileSync(process.env.SERVER_LOG_FILE, log);
    throw e;
  } finally {
    await browser.close();
    server.kill();
  }
}

main().catch((e) => {
  console.error(e);
  process.exit(1);
});
