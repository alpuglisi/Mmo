// End-to-end: the first-session onboarding. Starts its own evo-server like
// smoke.mjs. The other e2e tests open the client with `?tour=off`; this one
// doesn't.
//
// Desktop: a new player sees the tour after founding, with focus on Next;
// it walks a few steps, Skip closes it, the map takes clicks again and a
// reload doesn't bring it back; Help opens a modal dialog (Esc closes it and
// focus returns), Replay reopens the tour and Esc skips it; a first death
// shows a one-time hint that doesn't come back, even after a reload.
// Phone (Pixel 7): the tour fits the screen at every step without sideways
// scrolling, Finish completes it, the tab bar works and a reload keeps it
// closed.
//
//   npm run build && node e2e/onboarding.mjs     (env as for smoke.mjs)

import { spawn } from 'node:child_process';
import { createRequire } from 'node:module';
import { existsSync, mkdirSync } from 'node:fs';
import { createServer } from 'node:net';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const require = createRequire(import.meta.url);
const { chromium, devices } = require('playwright');

const here = path.dirname(fileURLToPath(import.meta.url));
const clientDir = path.resolve(here, '..');
const bin = process.env.EVO_SERVER_BIN ?? path.resolve(clientDir, '../target/release/evo-server');
const dist = path.join(clientDir, 'dist');
const shots = path.resolve(process.env.SCREENSHOT_DIR ?? path.join(here, 'out'));
const STEPS = 9;
// How long a UI reaction may take. Generous: on a loaded machine the page,
// the browser and the server's ticks all slow down; a waitFor returns as
// soon as its condition holds, so this costs nothing when things are quick.
const UI_MS = 15000;

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

function device(name) {
  const { defaultBrowserType: _ignored, ...d } = devices[name];
  return d;
}

async function shot(page, name) {
  for (let attempt = 0; ; attempt++) {
    try {
      return await page.screenshot({ path: path.join(shots, name) });
    } catch (e) {
      if (attempt >= 3) throw e;
      await page.waitForTimeout(500);
    }
  }
}

async function register(page, base, name) {
  await page.goto(base);
  await page.fill('#login-name', name);
  await page.fill('#login-password', 'a long password');
  await page.click('#register-submit');
  await page.waitForSelector('#design-name', { timeout: 45000 });
  await page.fill('#design-name', `${name} line`);
  await page.click('#design-submit');
  await page.waitForSelector('#world-canvas', { timeout: 45000 });
  await waitFor('own organisms', () => page.evaluate(() => (window.evo?.model.state?.organisms.length ?? 0) > 0));
}

/** Reload (the session is remembered) and wait for the game screen to settle. */
async function reload(page) {
  await page.reload();
  await page.waitForSelector('#world-canvas', { timeout: 45000 });
  await page.waitForFunction(() => document.querySelector('#conn-state')?.textContent === 'connected', null, { timeout: 45000 });
  await page.waitForTimeout(1200);
}

/** The tour card's step and count, its box against the viewport, focus and sideways overflow. */
const tourState = (page) => page.evaluate(() => {
  const card = document.querySelector('#tour');
  if (!card) return null;
  const r = card.getBoundingClientRect();
  const ring = document.querySelector('#tour-ring');
  return {
    step: card.dataset.step, count: document.querySelector('#tour-count').textContent, role: card.getAttribute('role'),
    labelled: document.getElementById(card.getAttribute('aria-labelledby'))?.textContent ?? '',
    box: { left: r.left, top: r.top, right: r.right, bottom: r.bottom }, vw: document.documentElement.clientWidth, vh: window.innerHeight,
    ring: ring && !ring.hidden ? ring.getBoundingClientRect().toJSON() : null,
    focus: document.activeElement?.id ?? '', docW: document.documentElement.scrollWidth,
  };
});

function checkFits(t, tag) {
  if (t.box.left < 0 || t.box.top < 0 || t.box.right > t.vw + 0.5 || t.box.bottom > t.vh + 0.5) throw new Error(`${tag}: tour card off screen at ${t.step}: ${JSON.stringify(t)}`);
  if (t.docW > t.vw + 1) throw new Error(`${tag}: page scrolls sideways at ${t.step} (${t.docW} > ${t.vw})`);
  if (t.ring && (t.ring.left < 0 || t.ring.right > t.vw + 0.5)) throw new Error(`${tag}: highlight off screen at ${t.step}: ${JSON.stringify(t.ring)}`);
}

/** Nothing left over the map: the canvas center takes the pointer. */
async function mapTakesInput(page, tag) {
  const ok = await page.evaluate(() => {
    const c = document.querySelector('#world-canvas').getBoundingClientRect();
    const el = document.elementFromPoint(c.left + c.width / 2, c.top + c.height / 2);
    return el?.id === 'world-canvas' && !document.querySelector('#tour, #tour-ring, #help');
  });
  if (!ok) throw new Error(`${tag}: something still covers the map`);
}

async function desktopChecks(browser, base, errors) {
  const context = await browser.newContext({ viewport: { width: 1280, height: 800 }, deviceScaleFactor: 1 });
  const page = await context.newPage();
  page.on('pageerror', (e) => errors.push(`desktop: ${e.message}`));
  await register(page, base, `ob_desk_${Date.now().toString(36)}`);

  // The tour opens by itself after founding, as a labelled non-modal dialog with focus on Next.
  const first = await waitFor('tour', () => tourState(page), 20000);
  if (first.step !== 'welcome' || first.count !== `1 of ${STEPS}` || first.role !== 'dialog' || !first.labelled.startsWith('Welcome')) throw new Error(`unexpected first step ${JSON.stringify(first)}`);
  await waitFor('focus on Next', async () => (await tourState(page)).focus === 'tour-next', UI_MS);
  checkFits(first, 'desktop');
  await shot(page, 'onboarding-desktop-welcome.png');
  step('desktop: the tour opens after founding, focus on Next');

  // Next walks the steps (pointing at the map toolbar, then the Lineage tab, then the inspector); Back goes back.
  const seen = [];
  for (let k = 0; k < 4; k++) {
    await page.click('#tour-next');
    const t = await waitFor('next step', async () => { const s = await tourState(page); return s && s.count === `${k + 2} of ${STEPS}` && s; }, UI_MS);
    checkFits(t, 'desktop');
    if (!t.ring) throw new Error(`no highlight at ${t.step}`);
    seen.push(t.step);
  }
  if (seen.join() !== 'map,lineage,inspector,possess') throw new Error(`unexpected steps ${seen}`);
  if (!(await page.isVisible('#panel-organism'))) throw new Error('the inspector step did not open the Organism tab');
  await shot(page, 'onboarding-desktop-possess.png');
  await page.keyboard.press('ArrowLeft');
  await waitFor('back', async () => (await tourState(page))?.step === 'inspector', UI_MS);
  step(`desktop: Next and Back walk the steps (${seen.join(', ')})`);

  // Skip closes it and leaves nothing over the map; the game takes input.
  await page.click('#tour-skip');
  await waitFor('tour closed', async () => (await tourState(page)) === null, UI_MS);
  await mapTakesInput(page, 'desktop');
  await page.click('#tab-seasons');
  await waitFor('seasons tab', () => page.isVisible('#panel-seasons'), UI_MS);
  step('desktop: Skip closes the tour; the map and tabs take input again');

  await reload(page);
  if (await tourState(page)) throw new Error('the skipped tour came back after a reload');
  step('desktop: a reload does not bring the tour back');

  // Help: a modal dialog; Esc closes it and focus returns to the Help button.
  await page.click('#help-btn');
  await page.waitForSelector('#help-dialog', { timeout: UI_MS });
  const help = await page.evaluate(() => ({ modal: document.querySelector('#help-dialog').getAttribute('aria-modal'), focus: document.activeElement?.id }));
  if (help.modal !== 'true' || help.focus !== 'help-replay') throw new Error(`help dialog ${JSON.stringify(help)}`);
  await page.keyboard.press('Tab');
  await page.keyboard.press('Tab');
  await page.keyboard.press('Tab');
  if (!(await page.evaluate(() => document.querySelector('#help-dialog').contains(document.activeElement)))) throw new Error('focus left the help dialog');
  await page.keyboard.press('Escape');
  await waitFor('help closed', () => page.evaluate(() => !document.querySelector('#help') && document.activeElement?.id === 'help-btn'), UI_MS);
  step('desktop: Help is a modal dialog that keeps focus, Esc closes it and focus returns');

  // Replay from Help reopens the tour at the first step; Esc skips it.
  await page.click('#help-btn');
  await page.click('#help-replay');
  const replay = await waitFor('replayed tour', () => tourState(page), UI_MS);
  if (replay.step !== 'welcome' || await page.$('#help')) throw new Error(`replay ${JSON.stringify(replay)}`);
  await page.keyboard.press('Escape');
  await waitFor('tour closed by Esc', async () => (await tourState(page)) === null, UI_MS);
  await mapTakesInput(page, 'desktop');
  await reload(page);
  if (await tourState(page)) throw new Error('the tour came back after a replay and a reload');
  step('desktop: Help replays the tour; Esc skips it; it stays closed after a reload');

  // A first death shows a one-time hint; dismissed, it never comes back.
  // The living world may already have shown it: Help's "Show tips again" forgets the tips seen.
  await page.click('#help-btn');
  await page.click('#help-reset-tips');
  await page.keyboard.press('Escape');
  await page.waitForSelector('#help', { state: 'detached', timeout: UI_MS });
  const death = (text) => page.evaluate((text) => window.evo.model.addFeed([{ kind: 'death', text, tick: 1e9 + text.length }]), text);
  await death('Your #1 starved (e2e).');
  // The living world may have called for other hints first (a pregnancy): dismiss those.
  await waitFor('death hint', async () => {
    const shown = await page.evaluate(() => document.querySelector('#onboarding-hint')?.dataset.hint ?? null);
    if (shown && shown !== 'death') await page.click('#hint-dismiss');
    return shown === 'death';
  }, UI_MS);
  await shot(page, 'onboarding-desktop-hint.png');
  await page.click('#hint-show');
  // The death hint closes and Genetics opens. The next due tip (the living
  // world may have earned one meanwhile, more likely on a slow machine) may
  // take the hint's place at once, so wait for the death hint to go, not
  // for no hint at all.
  await waitFor('genetics tab', () => page.evaluate(() => document.querySelector('#onboarding-hint')?.dataset.hint !== 'death' && !document.querySelector('#panel-genetics').hidden), UI_MS);
  await death('Your #2 starved again (e2e).');
  await page.waitForTimeout(600);
  if (await page.$('#onboarding-hint[data-hint="death"]')) throw new Error('the death hint showed twice');
  await reload(page);
  await death('Your #3 starved after a reload (e2e).');
  await page.waitForTimeout(600);
  if (await page.$('#onboarding-hint[data-hint="death"]')) throw new Error('the death hint came back after a reload');
  step('desktop: after "Show tips again", a death shows a one-time hint ("Show me" opens Genetics); it does not come back');
  await context.close();
}

async function phoneChecks(browser, base, errors) {
  const context = await browser.newContext({ ...device('Pixel 7') });
  const page = await context.newPage();
  page.on('pageerror', (e) => errors.push(`phone: ${e.message}`));
  await register(page, base, `ob_ph_${Date.now().toString(36)}`);

  const steps = [];
  for (let k = 0; k < STEPS; k++) {
    const t = await waitFor(`step ${k + 1}`, async () => { const s = await tourState(page); return s && s.count === `${k + 1} of ${STEPS}` && s; }, 20000);
    await page.waitForTimeout(200); // the highlight's slide
    checkFits(await tourState(page), 'phone');
    steps.push(t.step);
    if (t.step === 'lineage') await shot(page, 'onboarding-phone-lineage.png');
    if (k === STEPS - 1 && (await page.textContent('#tour-next')) !== 'Finish') throw new Error('no Finish on the last step');
    await page.tap('#tour-next');
  }
  await waitFor('tour finished', async () => (await tourState(page)) === null, UI_MS);
  step(`phone: every step fits ${page.viewportSize().width} px without sideways scrolling (${steps.join(', ')}); Finish completes it`);

  // The tab bar opens the sheet: nothing left blocking.
  await page.tap('#tab-lineage');
  await waitFor('sheet open', () => page.evaluate(() => document.querySelector('.game').classList.contains('sheet-open')), UI_MS);
  await page.tap('#tab-lineage');
  await waitFor('sheet closed', () => page.evaluate(() => !document.querySelector('.game').classList.contains('sheet-open')), UI_MS);
  await mapTakesInput(page, 'phone');
  const help = await page.locator('#help-btn').boundingBox();
  if (!help || help.x + help.width > page.viewportSize().width) throw new Error(`help button off screen ${JSON.stringify(help)}`);
  await reload(page);
  if (await tourState(page)) throw new Error('the completed tour came back after a reload');
  step('phone: the tab bar works after the tour, the Help button fits, and a reload keeps the tour closed');
  await context.close();
}

async function main() {
  if (!existsSync(bin)) throw new Error(`no server binary at ${bin}; run cargo build --release -p evo-server`);
  if (!existsSync(path.join(dist, 'index.html'))) throw new Error('no client/dist; run npm run build');
  mkdirSync(shots, { recursive: true });
  const port = await freePort();
  const base = `http://127.0.0.1:${port}`;
  // No wild predators: the lineage outlives the test on a slow machine.
  const server = spawn(bin, ['--fast', '--memory', '--size', '64', '--seed', '12', '--days-per-year', '480', '--no-wild', '--bind', `127.0.0.1:${port}`, '--static', dist], {
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
    await desktopChecks(browser, base, errors);
    await phoneChecks(browser, base, errors);
    if (errors.length) throw new Error(`page errors: ${errors.join('; ')}`);
    console.log(`onboarding test passed; screenshots in ${shots}`);
  } catch (e) {
    for (const c of browser.contexts()) for (const [k, p] of c.pages().entries()) await p.screenshot({ path: path.join(shots, `onboarding-failure-${k}.png`) }).catch(() => {});
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
