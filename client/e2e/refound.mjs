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
import { stamp } from './names.mjs';

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

/** Found the design on screen; a re-founding with fewer founders says so in
 * the app's dialog first. Returns the renown the notice says it cost (0 if none). */
async function found(page) {
  await page.click('#design-submit');
  const which = await waitFor('the map or the reduced-founding notice', async () => {
    if (await page.$('#founded-reduced-ok')) return 'reduced';
    if (await page.$('#world-canvas')) return 'map';
    return null;
  });
  let cost = 0;
  if (which === 'reduced') {
    const text = (await page.textContent('#founded-reduced')).replace(/\s+/g, ' ');
    cost = Number(/It cost (\d+) renown/.exec(text)?.[1] ?? 0);
    step(`re-founding notice: "${text.slice(0, 90)}…"`);
    check(!(await page.$('#founded-reduced-cancel')), 'the notice after founding has one OK button, no Cancel');
    await page.click('#founded-reduced-ok');
  }
  await page.waitForSelector('#world-canvas', { timeout: UI_MS });
  return cost;
}

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
    const login = `refound_${stamp()}`;
    await page.fill('#login-name', login);
    await page.fill('#login-password', 'a long password');
    await page.click('#register-submit');
    await page.waitForSelector('#design-name', { timeout: UI_MS });
    // Eight-day years: winter is never more than a few days off, so the season notice shows.
    const season = await page.textContent('#design-season');
    check(/winter/i.test(season) && season.includes('Fur, fat stores and hibernation') && season.includes('start fed'), 'season notice', season);
    step(`the design screen warns about winter: "${season}"`);
    await page.fill('#design-name', 'First try');
    await found(page);
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
    // Said over the map whichever tab is open (playtest 6: only the Lineage tab said so).
    await page.waitForSelector('#extinct-alert', { state: 'visible', timeout: UI_MS });
    const banner = await page.textContent('#extinct-alert');
    check(banner.includes('Your lineage died out') && banner.includes('Start a new lineage'), 'died-out banner over the map', banner);
    step('the map says the lineage died out, with "Start a new lineage"');
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
    // The banner's button opens it too.
    await page.waitForSelector('#extinct-alert-new', { state: 'visible', timeout: UI_MS });
    await page.click('#extinct-alert-new');
    await page.waitForSelector('#design-name', { timeout: UI_MS });
    await page.click('#design-cancel');
    await page.waitForSelector('#world-canvas', { timeout: UI_MS });
    await page.click('#tab-lineage');
    await page.click('#lineage-extinct-new');
    await page.waitForSelector('#design-name', { timeout: UI_MS });
    await page.screenshot({ path: path.join(shots, 'refound-design.png') });
    step('"Start a new lineage" opens the design screen for a new lineage; "Back to the map" returns');

    // Found again.
    await page.fill('#design-name', 'Second try');
    // A founders count other than the starter's: the next design screen must start from it.
    const lastFounders = Number(await page.inputValue('#design-founders')) - 2;
    await page.fill('#design-founders', String(lastFounders));
    const refoundCost = await found(page);
    const second = await waitFor('the second lineage', async () => {
      const s = await lineageNow(page);
      return s && s.lineage !== null && s.lineage !== first.lineage && s.population > 0 ? s : null;
    });
    if (first.balance !== null && second.balance !== null) check(second.balance >= first.balance - refoundCost, 'renown should be kept, less the re-founding cost', { first, second, refoundCost });
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
    await page.click('#retire-lineage');
    // The app's own dialog (not the browser's confirm), with the server's window.
    await page.waitForSelector('#retire-confirm', { timeout: UI_MS });
    const confirmText = await page.textContent('#retire-confirm');
    check(/re-founding within \d+ days \(about \d+ real minutes?\) founds half your founders \(rounded up(, at most \d+)?\) and costs \d+ renown after a retirement/.test(confirmText) && !confirmText.includes('ranked season'), 'the retire dialog states the server\'s rule', confirmText);
    await page.click('#retire-confirm-ok');
    // Retired: the design screen opens by itself (playtest 10: it didn't).
    await page.waitForSelector('#design-name', { timeout: UI_MS });
    const retired = await lineageNow(page);
    check(retired && retired.lineage === second.lineage && retired.population === 0, 'retired lineage', retired);
    await page.screenshot({ path: path.join(shots, 'refound-retired.png') });
    step('retiring opens the design screen by itself');

    // A new browser (nothing saved locally): the design screen still says the
    // lineage was retired (the server's lineage stats) and starts from the
    // last design (the server's GET /api/design/last).
    {
      const other = await browser.newContext({ viewport: { width: 1280, height: 800 } });
      const p2 = await other.newPage();
      p2.on('pageerror', (e) => errors.push(e.message));
      await p2.goto(`${base}/?tour=off`);
      await p2.fill('#login-name', login);
      await p2.fill('#login-password', 'a long password');
      await p2.click('#login-submit');
      await p2.waitForSelector('#world-canvas', { timeout: UI_MS });
      await p2.click('#tab-lineage');
      await p2.click('#lineage-extinct-new');
      await p2.waitForSelector('#design-name', { timeout: UI_MS });
      const head2 = await p2.textContent('.topbar');
      check(head2.includes('You retired your last lineage') && !head2.includes('died out'), 'a new browser should say the lineage was retired', head2);
      await p2.waitForSelector('#design-prefilled', { timeout: UI_MS });
      check(await p2.inputValue('#design-founders') === String(lastFounders), 'the new browser should start from the last design', await p2.inputValue('#design-founders'));
      await other.close();
      step('a new browser says the lineage was retired and starts from the last design (server-side)');
    }
    const header = await page.textContent('.topbar');
    check(header.includes('You retired your last lineage') && !header.includes('died out'), 'the design screen should say the lineage was retired', header);
    // A warm-blooded design that abandons its eggs: the server's egg warning, verbatim.
    await page.check('input[name="birth"][value="abandoned_eggs"]');
    const eggWarn = await waitFor('the egg warning', async () => {
      const t = await page.textContent('#design-warnings [data-warning="eggs"]').catch(() => null);
      return t && t.includes('24 °C') ? t : null;
    });
    check(await page.locator('#design-warnings [data-warning="eggs"]').count() === 1, 'one egg warning');
    await page.check('input[name="birth"][value="incubated_eggs"]');
    await waitFor('the egg warning to go', async () => (await page.locator('#design-warnings [data-warning="eggs"]').count()) === 0);
    step(`warm-blooded abandoned eggs show the server's warning: "${eggWarn.slice(0, 80)}…"`);
    // A name the server refuses: the reason shows under the field and Found is disabled until it changes.
    await page.fill('#design-name', 'Server Admin');
    await page.waitForSelector('#design-name-warning:not([hidden])', { timeout: UI_MS });
    const nameWarn = await page.textContent('#design-name-warning');
    check(await page.isDisabled('#design-submit'), 'Found should be disabled for a refused name', nameWarn);
    check(!/bad_name|playtest/.test(nameWarn), 'the name warning reads as words', nameWarn);
    await page.fill('#design-name', 'Third try');
    await page.waitForSelector('#design-name-warning', { state: 'hidden', timeout: UI_MS });
    step(`a refused name is explained inline with Found disabled: "${nameWarn}"`);
    await found(page);
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
