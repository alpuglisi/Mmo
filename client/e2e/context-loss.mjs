// End-to-end: the map survives a lost WebGL context. In headless Chromium,
// with a fresh evo-server like smoke.mjs: register, found a lineage, then
//
//  1. lose the canvas's context (WEBGL_lose_context) and restore it: state
//     frames keep arriving and the panels follow them while it is lost, and
//     after the restore the marker textures are rebuilt and the map is drawn
//     again (more than 6 distinct colors in the canvas's own pixels);
//  2. lose it and never restore it: after a few seconds the view makes a new
//     renderer on a new canvas and draws the map there.
//
// No uncaught page errors throughout.
//
//   npm run build && node e2e/context-loss.mjs     (env as for smoke.mjs)

import { spawn } from 'node:child_process';
import { createRequire } from 'node:module';
import { existsSync, mkdirSync } from 'node:fs';
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

async function waitFor(what, fn, timeoutMs = 30000) {
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
  throw new Error(`timed out waiting for ${what} (last: ${JSON.stringify(last) ?? last})`);
}

function step(msg) {
  console.log(`  ✓ ${msg}`);
}

/**
 * Distinct colors on a 15×15 grid of the map canvas's own pixels (as
 * e2e/android/scenario.mjs counts them): copied in an animation frame queued
 * after the renderer's, since the drawing buffer is not preserved.
 */
const mapColors = (page) => page.evaluate(async () => {
  const c = document.querySelector('#world-canvas');
  if (!c || c.width < 16 || c.height < 16) return 0;
  const cv = document.createElement('canvas');
  cv.width = c.width;
  cv.height = c.height;
  const g = cv.getContext('2d', { willReadFrequently: true });
  let best = 0;
  for (let frame = 0; frame < 10 && best <= 6; frame++) {
    await new Promise((r) => requestAnimationFrame(r));
    g.clearRect(0, 0, cv.width, cv.height);
    g.drawImage(c, 0, 0);
    const seen = new Set();
    for (let i = 1; i < 16; i++) {
      for (let j = 1; j < 16; j++) {
        const [r, gg, b] = g.getImageData(Math.floor((cv.width * i) / 16), Math.floor((cv.height * j) / 16), 1, 1).data;
        seen.add(`${r >> 3},${gg >> 3},${b >> 3}`);
      }
    }
    best = Math.max(best, seen.size);
  }
  return best;
});

const stats = (page) => page.evaluate(() => {
  const v = window.evo.screen().view;
  return { ...v.contextStats(), ...v.spriteStats(), tick: window.evo.model.state?.clock.tick ?? -1, conn: document.querySelector('#conn-state')?.textContent ?? '' };
});

/** Lose the map canvas's WebGL context (and keep the extension to restore it with). */
const loseContext = (page) => page.evaluate(() => {
  const c = document.querySelector('#world-canvas');
  const gl = c.getContext('webgl2') || c.getContext('webgl');
  const ext = gl?.getExtension('WEBGL_lose_context');
  if (!ext) return false;
  window.__loseExt = ext;
  c.dataset.lost = '1';
  ext.loseContext();
  return true;
});

async function main() {
  if (!existsSync(bin)) throw new Error(`no server binary at ${bin}; run cargo build --release -p evo-server`);
  if (!existsSync(path.join(dist, 'index.html'))) throw new Error('no client/dist; run npm run build');
  mkdirSync(shots, { recursive: true });
  const port = await freePort();
  const base = `http://127.0.0.1:${port}`;
  const server = spawn(bin, ['--fast', '--memory', '--size', '64', '--seed', '13', '--no-wild', '--bind', `127.0.0.1:${port}`, '--static', dist], {
    stdio: ['ignore', 'pipe', 'pipe'],
  });
  let log = '';
  server.stdout.on('data', (d) => (log += d));
  server.stderr.on('data', (d) => (log += d));
  const browser = cappedBrowser(await chromium.launch({ headless: !process.env.HEADED }));
  const page = await browser.newPage({ viewport: { width: 1000, height: 700 }, deviceScaleFactor: 1 });
  const pageErrors = [];
  page.on('pageerror', (e) => pageErrors.push(e.message));
  try {
    await waitFor('server', async () => (await fetch(`${base}/api/world`)).ok);
    step(`server up on ${base}`);
    await page.goto(`${base}/?tour=off`);
    await page.fill('#login-name', `ctx_${stamp()}`);
    await page.fill('#login-password', 'a long password');
    await page.click('#register-submit');
    await page.waitForSelector('#design-name', { timeout: 45000 });
    await page.fill('#design-name', 'Context keepers');
    await page.click('#design-submit');
    await page.waitForSelector('#world-canvas', { timeout: 45000 });
    await waitFor('markers with textures', async () => {
      const s = await stats(page);
      return s.markers > 0 && s.textures > 0 && s.dirty === 0;
    });
    await waitFor('the map drawn', async () => (await mapColors(page)) > 6);
    step(`map drawn: ${await mapColors(page)} colors, ${(await stats(page)).markers} markers`);

    // 1. Lost, then restored.
    if (!(await loseContext(page))) throw new Error('no WEBGL_lose_context on the map canvas');
    await waitFor('the view to see the loss', async () => (await stats(page)).state === 'lost', 5000);
    const atLoss = await stats(page);
    if (atLoss.textures !== 0) throw new Error(`generated textures kept after the loss: ${JSON.stringify(atLoss)}`);
    const lostTick = await waitFor('state frames while the context is lost', async () => {
      const s = await stats(page);
      return s.tick > atLoss.tick + 1 && s.state === 'lost' ? s.tick : 0;
    }, 2500);
    const conn = (await stats(page)).conn;
    if (conn !== 'connected') throw new Error(`connection pill reads '${conn}' while the context is lost`);
    step(`context lost: drawing paused, textures dropped, states still arriving (tick ${atLoss.tick} → ${lostTick}), pill '${conn}'`);
    await page.evaluate(() => window.__loseExt.restoreContext());
    await waitFor('the restore', async () => {
      const s = await stats(page);
      return s.state === 'live' && s.restored === 1 && s.dirty === 0 && s.textures > 0;
    }, 5000);
    await waitFor('the map redrawn after the restore', async () => (await mapColors(page)) > 6, 10000);
    const afterRestore = await stats(page);
    await waitFor('state frames after the restore', async () => (await stats(page)).tick > afterRestore.tick);
    if (afterRestore.recreated !== 0) throw new Error(`renderer recreated although the context came back: ${JSON.stringify(afterRestore)}`);
    step(`context restored: ${afterRestore.textures} textures rebuilt, map redrawn (${await mapColors(page)} colors)`);
    await page.screenshot({ path: path.join(shots, 'context-restored.png') });

    // 2. Lost for good: a new renderer on a new canvas.
    if (!(await loseContext(page))) throw new Error('no WEBGL_lose_context after the restore');
    await waitFor('the view to see the second loss', async () => (await stats(page)).lost === 2, 5000);
    await waitFor('a new renderer', async () => {
      const s = await stats(page);
      return s.recreated === 1 && s.state === 'live' && s.dirty === 0 && s.textures > 0;
    }, 15000);
    const fresh = await page.evaluate(() => {
      const canvases = document.querySelectorAll('canvas#world-canvas');
      return { count: canvases.length, fresh: canvases.length === 1 && canvases[0].dataset.lost !== '1' };
    });
    if (!fresh.fresh) throw new Error(`expected one new map canvas, got ${JSON.stringify(fresh)}`);
    await waitFor('the map drawn on the new renderer', async () => (await mapColors(page)) > 6, 10000);
    const afterNew = await stats(page);
    await waitFor('state frames on the new renderer', async () => (await stats(page)).tick > afterNew.tick);
    // Input is bound to the new canvas: a wheel zooms.
    const zoom0 = await page.evaluate(() => window.evo.screen().view.cam.zoom);
    const box = await page.locator('#world-canvas').boundingBox();
    await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
    await page.mouse.wheel(0, -400);
    await waitFor('zoom on the new canvas', async () => (await page.evaluate(() => window.evo.screen().view.cam.zoom)) > zoom0, 5000);
    step(`context not restored: new renderer after ${afterNew.lost} losses, map drawn (${await mapColors(page)} colors), input works`);
    await page.screenshot({ path: path.join(shots, 'context-recreated.png') });

    if (pageErrors.length) throw new Error(`page errors: ${pageErrors.join('; ')}`);
    console.log('context-loss test passed');
  } catch (e) {
    await page.screenshot({ path: path.join(shots, 'context-loss-failure.png') }).catch(() => {});
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
