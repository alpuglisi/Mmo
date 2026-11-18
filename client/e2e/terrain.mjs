// End-to-end: the painted map stays true to the terrain over a long, fast
// session (playtests 1 and 2, #15: stale snow blocks in spring and summer,
// chunk seams after ~45 minutes, a blocky patchwork after a reload). In
// headless Chromium, frame-capped as the other scripts (software WebGL,
// E2E_FPS: a slow machine), with a fresh evo-server on a fast clock and a
// short year:
//
//  1. a player founds and the page wanders the map in winter, at both detail
//     levels, so snowy chunks are painted and cached (and many go off screen);
//  2. once summer has melted the snow, the page goes back over the same
//     places. On every frame no chunk on screen may stay far from the
//     terrain for more than `STALE_FRAMES_MAX` frames in a row (before the
//     fix the chunks round the screen center took every repaint and the rest
//     kept last season's snow), and at each place, once the map has caught up
//     (`terrainStats`: every chunk painted, none past `STALE`; within
//     `CATCH_UP_FRAMES`), what is drawn is compared with a painting made
//     from scratch of the same terrain:
//     - no stale snow: no white texel where the fresh painting has none;
//     - the same picture: almost every texel within a small distance;
//     - no seams: across chunk borders neighbouring texels differ no more
//       than they do inside chunks (pixel continuity).
//     Bounds are in frames, not seconds: a loaded machine draws fewer;
//  3. the page is reloaded and the map, painted afresh, must catch up as
//     fast and pass the same checks.
//
// Screenshots of each stage go to SCREENSHOT_DIR (default e2e/out) as
// terrain-*.png.
//
//   npm run build && node e2e/terrain.mjs     (env as for smoke.mjs)

import { spawn } from 'node:child_process';
import { createRequire } from 'node:module';
import { existsSync, mkdirSync } from 'node:fs';
import { createServer } from 'node:net';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { cappedBrowser } from './frames.mjs';

const require = createRequire(import.meta.url);
const { chromium } = require('playwright');

const here = path.dirname(fileURLToPath(import.meta.url));
const clientDir = path.resolve(here, '..');
const bin = process.env.EVO_SERVER_BIN ?? path.resolve(clientDir, '../target/release/evo-server');
const dist = path.join(clientDir, 'dist');
const shots = path.resolve(process.env.SCREENSHOT_DIR ?? path.join(here, 'out'));

/** Places visited (tile centers) and zooms: both detail levels (8 and 16 texels a tile at pixel ratio 1). */
const TOUR = [
  { x: 30, y: 30, zoom: 9 },
  { x: 96, y: 34, zoom: 15 },
  { x: 64, y: 64, zoom: 9 },
  { x: 34, y: 96, zoom: 15 },
  { x: 96, y: 96, zoom: 9 },
];
// Bounds count the page's frames, not seconds: painting happens in frames
// (a share of each, `paintBudget`), and on a loaded machine (the parallel
// runner) the same seconds bring fewer of them. The proof is the first
// bound, checked on every frame: no chunk on screen stays far from the
// terrain (`terrainStats().maxStaleFrames`) for long, as they did when the
// chunks round the screen center took every repaint. Catching up has a
// generous ceiling of its own; a wall-clock ceiling stops a page that stalls.
/** The most frames in a row a chunk on screen may show a painting far from the terrain. */
const STALE_FRAMES_MAX = 60;
/** The map must catch up with the terrain within this many frames of arriving somewhere. */
const CATCH_UP_FRAMES = 300;
/** The most wall time catching up may take (ms). */
const CEILING_MS = 120000;
/** The page's CPU is slowed this many times (a loaded machine: the playtests ran at a load of ~12 on 4 cores). */
const CPU_SLOWDOWN = Number(process.env.E2E_TERRAIN_CPU ?? 1);

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

/**
 * In the page: null while the map is still painting; else what is drawn
 * against a painting from scratch of the same terrain and view (all in one
 * task, so no terrain update comes in between).
 */
function compareWithFresh(gate) {
  const v = window.evo.screen().view;
  const L = v.terrainLayer;
  const { width, height } = v.app.screen;
  const tl = v.screenToWorld(0, 0), br = v.screenToWorld(width, height);
  const view = { x0: tl.x, y0: tl.y, x1: br.x, y1: br.y };
  const dpt = v.cam.zoom * (v.app.renderer.resolution || 1);
  // A frame of the layer for this view (the page's last one may have been for
  // the place before), the same light for both (the hour's light eases), and
  // no painting time beyond its one slice: then is anything left to paint?
  const keep = L.lightOverride;
  L.lightOverride = 12;
  L.budgetOverride = 0;
  L.update(view, dpt, 12, 1e9);
  L.budgetOverride = null;
  const st = L.terrainStats();
  // Caught up: every chunk on screen painted, none past `STALE` (the terrain
  // refreshes every 4 s, so this comes between refreshes).
  if (gate && (st.pending > 0 || st.stale > 0)) {
    L.lightOverride = keep;
    return { waiting: { pending: st.pending, unpainted: st.unpainted, stale: st.stale, maxStale: st.maxStale, detail: st.detail, frames: st.frames } };
  }
  const Rect = L.root.getBounds().rectangle.constructor;
  const x0 = Math.max(0, Math.floor(view.x0)), y0 = Math.max(0, Math.floor(view.y0));
  const x1 = Math.min(st.width, Math.ceil(view.x1)), y1 = Math.min(st.height, Math.ceil(view.y1));
  const res = Math.max(4, st.detail || st.baseTpx);
  const grab = (layer) => v.app.renderer.extract.pixels({ target: layer.root, frame: new Rect(x0, y0, x1 - x0, y1 - y0), resolution: res });
  const shown = grab(L);
  const fresh = new L.constructor();
  fresh.lightOverride = 12;
  fresh.budgetOverride = 1e9;
  fresh.setTerrain(v.terrain);
  for (let i = 0; i < 1000 && (i === 0 || fresh.terrainStats().pending > 0); i++) fresh.update(view, dpt, 12, 1e9);
  const want = grab(fresh);
  fresh.destroy();
  L.lightOverride = keep;
  const a = shown.pixels, b = want.pixels, W = shown.width, H = shown.height;
  const white = (p, i) => p[i] > 200 && p[i + 1] > 200 && p[i + 2] > 200;
  let far = 0, staleSnow = 0, snowShown = 0, sum = 0;
  const offBy = new Map();
  for (let i = 0; i < a.length; i += 4) {
    const d = Math.abs(a[i] - b[i]) + Math.abs(a[i + 1] - b[i + 1]) + Math.abs(a[i + 2] - b[i + 2]);
    sum += d;
    if (d > 30) {
      far++;
      const p = i / 4, key = `${Math.floor((x0 + (p % W) / res) / 16)},${Math.floor((y0 + Math.floor(p / W) / res) / 16)}`;
      offBy.set(key, (offBy.get(key) ?? 0) + 1);
    }
    if (white(a, i)) snowShown++;
    if (white(a, i) && !white(b, i)) staleSnow++;
  }
  // Seams: the step between neighbouring texels across chunk borders against
  // the steps next to them inside chunks (both directions).
  const seams = (p) => {
    const step = (i, j) => Math.abs(p[i] - p[j]) + Math.abs(p[i + 1] - p[j + 1]) + Math.abs(p[i + 2] - p[j + 2]);
    let across = 0, nAcross = 0, inside = 0, nInside = 0;
    const chunk = 16 * res;
    for (let bx = (16 - (x0 % 16)) % 16 * res; bx < W; bx += chunk) {
      if (bx < 4 || bx > W - 4) continue;
      for (let y = 0; y < H; y++) {
        const r = y * W;
        across += step((r + bx) * 4, (r + bx - 1) * 4); nAcross++;
        for (const o of [-3, -2, 2, 3]) { inside += step((r + bx + o) * 4, (r + bx + o - 1) * 4); nInside++; }
      }
    }
    for (let by = (16 - (y0 % 16)) % 16 * res; by < H; by += chunk) {
      if (by < 4 || by > H - 4) continue;
      for (let x = 0; x < W; x++) {
        across += step((by * W + x) * 4, ((by - 1) * W + x) * 4); nAcross++;
        for (const o of [-3, -2, 2, 3]) { inside += step(((by + o) * W + x) * 4, ((by + o - 1) * W + x) * 4); nInside++; }
      }
    }
    return nAcross && nInside ? across / nAcross / Math.max(1, inside / nInside) : 1;
  };
  let snowTiles = 0;
  for (const s of v.terrain.snow) if (s > 25) snowTiles++;
  const n = a.length / 4;
  return {
    caughtUp: !(st.pending > 0 || st.stale > 0), day: v.terrain.day, season: v.terrain.season, snowTiles, detail: st.detail, texels: n,
    far: far / n, mean: sum / n, staleSnow, snowShown, seams: seams(a), seamsFresh: seams(b),
    // Where it differs most: chunk "x,y" and its state, for a failure's message.
    worst: [...offBy].sort((p, q) => q[1] - p[1]).slice(0, 3).map(([key, texels]) => ({ key, texels, chunk: L.chunkInfo?.(key) })),
  };
}

async function main() {
  if (!existsSync(bin)) throw new Error(`no server binary at ${bin}; run cargo build --release -p evo-server`);
  if (!existsSync(path.join(dist, 'index.html'))) throw new Error('no client/dist; run npm run build');
  mkdirSync(shots, { recursive: true });
  const port = await freePort();
  const base = `http://127.0.0.1:${port}`;
  // A 128-tile map (the playtests'), seasons of 16 days (about 20 s here).
  const server = spawn(bin, ['--fast', '--memory', '--size', '128', '--seed', '11', '--days-per-year', '64', '--no-catastrophes', '--no-rate-limits', '--no-push',
    '--bind', `127.0.0.1:${port}`, '--static', dist], { stdio: ['ignore', 'ignore', 'pipe'] });
  let serverErr = '';
  server.stderr.on('data', (d) => (serverErr += d));
  const browser = cappedBrowser(await chromium.launch());
  const pageErrors = [];
  try {
    for (let k = 0; ; k++) {
      try {
        if ((await fetch(`${base}/api/world`)).ok) break;
      } catch {
        /* starting */
      }
      if (k > 200) throw new Error(`server did not start: ${serverErr}`);
      await new Promise((r) => setTimeout(r, 100));
    }
    const page = await browser.newPage({ viewport: { width: 1280, height: 800 }, deviceScaleFactor: 1 });
    page.on('pageerror', (e) => pageErrors.push(e.message));
    if (CPU_SLOWDOWN > 1) await (await page.context().newCDPSession(page)).send('Emulation.setCPUThrottlingRate', { rate: CPU_SLOWDOWN });
    await page.goto(`${base}/?tour=off`);
    await page.fill('#login-name', 'mapwatcher');
    await page.fill('#login-password', 'a long password');
    await page.click('#register-submit');
    await page.waitForSelector('#design-name');
    await page.fill('#design-name', 'Map hares');
    await page.click('#design-submit');
    await page.waitForSelector('#world-canvas');
    await page.waitForFunction(() => window.evo?.screen()?.view?.terrain, null, { timeout: 30000 });

    const season = () => page.evaluate(() => {
      const t = window.evo.screen().view.terrain;
      let snow = 0;
      for (const s of t.snow) if (s > 25) snow++;
      return { season: t.season, day: t.day, snow };
    });
    const goTo = (p) => page.evaluate((p) => {
      const v = window.evo.screen().view;
      v.setFollow(false);
      v.cam.zoom = p.zoom;
      v.centerOn(p.x, p.y);
      // Two frames, so the layer's stats are for this view.
      return new Promise((done) => requestAnimationFrame(() => requestAnimationFrame(() => done())));
    }, p);
    const waitFor = async (what, fn, ms) => {
      const until = Date.now() + ms;
      for (;;) {
        const r = await fn();
        if (r) return r;
        if (Date.now() > until) throw new Error(`timed out waiting for ${what}`);
        await page.waitForTimeout(250);
      }
    };

    // 1. Winter, with snow on a good part of the land: paint and cache it everywhere.
    const winter = await waitFor('a snowy winter', async () => { const s = await season(); return s.season === 'winter' && s.snow > 500 ? s : null; }, 120000);
    for (const p of TOUR) {
      await goTo(p);
      // (Best effort: snowy chunks painted and cached is what matters here.)
      await waitFor('the winter map painted', () => page.evaluate(() => window.evo.screen().view.terrainStats().pending === 0), 15000).catch(() => {});
    }
    await page.screenshot({ path: path.join(shots, 'terrain-winter.png') });
    step(`winter (day ${winter.day + 1}, ${winter.snow} snowy tiles): painted at ${TOUR.length} places, both detail levels`);

    // 2. Summer, the snow gone: back over the same places.
    const summer = await waitFor('summer without snow', async () => { const s = await season(); return s.season === 'summer' && s.snow === 0 ? s : null; }, 120000);
    // Frames drawn since the page loaded (the capped requestAnimationFrame),
    // and the longest run of frames a chunk on screen stayed far from the
    // terrain since the last reset.
    const countFrames = () => page.evaluate(() => {
      if (window.__terrainFrames !== undefined) return;
      window.__terrainFrames = 0;
      window.__terrainStale = 0;
      const tick = () => {
        window.__terrainFrames++;
        const st = window.evo?.screen()?.view?.terrainStats();
        if (st) window.__terrainStale = Math.max(window.__terrainStale, st.maxStaleFrames ?? 0);
        requestAnimationFrame(tick);
      };
      requestAnimationFrame(tick);
    });
    const frames = () => page.evaluate(() => window.__terrainFrames);
    const staleRun = (reset) => page.evaluate((reset) => {
      const n = window.__terrainStale;
      if (reset) window.__terrainStale = 0;
      return n;
    }, reset);
    await countFrames();
    const checkHere = async (label) => {
      const t0 = Date.now();
      const f0 = await frames();
      await staleRun(true);
      // Caught up, then what is drawn against a fresh painting.
      let r;
      for (;;) {
        r = await page.evaluate(compareWithFresh, true);
        if (!r.waiting) break;
        const n = (await frames()) - f0;
        check(n < CATCH_UP_FRAMES && Date.now() - t0 < CEILING_MS, `${label}: the map did not catch up with the terrain in ${n} frames (${((Date.now() - t0) / 1000).toFixed(1)} s)`, r.waiting);
        await page.waitForTimeout(100);
      }
      const ms = Date.now() - t0;
      const n = (await frames()) - f0;
      const stale = await staleRun(false);
      check(stale <= STALE_FRAMES_MAX, `${label}: a chunk showed a painting far from the terrain for ${stale} frames in a row`, r);
      check(r.staleSnow === 0, `${label}: stale snow on the map`, r);
      check(r.far < 0.01 && r.mean < 3, `${label}: the map differs from a fresh painting of the terrain`, r);
      check(r.seams < 1.6 && r.seams < r.seamsFresh * 1.35 + 0.1, `${label}: seams at chunk borders`, r);
      return { ...r, ms, frames: n, stale };
    };
    const results = [];
    for (const [i, p] of TOUR.entries()) {
      await goTo(p);
      results.push(await checkHere(`summer, place ${i + 1} (zoom ${p.zoom})`));
    }
    await page.screenshot({ path: path.join(shots, 'terrain-summer.png') });
    const worst = (k) => Math.max(...results.map((r) => r[k]));
    step(`summer (day ${summer.day + 1}): ${results.length} places match a fresh painting (worst: ${(worst('far') * 100).toFixed(2)}% of texels off, `
      + `seam ratio ${worst('seams').toFixed(2)}, caught up in ≤ ${worst('frames')} frames / ${(worst('ms') / 1000).toFixed(1)} s, `
      + `no chunk far from the terrain for more than ${worst('stale')} frames), no stale snow`);

    // 3. A reload: the first painting is smooth and seamless too.
    await page.reload();
    await page.waitForSelector('#world-canvas');
    await page.waitForFunction(() => window.evo?.screen()?.view?.terrain, null, { timeout: 30000 });
    await countFrames();
    await goTo(TOUR[1]);
    const after = await checkHere('after a reload');
    await page.screenshot({ path: path.join(shots, 'terrain-reload.png') });
    step(`after a reload (${after.season}): matches a fresh painting in ${after.frames} frames / ${(after.ms / 1000).toFixed(1)} s, seam ratio ${after.seams.toFixed(2)}, longest stale run ${after.stale} frames`);

    check(pageErrors.length === 0, 'page errors', pageErrors);
    step('no page errors');
  } finally {
    await browser.close();
    server.kill('SIGINT');
  }
}

main().then(
  () => console.log('terrain e2e passed'),
  (e) => {
    console.error(`terrain e2e FAILED: ${e.stack ?? e}`);
    process.exit(1);
  },
);
