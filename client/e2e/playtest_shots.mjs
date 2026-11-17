// Screenshot helper (not part of the e2e suite): the playtest-1 fixes on a
// real server. A short-year world runs a while so the wild species are
// detected (the builder's interbreeding warning) and a catastrophe strikes
// (the pinned notice). Shots: the design editor's interbreeding and founding
// cards (desktop and iPhone 13), the Lineage tab's population cap row, the
// Genes tab's "Whose genes", the Food layer and the catastrophe notice.
//
//   npm run build && node e2e/playtest_shots.mjs
//   (SCREENSHOT_DIR defaults to docs/screenshots/playtest-fixes; EVO_SERVER_BIN as for smoke.mjs)

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
const shots = path.resolve(process.env.SCREENSHOT_DIR ?? path.join(clientDir, '../docs/screenshots/playtest-fixes'));
mkdirSync(shots, { recursive: true });
const port = 18000 + Math.floor(Math.random() * 2000);
const base = `http://127.0.0.1:${port}`;
// About two seconds a day: catastrophes last a couple of days, so one stays on screen long enough.
const server = spawn(bin, ['--fast', '--memory', '--size', '48', '--seed', '3', '--days-per-year', '8', '--tick-ms', '40',
  '--bind', `127.0.0.1:${port}`, '--static', path.join(clientDir, 'dist')], { stdio: 'ignore' });
const say = (m) => console.log(`  ✓ ${m}`);
const get = async (p) => (await fetch(`${base}${p}`)).json();

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
  await page.fill('#design-name', 'Shot grazers');
}

const browser = await chromium.launch();
try {
  await until('server', async () => (await fetch(`${base}/api/world`)).ok, 15000);
  // Wild species are detected every few days.
  await until('wild species', async () => (await get('/api/species')).species?.length > 0, 60000);
  say('wild species detected');

  // The builder on a phone: the interbreeding warning and the founding hint.
  const phone = await browser.newContext({ ...(({ defaultBrowserType: _d, ...d }) => d)(devices['iPhone 13']) });
  const pp = await phone.newPage();
  await register(pp, `shots_p${Date.now().toString(36)}`);
  await pp.$eval('#design-hybrid', (el) => el.scrollIntoView({ block: 'end' }));
  await pp.screenshot({ path: path.join(shots, 'builder-hybrid-iphone13.png') });
  const tall = await pp.evaluate(() => ({ doc: document.documentElement.scrollHeight, vh: window.innerHeight, w: document.documentElement.scrollWidth, vw: window.innerWidth }));
  say(`phone design page: document ${tall.doc}px for a ${tall.vh}px screen, ${tall.w}px wide for ${tall.vw}px`);
  await phone.close();

  // Desktop: design, found, then the game.
  const ctx = await browser.newContext({ viewport: { width: 1280, height: 860 }, deviceScaleFactor: 2 });
  const page = await ctx.newPage();
  await register(page, `shots_${Date.now().toString(36)}`);
  await page.$eval('#design-hybrid', (el) => el.scrollIntoView({ block: 'start' }));
  await page.locator('#design-hybrid').screenshot({ path: path.join(shots, 'builder-hybrid.png') });
  await page.$eval('#design-food', (el) => el.scrollIntoView({ block: 'center' }));
  await page.locator('#design-food').screenshot({ path: path.join(shots, 'builder-founding-hint.png') });
  say(`builder: "${(await page.textContent('#design-hybrid .hybrid-headline'))?.trim()}"`);
  await page.$eval('.design-body', (el) => { el.scrollTop = 0; });
  await page.click('#design-submit');
  await page.waitForSelector('#world-canvas', { timeout: 45000 });
  await until('own organisms', () => page.evaluate(() => (window.evo?.model.state?.organisms.length ?? 0) > 0), 30000);

  await page.click('#tab-lineage');
  await page.waitForSelector('#lineage-size');
  await page.waitForTimeout(800);
  await page.locator('#panel-lineage').screenshot({ path: path.join(shots, 'lineage-cap.png') });
  say(`lineage: ${(await page.textContent('#lineage-size'))?.trim()} · ${(await page.textContent('#cap-note').catch(() => ''))?.trim().slice(0, 80)}`);

  await page.click('#food-toggle');
  await page.waitForTimeout(1500);
  await page.screenshot({ path: path.join(shots, 'food-layer.png') });
  await page.click('#food-toggle');
  say('food layer');

  // Wait for a catastrophe, then the notice over the map.
  // Not on its last day, so it is still up for the shot.
  await page.waitForFunction(() => {
    const el = document.querySelector('#catastrophe-alert');
    return el && !el.hidden && /days left/.test(el.textContent);
  }, null, { timeout: 300000 });
  await page.waitForTimeout(300);
  await page.screenshot({ path: path.join(shots, 'catastrophe-desktop.png') });
  const cat = (await get('/api/catastrophes')).active ?? { kind: '(just ended)' };
  say(`catastrophe: ${cat.kind}, "${(await page.textContent('#catastrophe-alert'))?.trim().slice(0, 90)}"`);

  // Whose genes, once the lineage has bred with its neighbours a while.
  await page.click('#tab-genetics');
  await page.waitForSelector('#allele-origins', { timeout: 30000 });
  await page.waitForTimeout(1500);
  await page.$eval('#allele-origins', (el) => el.scrollIntoView({ block: 'start' }));
  const box = await page.$eval('#allele-origins', (el) => { const r = el.getBoundingClientRect(); return { x: r.x, y: r.y, width: r.width, height: Math.min(r.height, 860 - r.y) }; });
  await page.screenshot({ path: path.join(shots, 'whose-genes.png'), clip: box });
  say(`whose genes: "${(await page.textContent('#whose-headline'))?.trim()}"`);
  await ctx.close();

  // The notice on a phone (the same world, a second player).
  const cat2 = await until('a catastrophe (phone)', async () => (await get('/api/catastrophes')).active, 240000);
  const ph = await browser.newContext({ ...(({ defaultBrowserType: _d, ...d }) => d)(devices['iPhone 13']) });
  const p2 = await ph.newPage();
  await register(p2, `shots_q${Date.now().toString(36)}`);
  await p2.$eval('.design-body', (el) => { el.scrollTop = 0; }).catch(() => {});
  await p2.click('#design-submit');
  await p2.waitForSelector('#world-canvas', { timeout: 45000 });
  await p2.waitForSelector('#catastrophe-alert:not([hidden])', { timeout: 30000 }).catch(() => {});
  await p2.waitForTimeout(600);
  await p2.screenshot({ path: path.join(shots, 'catastrophe-iphone13.png') });
  say(`phone catastrophe shot (${cat2.kind})`);
  await ph.close();
} finally {
  await browser.close();
  server.kill();
}
