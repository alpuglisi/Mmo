// Screenshot helper (not part of the e2e suite): the playtest-2 information
// fixes on a real server. A short-year world (a day ≈ 2 s) with the wild
// ecosystem; one player on a desktop, one on a phone.
//
// Shots (docs/screenshots/round3/ by default): the builder's interbreeding
// card with its confidence note; the Lineage tab's soft cap, grace and season
// weight; an attack order's result alert; the deaths chart by cause; the Ranks
// tab's legacy explanation; Help's "How the world works"; the phone top bar;
// the drought notice when a drought comes (skipped if none does in time).
//
//   npm run build && node e2e/round3_shots.mjs   (EVO_SERVER_BIN as for smoke.mjs)

import { spawn } from 'node:child_process';
import { createRequire } from 'node:module';
import { mkdirSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const require = createRequire(import.meta.url);
const { chromium, devices } = require('playwright');
const here = path.dirname(fileURLToPath(import.meta.url));
const clientDir = path.resolve(here, '..');
const bin = process.env.EVO_SERVER_BIN ?? path.resolve(clientDir, '../target/release/evo-server');
const shots = path.resolve(process.env.SCREENSHOT_DIR ?? path.join(clientDir, '../docs/screenshots/round3'));
mkdirSync(shots, { recursive: true });
const port = 18000 + Math.floor(Math.random() * 2000);
const base = `http://127.0.0.1:${port}`;
const server = spawn(bin, ['--fast', '--memory', '--size', '48', '--seed', '3', '--days-per-year', '8', '--tick-ms', '40',
  '--bind', `127.0.0.1:${port}`, '--static', path.join(clientDir, 'dist')], { stdio: 'ignore' });
const say = (m) => console.log(`  ✓ ${m}`);
const get = async (p) => (await fetch(`${base}${p}`)).json();
const phoneDevice = (({ defaultBrowserType: _d, ...d }) => d)(devices['iPhone 13']);

async function until(what, fn, ms) {
  const end = Date.now() + ms;
  while (Date.now() < end) {
    try {
      const v = await fn();
      if (v) return v;
    } catch { /* not yet */ }
    await new Promise((r) => setTimeout(r, 250));
  }
  throw new Error(`timed out waiting for ${what}`);
}

async function register(page, name) {
  await page.goto(`${base}/?tour=off`);
  await page.fill('#login-name', name);
  await page.fill('#login-password', 'a long password');
  await page.click('#register-submit');
  await page.waitForSelector('#design-name', { timeout: 45000 });
  await page.waitForSelector('#design-hybrid:not([hidden]) h2', { timeout: 15000 });
  await page.fill('#design-name', `${name.slice(0, 6)} herd`);
}

async function found(page) {
  await page.click('#design-submit');
  await page.waitForSelector('#world-canvas', { timeout: 45000 });
  await until('own organisms', () => page.evaluate(() => (window.evo?.model.state?.organisms.length ?? 0) > 0), 30000);
}

/** A shot of one element (a selector or a locator), clipped to the viewport. */
async function clip(page, sel, file) {
  const loc = typeof sel === 'string' ? page.locator(sel).first() : sel.first();
  await loc.evaluate((el) => el.scrollIntoView({ block: 'start' }));
  await page.waitForTimeout(200);
  const vh = page.viewportSize().height;
  const box = await loc.evaluate((el) => { const r = el.getBoundingClientRect(); return { x: r.x, y: Math.max(0, r.y), width: r.width, height: r.height }; });
  box.height = Math.min(box.height, vh - box.y);
  await page.screenshot({ path: path.join(shots, file), clip: box });
}

const browser = await chromium.launch();
try {
  await until('server', async () => (await fetch(`${base}/api/world`)).ok, 15000);
  await until('wild species', async () => (await get('/api/species')).species?.length > 0, 60000);
  say('wild species detected');

  const ctx = await browser.newContext({ viewport: { width: 1280, height: 860 }, deviceScaleFactor: 2 });
  const page = await ctx.newPage();
  await register(page, `r3_${Date.now().toString(36)}`);
  await clip(page, '#design-hybrid', 'builder-interbreeding.png');
  say(`builder: "${(await page.textContent('#design-hybrid .hybrid-headline'))?.trim()}" + "${(await page.textContent('#design-hybrid-confidence'))?.slice(0, 60)}…"`);
  await page.$eval('.design-body', (el) => { el.scrollTop = 0; });
  await found(page);

  // The Lineage tab: soft cap, grace, season weight, the legacy explanation.
  await page.click('#tab-lineage');
  await page.waitForSelector('#grace-left', { timeout: 15000 });
  await page.waitForTimeout(500);
  await clip(page, '#panel-lineage', 'lineage-soft-cap-grace.png');
  say(`lineage: ${(await page.textContent('#lineage-size'))?.trim()} · grace "${(await page.textContent('#grace-left'))?.slice(0, 50)}…" · weight "${(await page.textContent('#ramp-weight'))?.slice(0, 40)}…"`);

  // An attack order on a wild animal, and how it ended.
  const result = await until('an attack result alert', async () => {
    const r = await page.evaluate(async () => {
      const evo = window.evo;
      const s = evo.model.state;
      const feed = evo.model.feed ?? [];
      const done = feed.find((f) => /^Attack over:/.test(f.text));
      if (done) return { done: done.text };
      let mine = s.organisms.find((o) => o.possessed);
      if (!mine) {
        mine = s.organisms.find((o) => o.matured);
        if (!mine) return null;
        await evo.socket.send({ type: 'possess', organism: mine.id });
        return null;
      }
      const prey = s.nearby.filter((o) => o.owner === 0).sort((a, b) => Math.hypot(a.x - mine.x, a.y - mine.y) - Math.hypot(b.x - mine.x, b.y - mine.y));
      for (const p of prey.slice(0, 6)) {
        const r = await evo.socket.send({ type: 'possess_attack', organism: mine.id, target: p.id });
        if (r.ok) return null;
      }
      // Nothing in reach: walk on.
      await evo.socket.send({ type: 'order_move', organism: mine.id, x: mine.x + 6, y: mine.y + 3 });
      return null;
    });
    return r?.done ?? null;
  }, 120000);
  // The result toasts over the map (attack alerts toast by default).
  const toast = page.locator('.toasts .toast', { hasText: 'Attack over:' });
  if (await toast.count()) await clip(page, page.locator('.toasts'), 'attack-result-toast.png');
  else {
    await page.click('#tab-genetics');
    await page.waitForTimeout(400);
    await clip(page, page.locator('#alerts-feed li', { hasText: 'Attack over:' }), 'attack-result-toast.png');
  }
  say(`attack: "${result}"`);


  // Ranks: how the legacy crown is scored.
  await page.evaluate(() => { document.scrollingElement.scrollTop = 0; for (const el of document.querySelectorAll('.game-body, .stage-wrap')) el.scrollTop = 0; });
  await page.click('#tab-ranks');
  await page.waitForSelector('#season-legacy-explain summary', { state: 'visible', timeout: 30000 }).catch(async (e) => {
    console.log('DEBUG', await page.evaluate(() => { const p = document.querySelector('#panel-ranks'); const r = p.getBoundingClientRect(); return { hidden: p.hidden, r: [r.x, r.y, r.width, r.height], tab: document.querySelector('.tab.active')?.id }; }));
    throw e;
  });
  await page.click('#season-legacy-explain summary');
  await page.waitForTimeout(1500); // a standings push redraws the part: it must stay open
  if (!(await page.$eval('#season-legacy-explain', (d) => d.open))) throw new Error('the legacy explanation folded on a redraw');
  await clip(page, '#panel-ranks', 'ranks-legacy-explain.png');
  say('ranks: legacy scoring explained');

  // Help: how the world works.
  await page.click('#help-btn');
  await page.waitForSelector('#help-topics');
  for (const d of await page.$$('#help-topics details')) await d.evaluate((el) => { el.open = true; });
  await page.locator('#help-dialog').screenshot({ path: path.join(shots, 'help-topics.png') });
  await page.click('#help-close');
  say('help: attacks, grace, territories, toxins, droughts, the soft cap');

  // A second player on a phone: the top bar, and (if a drought comes) the drought notice.
  const ph = await browser.newContext(phoneDevice);
  const p2 = await ph.newPage();
  await register(p2, `r3p_${Date.now().toString(36)}`);
  await p2.$eval('.design-body', (el) => { el.scrollTop = 0; }).catch(() => {});
  await found(p2);
  await p2.waitForTimeout(800);
  await p2.screenshot({ path: path.join(shots, 'phone-top-bar.png'), clip: { x: 0, y: 0, width: 390, height: 120 } });
  say('phone top bar');
  try {
    await p2.waitForSelector('#drought-alert:not([hidden])', { timeout: 240000 });
    await p2.waitForTimeout(400);
    await p2.screenshot({ path: path.join(shots, 'drought-notice-iphone13.png') });
    say(`drought: "${(await p2.textContent('#drought-alert .cat-title'))?.trim()}"`);
    // The desktop page in the other context: best effort (its state can lag while it is not in front).
    await page.bringToFront();
    if (await page.waitForSelector('#drought-alert:not([hidden])', { timeout: 15000 }).then(() => true, () => false)) {
      await page.screenshot({ path: path.join(shots, 'drought-notice-desktop.png') });
    } else {
      console.log('  – the desktop page did not show the drought in time; phone shot only');
    }
  } catch {
    console.log('  – no drought of 45% or more came in time; no drought shot');
  }
  await ph.close();
  // The deaths chart by cause, now that days have passed (a drought and winters).
  await page.bringToFront();
  await page.click('#tab-genetics');
  await page.waitForSelector('#allele-origins', { timeout: 30000 });
  await page.waitForTimeout(6000);
  await page.locator('.chart-card', { hasText: 'Births and deaths' }).first().screenshot({ path: path.join(shots, 'deaths-by-cause.png') });
  say(`genes tab: ${(await page.locator('.chart-card', { hasText: 'Births and deaths' }).first().locator('.legend').textContent().catch(() => ''))}`);
  await ctx.close();
} finally {
  await browser.close();
  server.kill();
}
