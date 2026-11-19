// End-to-end: the spectator view and the share cards. In headless Chromium,
// with a fresh evo-server like smoke.mjs:
//
//  1. a player founds a lineage (through the API);
//  2. a logged-out page opens its watch link (`?watch=main&lineage=N`): no
//     login screen, the followed lineage's name, owner, numbers and
//     portrait, the map drawn with its discs, the largest lineages and the
//     clock; the spectator data carries nothing private;
//  3. "Share card" saves the portrait card as a PNG (headless Chromium has
//     no Web Share API: the download fallback), 1080 × 1350;
//  4. logged in, the Family tab's Share bar saves the tree card (1080 ×
//     1920) and the portrait card.
//
// No uncaught page errors throughout.
//
//   npm run build && node e2e/spectate.mjs     (env as for smoke.mjs)

import { spawn } from 'node:child_process';
import { createRequire } from 'node:module';
import { existsSync, mkdirSync, readFileSync } from 'node:fs';
import { createServer } from 'node:net';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { cappedBrowser } from './frames.mjs';
import { stamp } from './names.mjs';

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

function step(msg) {
  console.log(`  ✓ ${msg}`);
}

function check(ok, msg, detail) {
  if (!ok) throw new Error(`${msg}${detail === undefined ? '' : `: ${JSON.stringify(detail)}`}`);
}

async function api(base, method, p, body, token) {
  const res = await fetch(base + p, {
    method,
    headers: { 'content-type': 'application/json', ...(token ? { authorization: `Bearer ${token}` } : {}) },
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  const text = await res.text();
  if (!res.ok) throw new Error(`${method} ${p}: ${res.status} ${text}`);
  return JSON.parse(text);
}

/** Width and height of a PNG file (its IHDR). */
function pngSize(file) {
  const b = readFileSync(file);
  check(b.toString('ascii', 1, 4) === 'PNG', `${file} is not a PNG`);
  return [b.readUInt32BE(16), b.readUInt32BE(20)];
}

async function main() {
  if (!existsSync(bin)) throw new Error(`no server binary at ${bin}; run cargo build --release -p evo-server`);
  if (!existsSync(path.join(dist, 'index.html'))) throw new Error('no client/dist; run npm run build');
  mkdirSync(shots, { recursive: true });
  const port = await freePort();
  const base = `http://127.0.0.1:${port}`;
  const server = spawn(bin, ['--fast', '--memory', '--size', '64', '--seed', '13', '--no-rate-limits', '--no-push', '--bind', `127.0.0.1:${port}`, '--static', dist], {
    stdio: ['ignore', 'ignore', 'pipe'],
  });
  let log = '';
  server.stderr.on('data', (d) => (log = (log + d).slice(-4000)));
  const browser = cappedBrowser(await chromium.launch({ headless: !process.env.HEADED }));
  const errors = [];
  try {
    for (let i = 0; ; i++) {
      try {
        if ((await fetch(`${base}/api/world`)).ok) break;
      } catch {
        /* not up yet */
      }
      if (i > 100) throw new Error('server did not start');
      await new Promise((r) => setTimeout(r, 100));
    }
    const name = `spec_${stamp()}`;
    const auth = await api(base, 'POST', '/api/register', { name, password: 'a long password' });
    const design = { name: 'Watched striders', thermo: 'warm', birth: 'live', litter_size: 2, founders: 20, traits: { body_size: 0.4, fur: 0.5, coloration: 0.8 } };
    const { lineage } = await api(base, 'POST', '/api/lineage', design, auth.token);
    step(`${name} founded lineage ${lineage}`);

    // The data: public, coarse, nothing private.
    const data = await api(base, 'GET', '/api/spectate');
    const mine = data.lineages.find((l) => l.lineage === lineage);
    check(mine && mine.owner === name && mine.alive === 20 && mine.look, 'the lineage in /api/spectate', mine);
    const text = JSON.stringify(data);
    for (const p of ['"id":', '"x":', '"genes"', '"pregnant"', '"token"']) check(!text.includes(p), `/api/spectate shows ${p}`);
    step(`/api/spectate (no login): ${data.lineages.length} lineages on ${data.cell}-tile cells, nothing private`);

    // A logged-out page on the watch link.
    const context = await browser.newContext({ viewport: { width: 1280, height: 720 }, deviceScaleFactor: 1, acceptDownloads: true });
    const page = await context.newPage();
    page.on('pageerror', (e) => errors.push(e.message));
    await page.goto(`${base}/?watch=main&lineage=${lineage}`);
    await page.waitForFunction(() => window.evoSpectator?.last(), null, { timeout: 30000 });
    check(!(await page.$('#login-form, .login')), 'the watch link showed a login screen');
    await page.waitForFunction((n) => document.querySelector('#sp-name')?.textContent === n, design.name, { timeout: 15000 });
    const shown = await page.evaluate(() => ({
      owner: document.querySelector('.sp-owner')?.textContent,
      facts: document.querySelector('#sp-facts')?.textContent,
      clock: document.querySelector('#sp-clock')?.textContent,
      rows: document.querySelectorAll('#sp-list li').length,
      portrait: document.querySelector('#sp-portrait')?.getAttribute('aria-label') ?? '',
    }));
    check(shown.owner === `Lineage of ${name}` && /alive/.test(shown.facts) && /^Year \d+/.test(shown.clock) && shown.rows >= 2 && shown.portrait.length > 10, 'the followed lineage', shown);
    // The map is drawn: many colours in the canvas (once a frame has come;
    // the e2e pages run at a capped frame rate).
    const countColors = () => page.evaluate(() => {
      const c = document.querySelector('#sp-map');
      if (!c || c.width < 16 || c.height < 16) return 0;
      const g = c.getContext('2d');
      const seen = new Set();
      for (let i = 1; i < 16; i++) for (let j = 1; j < 16; j++) seen.add(g.getImageData(Math.floor((c.width * i) / 16), Math.floor((c.height * j) / 16), 1, 1).data.join(','));
      return seen.size;
    });
    let colors = 0;
    for (let i = 0; i < 100 && colors <= 12; i++) {
      colors = await countColors();
      if (colors <= 12) await new Promise((r) => setTimeout(r, 100));
    }
    check(colors > 12, 'the spectator map looks blank', colors);
    await page.screenshot({ path: path.join(shots, 'spectate-desktop.png') });
    step(`watch link: ${design.name} followed (${shown.facts}), ${shown.rows} lineages listed, map drawn (${colors} colours), no login`);

    const [dl] = await Promise.all([page.waitForEvent('download', { timeout: 15000 }), page.click('#sp-share')]);
    const cardFile = path.join(shots, 'spectate-card.png');
    await dl.saveAs(cardFile);
    check(dl.suggestedFilename() === 'watched-striders-portrait.png', 'card file name', dl.suggestedFilename());
    check(pngSize(cardFile).join('x') === '1080x1350', 'portrait card size', pngSize(cardFile));
    step('Share card: no Web Share API here, so the 1080 × 1350 portrait PNG is downloaded');
    await context.close();

    // Logged in: the Family tab's Share bar.
    const game = await browser.newContext({ viewport: { width: 1280, height: 800 }, deviceScaleFactor: 1, acceptDownloads: true });
    await game.addInitScript((s) => localStorage.setItem('evo.session', JSON.stringify(s)), auth);
    const gp = await game.newPage();
    gp.on('pageerror', (e) => errors.push(e.message));
    await gp.goto(`${base}/?tour=off`);
    await gp.waitForSelector('#tab-family', { timeout: 30000 });
    await gp.click('#tab-family');
    await gp.waitForSelector('#share-tree', { timeout: 30000 });
    const [tree] = await Promise.all([gp.waitForEvent('download', { timeout: 15000 }), gp.click('#share-tree')]);
    const treeFile = path.join(shots, 'family-tree-card.png');
    await tree.saveAs(treeFile);
    check(pngSize(treeFile).join('x') === '1080x1920', 'tree card size', pngSize(treeFile));
    const [por] = await Promise.all([gp.waitForEvent('download', { timeout: 15000 }), gp.click('#share-portrait')]);
    const porFile = path.join(shots, 'family-portrait-card.png');
    await por.saveAs(porFile);
    check(pngSize(porFile).join('x') === '1080x1350', 'family portrait card size', pngSize(porFile));
    check(await gp.$('#share-watch'), 'no Copy watch link button');
    step('Family tab: Share tree (1080 × 1920) and Share portrait (1080 × 1350) download their cards; a watch link is offered');
    await game.close();

    check(errors.length === 0, 'page errors', errors);
    console.log('spectate e2e passed');
  } catch (e) {
    console.error(log);
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
