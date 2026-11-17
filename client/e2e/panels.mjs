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

    // Matings: the "My adult" choice survives state pushes (same option element, same value).
    await page.click('#tab-matings');
    await waitFor('adults in the form', () => page.evaluate(() => document.querySelectorAll('#mating-mine option').length >= 4));
    // A female (founders are half female; no predators in this world): her
    // partners are males, who are never pregnant, so the partner list is
    // never empty whatever the world is doing.
    const picked = await page.evaluate(() => {
      const sel = document.querySelector('#mating-mine');
      const females = [...sel.options].filter((o) => / female /.test(o.textContent));
      const opt = females[females.length - 1];
      sel.value = opt.value;
      sel.dispatchEvent(new Event('change', { bubbles: true }));
      window.__pickedOption = opt;
      return opt.value;
    });
    const ticks0 = await page.evaluate(() => window.evo.model.state.clock.tick);
    await page.waitForTimeout(4000);
    const after = await page.evaluate(() => {
      const sel = document.querySelector('#mating-mine');
      return { value: sel.value, same: [...sel.options].includes(window.__pickedOption), tick: window.evo.model.state.clock.tick };
    });
    check(after.tick > ticks0 + 20, 'the world should have moved on', { ticks0, after });
    check(after.value === picked && after.same, 'the picked adult should stay picked, in the same option element', { picked, after });
    const partnerList = await waitFor('an own male to pick as partner', () => page.evaluate(() => {
      const opts = [...document.querySelectorAll('#mating-partner-pick option')];
      const free = opts.filter((o) => !o.disabled && o.value && o.textContent.includes(' male (yours)'));
      return free.length > 0 ? opts.map((o) => o.textContent) : null;
    }));
    step(`"My adult" kept #${picked} (a female) over ${after.tick - ticks0} ticks of state pushes; partners listed (${partnerList.length - 1}, her own males among them)`);

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
