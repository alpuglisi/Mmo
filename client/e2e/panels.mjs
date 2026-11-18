// End-to-end: panels that keep their input and the possession controls
// (playtest 1, client fixes C). Starts its own evo-server (fast, in memory,
// no wild ecosystem).
//
// A tab clicked while the game screen loads opens once it is ready, with no
// error; the Matings "My adult" choice survives state pushes (the same option
// element); the partner list offers own adults; the matings history endpoint
// answers and Recent outcomes renders after a reload; while possessing, the
// possession bar shows Release, and a click on another animal walks there
// instead of selecting it. Screenshot: e2e/out/panels-possess-bar.png.
//
//   npm run build && node e2e/panels.mjs     (env as for smoke.mjs)

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


/** Record command results (`window.__results`), as smoke.mjs does. */
const recordResults = (page) => page.evaluate(() => {
  window.__results = [];
  const s = window.evo.socket;
  const orig = s.send.bind(s);
  s.send = async (m) => {
    const r = await orig(m);
    window.__results.push({ type: m.type, ok: r.ok, error: r.error, organism: m.organism });
    return r;
  };
});

async function main() {
  if (!existsSync(bin)) throw new Error(`no server binary at ${bin} (cargo build --release -p evo-server)`);
  if (!existsSync(path.join(dist, 'index.html'))) throw new Error('no client build (npm run build)');
  mkdirSync(shots, { recursive: true });
  const port = await freePort();
  const base = `http://127.0.0.1:${port}`;
  const server = spawn(bin, ['--fast', '--memory', '--size', '64', '--seed', '3', '--no-wild', '--bind', `127.0.0.1:${port}`, '--static', dist], {
    stdio: ['ignore', 'pipe', 'pipe'],
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
    await page.fill('#login-name', `panels_${Date.now().toString(36)}`);
    await page.fill('#login-password', 'a long password');
    await page.click('#register-submit');
    await page.waitForSelector('#design-name', { timeout: UI_MS });
    await page.fill('#design-name', 'Panel testers');
    await page.click('#design-submit');

    // A tab clicked while the game screen is still loading: no error, and it takes.
    await page.waitForSelector('#tab-market', { timeout: UI_MS });
    await page.click('#tab-market');
    await page.waitForSelector('#world-canvas', { timeout: UI_MS });
    await waitFor('the screen to finish loading', () => page.evaluate(() => !document.querySelector('.game.starting')));
    await waitFor('the Market tab to stay picked', () => page.evaluate(() => document.querySelector('#tab-market')?.classList.contains('active') && !document.querySelector('#panel-market')?.hidden));
    check(errors.length === 0, 'page errors while loading', errors);
    step('a tab clicked during loading opens once the screen is ready, with no error');

    // Matings: "My adult" is a searchable picker (no huge dropdown); its
    // choice survives state pushes.
    await page.click('#tab-matings');
    await waitFor('adults in the form', () => page.evaluate(() => window.evo.model.state.organisms.filter((o) => o.matured).length >= 4));
    await page.click('#mating-mine .picker-button');
    await page.waitForSelector('#mating-mine .picker-panel:not([hidden])', { timeout: UI_MS });
    // A female (founders are half female; no predators in this world): her
    // partners are males, who are never pregnant, so the partner list is
    // never empty whatever the world is doing.
    await page.click('#mating-mine .chip:has-text("females")');
    const rows = await waitFor('female rows', () => page.evaluate(() => {
      const r = [...document.querySelectorAll('#mating-mine .picker-row')].map((b) => b.textContent);
      return r.length > 0 && r.every((t) => / female /.test(t)) ? r : null;
    }));
    check(rows.length <= 50, 'the picker shows at most 50 rows', rows.length);
    const picked = await page.evaluate(() => {
      const r = [...document.querySelectorAll('#mating-mine .picker-row')];
      return r[r.length - 1].textContent.match(/^#(\d+)/)[1];
    });
    // Search by id finds it.
    await page.fill('#mating-mine .picker-search', picked);
    await waitFor('the searched row', () => page.evaluate((id) => {
      const r = [...document.querySelectorAll('#mating-mine .picker-row')];
      return r.length >= 1 && r[0].textContent.startsWith(`#${id} `);
    }, picked));
    await page.click(`#mating-mine .picker-row:has-text("#${picked} ")`);
    await waitFor('the picker to close on a choice', () => page.evaluate(() => document.querySelector('#mating-mine .picker-panel').hidden));
    await page.evaluate(() => { window.__pickerButton = document.querySelector('#mating-mine .picker-button'); });
    check(await page.evaluate(() => document.querySelector('#mating-mine').value) === picked, 'the picker value');
    const ticks0 = await page.evaluate(() => window.evo.model.state.clock.tick);
    await page.waitForTimeout(4000);
    const after = await page.evaluate(() => {
      const el = document.querySelector('#mating-mine');
      return { value: el.value, same: el.querySelector('.picker-button') === window.__pickerButton, text: el.querySelector('.picker-button').textContent, tick: window.evo.model.state.clock.tick };
    });
    check(after.tick > ticks0 + 20, 'the world should have moved on', { ticks0, after });
    check(after.value === picked && after.same && after.text.startsWith(`#${picked} female`), 'the picked adult should stay picked, in the same button', { picked, after });
    await page.click('#mating-partner-pick .picker-button');
    const partnerList = await waitFor('an own male to pick as partner', () => page.evaluate(() => {
      const opts = [...document.querySelectorAll('#mating-partner-pick .picker-row')];
      const free = opts.filter((o) => !o.disabled && o.textContent.includes(' male (yours)') && / tiles/.test(o.textContent));
      return free.length > 0 ? opts.map((o) => o.textContent) : null;
    }));
    await page.screenshot({ path: path.join(shots, 'panels-partner-picker.png') });
    const firstMale = partnerList.find((t) => t.includes(' male (yours)')).match(/^#(\d+)/)[1];
    await page.click(`#mating-partner-pick .picker-row:has-text("#${firstMale} male")`);
    check(await page.inputValue('#mating-partner') === firstMale, 'a partner picked fills the id field');
    await waitFor('the pair preview', async () => (await page.getAttribute('#mating-compat', 'data-pair')) === `${picked}:${firstMale}`);
    step(`"My adult" picked #${picked} (a female) by filter and id search, kept over ${after.tick - ticks0} ticks of state pushes; partners listed (${partnerList.length}, her own males among them, nearest first); #${firstMale} picked and previewed`);

    // History: the endpoint answers, and Recent outcomes shows it after a reload.
    const hist = await page.evaluate(async () => {
      const s = JSON.parse(localStorage.getItem('evo.session'));
      const r = await fetch('/api/matings/history', { headers: { authorization: `Bearer ${s.token}` } });
      return { status: r.status, body: await r.json() };
    });
    check(hist.status === 200 && Array.isArray(hist.body), 'matings history endpoint', hist);
    await page.reload();
    await page.waitForSelector('#world-canvas', { timeout: UI_MS });
    await waitFor('the screen to finish loading', () => page.evaluate(() => !document.querySelector('.game.starting')));
    await page.click('#tab-matings');
    await waitFor('recent outcomes', () => page.evaluate(() => document.querySelector('#closed-offers')?.previousElementSibling?.textContent === 'None yet.'
      || document.querySelectorAll('#closed-offers .offer').length > 0));
    step(`matings history: ${hist.body.length} record(s) from the server; Recent outcomes rendered after a reload`);

    // Who may send me requests: auto-decline and blocking (server `mating_prefs`).
    await page.click('#mating-prefs summary');
    await waitFor('the blocked list', async () => (await page.textContent('#mating-blocked')).includes('Nobody'));
    await page.check('#mating-auto-decline');
    const prefsOn = await waitFor('auto-decline saved', () => page.evaluate(async () => {
      const r = await window.evo.socket.send({ type: 'mating_prefs' });
      return r.ok && r.data.auto_decline === true ? r.data : null;
    }));
    await page.uncheck('#mating-auto-decline');
    await waitFor('auto-decline off', () => page.evaluate(async () => (await window.evo.socket.send({ type: 'mating_prefs' })).data?.auto_decline === false));
    step(`Mates has request settings: auto-decline saved on the server (${JSON.stringify(prefsOn)}) and turned off again; nobody blocked`);

    // Lineage: "Retire this lineage" sits at the top, above the stats and the organism list.
    await page.click('#tab-lineage');
    await page.waitForSelector('#lineage-retire', { state: 'visible', timeout: UI_MS });
    const retireFirst = await page.evaluate(() => {
      const r = document.querySelector('#lineage-retire');
      return ['#lineage-stats', '#organism-list'].every((q) => r.compareDocumentPosition(document.querySelector(q)) & Node.DOCUMENT_POSITION_FOLLOWING);
    });
    check(retireFirst, '"Retire this lineage" should come before the stats and the organism list');
    step('"Retire this lineage" is at the top of the Lineage tab');

    // Market: the shop buttons and the pickers stay the same elements over
    // several refreshes (updated in place, so a click is never lost).
    await page.click('#tab-market');
    await page.waitForSelector('#shop-reinforce', { state: 'attached', timeout: UI_MS });
    await page.waitForSelector('#market-mine .picker-button', { timeout: UI_MS });
    await page.evaluate(() => {
      window.__marketEls = ['#shop-reinforce', '#shop-slot', '#market-mine .picker-button', '#market-list-organism .picker-button', '#market-refresh'].map((q) => document.querySelector(q));
    });
    await page.click('#market-refresh');
    await page.waitForTimeout(7000);
    const kept = await page.evaluate(() => window.__marketEls.map((el) => el?.isConnected === true));
    check(kept.every(Boolean), 'the Market buttons should survive refreshes', kept);
    await page.click('#market-list-organism .picker-button');
    const listRows = await waitFor('adults to list', () => page.evaluate(() => document.querySelectorAll('#market-list-organism .picker-row').length || null));
    await page.click('#market-list-organism .picker-row');
    check(/^\d+$/.test(await page.evaluate(() => document.querySelector('#market-list-organism').value)), 'an adult picked to list');
    step(`Market: Reinforcements, Buy slot and the pickers stay attached over 7 s of refreshes; the list picker offers ${listRows} adults`);

    // Possession: the bar with Release; a click on another animal walks there instead of selecting it.
    await recordResults(page);
    const pair = await page.evaluate(() => {
      const own = window.evo.model.state.organisms.filter((o) => o.matured);
      return { a: own[0].id, b: own[1].id };
    });
    await page.evaluate((id) => window.evo.socket.send({ type: 'possess', organism: id }), pair.a);
    await page.waitForSelector('#possess-bar:not([hidden])', { timeout: UI_MS });
    const barText = await page.textContent('#possess-bar');
    check(barText.includes(`#${pair.a}`) && barText.includes('Release'), 'possession bar', barText);
    await page.evaluate(() => window.evo.screen().view.setFollow(false));
    const target = await waitFor('the other animal on screen', () => page.evaluate((id) => {
      const v = window.evo.screen().view;
      const m = v.markers.get(id);
      if (!m) return null;
      v.centerOn(m.x, m.y);
      const p = v.worldToScreen(m.x, m.y);
      return { x: p.x, y: p.y };
    }, pair.b));
    await page.waitForTimeout(200);
    const box = await page.locator('#world-canvas').boundingBox();
    const selectedBefore = await page.evaluate(() => window.evo.model.selected);
    await page.mouse.click(box.x + target.x, box.y + target.y);
    await waitFor('a walk order', () => page.evaluate(() => window.__results.some((r) => r.type === 'order_move')));
    const selectedAfter = await page.evaluate(() => window.evo.model.selected);
    check(selectedAfter === selectedBefore, 'a click while possessing should walk, not select', { selectedBefore, selectedAfter, b: pair.b });
    await page.screenshot({ path: path.join(shots, 'panels-possess-bar.png') });
    await page.click('#possess-release');
    await page.waitForSelector('#possess-bar', { state: 'hidden', timeout: UI_MS });
    step(`possessing #${pair.a}: the bar shows Release; a click on #${pair.b} walked there; Release from the bar`);

    check(errors.length === 0, 'page errors', errors);
    await context.close();
    console.log('panels e2e passed');
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
