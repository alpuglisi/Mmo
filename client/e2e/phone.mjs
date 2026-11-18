// End-to-end: the phone fixes from playtest 3 (finding 26 and the tap
// findings), on a 390 × 664 touch phone (iPhone 13 emulation in Chromium),
// with a fresh evo-server (fast, wild ecosystem, test hooks on):
//
//  1. the renown pill opens the Market in the bottom sheet;
//  2. with a catastrophe notice expanded, the open Tools menu is on top: the
//     point at the middle of Migrate… and of the group menu is the control,
//     not the notice, and a tap on Migrate… starts the migration mode; the
//     notice's fold is remembered over a reload;
//  3. the camera can go past the map's top edge by the toolbar (and notice):
//     the top row of tiles comes out from under them;
//  4. a tap in a herd selects the animal under the finger and the "Here:"
//     chooser is one compact row, listing it first, that goes at the next
//     touch of the map;
//  5. with Follow on, the camera holds still while the herd mills about the
//     middle (a dead zone: before, it chased the server's view centre and
//     slid every animal under the finger), and taps on animals land on them;
//  6. possessing says so once (the bar), not again in a toast;
//  7. Split is disabled at the four-group limit.
//
// Screenshots: e2e/out/phone-*.png. No uncaught page errors.
//
//   npm run build && node e2e/phone.mjs     (env as for smoke.mjs)

import { spawn } from 'node:child_process';
import { createRequire } from 'node:module';
import { existsSync, mkdirSync } from 'node:fs';
import { createServer } from 'node:net';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { cappedBrowser, chromiumDevice } from './frames.mjs';

const require = createRequire(import.meta.url);
const { chromium, devices } = require('playwright');

const here = path.dirname(fileURLToPath(import.meta.url));
const clientDir = path.resolve(here, '..');
const bin = process.env.EVO_SERVER_BIN ?? path.resolve(clientDir, '../target/release/evo-server');
const dist = path.join(clientDir, 'dist');
const shots = path.resolve(process.env.SCREENSHOT_DIR ?? path.join(here, 'out'));
const UI_MS = 20000;
/** Taps on moving animals with Follow on, and how many must land. */
const FOLLOW_TAPS = 12;
const FOLLOW_HITS = 11;

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

/** Record the view's clicks (what each tap landed on) as `window.__taps`. */
const recordTaps = (page) => page.evaluate(() => {
  const v = window.evo.screen().view;
  if (v.__recording) return;
  v.__recording = true;
  window.__taps = [];
  const orig = v.onClick;
  v.onClick = (c) => {
    window.__taps.push({ organism: c.organism, candidates: c.candidates ?? [] });
    return orig(c);
  };
});

async function main() {
  if (!existsSync(bin)) throw new Error(`no server binary at ${bin} (cargo build --release -p evo-server)`);
  if (!existsSync(path.join(dist, 'index.html'))) throw new Error('no client build (npm run build)');
  mkdirSync(shots, { recursive: true });
  const port = await freePort();
  const base = `http://127.0.0.1:${port}`;
  // The playtests' clock (a tick every 100 ms): animals move at the speed players saw.
  const server = spawn(bin, ['--tick-ms', '100', '--memory', '--size', '64', '--seed', '3', '--no-rate-limits', '--no-push', '--bind', `127.0.0.1:${port}`, '--static', dist], {
    stdio: ['ignore', 'pipe', 'pipe'], env: { ...process.env, EVO_E2E_HOOKS: '1' },
  });
  let log = '';
  server.stdout.on('data', (d) => (log += d));
  server.stderr.on('data', (d) => (log += d));
  const browser = cappedBrowser(await chromium.launch());
  const errors = [];
  try {
    await waitFor('server', async () => (await fetch(`${base}/api/world`)).ok);
    step(`server up on ${base}`);
    const context = await browser.newContext({ ...chromiumDevice(devices, 'iPhone 13') });
    const page = await context.newPage();
    page.on('pageerror', (e) => errors.push(e.message));
    await page.goto(`${base}/?tour=off`);
    await page.fill('#login-name', 'pocket_phone');
    await page.fill('#login-password', 'a long password');
    await page.tap('#register-submit');
    await page.waitForSelector('#design-name', { timeout: UI_MS });
    await page.fill('#design-name', 'Pocket herd');
    await page.tap('#design-submit');
    await page.waitForSelector('#world-canvas', { timeout: UI_MS });
    await waitFor('the lineage', () => page.evaluate(() => (window.evo.model.state?.organisms.length ?? 0) > 1));
    const viewport = page.viewportSize();
    step(`founded on a ${viewport.width} × ${viewport.height} phone`);

    // 1. The renown pill opens the Market.
    await page.tap('#balance');
    await waitFor('the Market in the sheet', () => page.evaluate(() => document.querySelector('.game')?.classList.contains('sheet-open')
      && !document.querySelector('#panel-market').hidden));
    step('the renown pill opens the Market in the bottom sheet');
    await page.tap('#tab-market'); // a second tap on the open tab closes the sheet
    await waitFor('the sheet closed', () => page.evaluate(() => !document.querySelector('.game').classList.contains('sheet-open')));

    // 2. A catastrophe notice, expanded, under the open Tools menu.
    await page.evaluate(() => { try { localStorage.removeItem('evo.notice.catastrophe'); } catch { /* blocked */ } });
    const r = await fetch(`${base}/api/e2e/catastrophe`, { method: 'POST', headers: { 'content-type': 'application/json' }, body: JSON.stringify({ kind: 'megadrought', days: 60 }) });
    check(r.ok, 'the test hook started a catastrophe', await r.text());
    await page.waitForSelector('#catastrophe-alert:not([hidden])', { timeout: UI_MS });
    check(!(await page.evaluate(() => document.querySelector('#catastrophe-alert').classList.contains('folded'))), 'the notice starts expanded');
    await page.tap('#tools-more');
    await page.waitForSelector('#tools-menu #migrate', { state: 'visible' });
    const onTop = await page.evaluate(() => ['#migrate', '#group-select'].map((q) => {
      const el = document.querySelector(q);
      const b = el.getBoundingClientRect();
      const at = document.elementFromPoint(b.left + b.width / 2, b.top + b.height / 2);
      const notice = document.querySelector('#map-notices').getBoundingClientRect();
      const overlaps = !(b.bottom <= notice.top || b.top >= notice.bottom || b.right <= notice.left || b.left >= notice.right);
      return { q, overlaps, hit: !!at && (at === el || el.contains(at)), at: at ? `${at.tagName}#${at.id}.${at.className}` : null };
    }));
    check(onTop.some((x) => x.overlaps), 'the menu and the notice should overlap on a phone (else this checks nothing)', onTop);
    for (const x of onTop) check(x.hit, `${x.q} should be on top of the notice`, x);
    await page.screenshot({ path: path.join(shots, 'phone-tools-over-notice.png') });
    await page.tap('#migrate');
    await waitFor('migration mode', () => page.evaluate(() => /migration target/.test(document.querySelector('#hint')?.textContent ?? '')));
    await page.tap('#tools-more');
    await page.tap('#migrate'); // off again
    step(`with the catastrophe notice expanded, the Tools menu is on top (${onTop.map((x) => x.q).join(', ')} tappable) and Migrate… works`);
    // The fold, remembered.
    await page.tap('#catastrophe-alert .cat-fold');
    check(await page.evaluate(() => document.querySelector('#catastrophe-alert').classList.contains('folded')), 'folded');
    await page.reload();
    await page.waitForSelector('#catastrophe-alert:not([hidden])', { timeout: UI_MS });
    check(await page.evaluate(() => document.querySelector('#catastrophe-alert').classList.contains('folded')), 'the fold should be remembered over a reload');
    step('the notice\'s fold is remembered over a reload');
    await waitFor('the lineage again', () => page.evaluate(() => (window.evo.model.state?.organisms.length ?? 0) > 1));

    // 3. Past the top edge by the toolbar.
    const edge = await page.evaluate(async () => {
      const v = window.evo.screen().view;
      v.setFollow(false);
      v.cam.zoom = 24;
      v.cam.y = -100;
      await new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r)));
      const canvas = document.querySelector('#world-canvas').getBoundingClientRect();
      const over = Math.max(document.querySelector('.toolbar').getBoundingClientRect().bottom,
        ...[...document.querySelectorAll('#map-notices > :not([hidden])')].map((n) => n.getBoundingClientRect().bottom));
      return { mapTop: canvas.top + v.worldToScreen(0, 0).y, over, pad: v.edgePad.top };
    });
    check(edge.mapTop >= edge.over, 'the map\'s top edge should come out from under the toolbar and notice', edge);
    step(`the camera goes past the top edge by ${edge.pad} px: the map starts at y ${edge.mapTop.toFixed(0)}, under the toolbar and notice (y ${edge.over.toFixed(0)})`);

    // 4. A tap in a herd: the animal under the finger, and a compact chooser.
    await recordTaps(page);
    const canvasBox = await page.locator('#world-canvas').boundingBox();
    const herdTap = await waitFor('a tap on an animal with others in reach', async () => {
      const at = await page.evaluate(() => {
        const v = window.evo.screen().view;
        v.setFollow(false);
        v.cam.zoom = 10;
        const s = window.evo.model.state;
        const own = s.organisms[0];
        v.centerOn(own.x, own.y);
        // An animal with others under the same fingertip, a wild one if there is one.
        const ids = [...s.nearby.filter((o) => o.owner === 0).map((o) => o.id), ...s.organisms.map((o) => o.id)];
        for (const id of ids) {
          const p = v.markerScreenPos(id);
          if (!p || p.y < 120 || p.y > window.innerHeight - 260) continue;
          if (v.hitTest(p.x, p.y, true) === id && v.hitAll(p.x, p.y, true).length > 1) return { id, ...p };
        }
        return null;
      });
      if (!at) return null;
      await page.evaluate(() => { window.__taps = []; });
      await page.touchscreen.tap(canvasBox.x + at.x, canvasBox.y + at.y);
      const tap = await waitFor('the tap', () => page.evaluate(() => window.__taps[0]), 3000).catch(() => null);
      return tap && tap.organism !== null && tap.candidates.length > 1 ? { at, tap } : null;
    });
    const picked = await page.evaluate(() => window.evo.model.selected);
    check(picked === herdTap.tap.organism, 'the tap should select the animal under the finger', { picked, tap: herdTap.tap });
    await page.waitForSelector('#pick-chooser:not([hidden])', { timeout: 3000 });
    const chooser = await page.evaluate(() => {
      const c = document.querySelector('#pick-chooser');
      const b = c.getBoundingClientRect();
      const chips = [...c.querySelectorAll('.chip[data-org]')];
      return { height: b.height, width: b.width, first: Number(chips[0]?.dataset.org), rows: new Set(chips.map((x) => Math.round(x.getBoundingClientRect().top))).size };
    });
    check(chooser.rows === 1 && chooser.height <= 52, 'the chooser should be one compact row', chooser);
    check(chooser.first === picked, 'the chooser lists the tapped animal first', { chooser, picked });
    await page.screenshot({ path: path.join(shots, 'phone-chooser.png') });
    // The next touch of the map puts it away (a drag here: it pans, it does not tap).
    await page.evaluate(() => {
      const c = document.querySelector('#world-canvas');
      const b = c.getBoundingClientRect();
      const at = { clientX: b.left + 30, clientY: b.top + b.height / 2, pointerId: 7, pointerType: 'touch', isPrimary: true, bubbles: true };
      c.dispatchEvent(new PointerEvent('pointerdown', at));
      c.dispatchEvent(new PointerEvent('pointerup', at));
    });
    await page.waitForSelector('#pick-chooser', { state: 'hidden', timeout: 3000 });
    step(`a tap in a herd selects #${picked} (under the finger, of ${herdTap.tap.candidates.length} in reach); the chooser is one ${chooser.height.toFixed(0)} px row, listing it first, gone at the next touch`);

    // 5. Follow on: taps on (moving) animals land on them. Real frames for this.
    await page.evaluate(() => window.__e2eFrameCap?.(0));
    await page.evaluate(() => {
      const v = window.evo.screen().view;
      v.cam.zoom = 22;
      v.setFollow(true);
      window.evo.model.select(null);
    });
    await page.waitForTimeout(1500);
    // While the herd mills about the middle of the screen, the camera holds
    // still (the dead zone): the server's view centre moves with every birth
    // and step, and a camera chasing it slid every animal under the finger.
    const still = await page.evaluate(async () => {
      const v = window.evo.screen().view;
      let travel = 0;
      let last = { ...v.cam };
      const t0 = performance.now();
      while (performance.now() - t0 < 4000) {
        await new Promise((r) => requestAnimationFrame(r));
        travel += Math.hypot(v.cam.x - last.x, v.cam.y - last.y) * v.cam.zoom;
        last = { ...v.cam };
      }
      const s = window.evo.model.state.view;
      const { width, height } = v.app.screen;
      // Follow's dead zone for a herd (worldView FOLLOW_DEAD: a quarter of half the shorter side).
      return { travel, off: Math.max(Math.abs(s.x - v.cam.x), Math.abs(s.y - v.cam.y)) * v.cam.zoom, dead: 0.25 * Math.min(width, height) / 2 };
    });
    // (Unless the herd has walked out of the dead zone meanwhile: then the camera rightly follows.)
    check(still.travel <= 3 || still.off >= still.dead * 0.9, 'the camera should hold still while the herd mills about the centre', still);
    step(`Follow holds the camera still while the herd mills about (${still.travel.toFixed(1)} px of travel in 4 s, the herd centre ${still.off.toFixed(0)} px off centre)`);
    let hits = 0;
    const misses = [];
    for (let i = 0; i < FOLLOW_TAPS; i++) {
      // An animal of ours on screen, clear of the toolbar and the bottom controls, alone under a fingertip.
      const target = await page.evaluate((i) => {
        const v = window.evo.screen().view;
        const ids = window.evo.model.state.organisms.map((o) => o.id);
        const ok = ids.map((id) => ({ id, p: v.markerScreenPos(id) }))
          .filter(({ id, p }) => p && p.y > 130 && p.y < window.innerHeight - 260 && p.x > 30 && p.x < window.innerWidth - 30 && v.hitAll(p.x, p.y, true).length === 1 && v.hitTest(p.x, p.y, true) === id);
        return ok.length ? ok[i % ok.length] : null;
      }, i);
      if (!target) {
        await page.waitForTimeout(300);
        continue;
      }
      // A person aims, then taps: about a quarter of a second later.
      const cam0 = await page.evaluate(() => ({ ...window.evo.screen().view.cam }));
      await page.waitForTimeout(250);
      const now = await page.evaluate((id) => ({ p: window.evo.screen().view.markerScreenPos(id), cam: { ...window.evo.screen().view.cam } }), target.id);
      await page.evaluate(() => { window.__taps = []; });
      await page.touchscreen.tap(canvasBox.x + target.p.x, canvasBox.y + target.p.y);
      const tap = await waitFor('the tap', () => page.evaluate(() => window.__taps[0]), 3000).catch(() => null);
      if (tap?.organism === target.id) hits++;
      else {
        misses.push({ id: target.id, got: tap?.organism ?? null, moved: now.p ? Math.hypot(now.p.x - target.p.x, now.p.y - target.p.y).toFixed(1) : null,
          camMoved: (Math.hypot(now.cam.x - cam0.x, now.cam.y - cam0.y) * now.cam.zoom).toFixed(1) });
      }
      await page.evaluate(() => { document.querySelector('#pick-chooser')?.setAttribute('hidden', ''); window.evo.model.select(null); });
    }
    await page.evaluate((fps) => window.__e2eFrameCap?.(fps), Number(process.env.E2E_FPS ?? 5));
    const tried = hits + misses.length;
    check(tried >= FOLLOW_TAPS - 2, 'enough animals to tap', { tried });
    check(hits >= Math.min(FOLLOW_HITS, tried - 1), `taps with Follow on should land: ${hits} of ${tried}`, misses);
    step(`Follow on: ${hits} of ${tried} taps on our animals landed on them${misses.length ? ` (missed: ${JSON.stringify(misses)})` : ''}`);

    // 6. Possessing says so once.
    const own = await page.evaluate(() => {
      const o = window.evo.model.state.organisms.filter((x) => x.matured).sort((a, b) => a.needs.thirst - b.needs.thirst)[0];
      window.evo.model.select(o.id);
      document.querySelector('#toasts')?.replaceChildren();
      return o.id;
    });
    await page.tap('#fab-possess');
    await page.waitForSelector('#possess-bar:not([hidden])', { timeout: UI_MS });
    await page.waitForTimeout(500);
    const said = await page.evaluate(() => ({ bar: document.querySelector('#possess-bar').textContent, toasts: document.querySelector('#toasts')?.textContent ?? '' }));
    check(said.bar.includes(`#${own}`), 'the bar names the possessed animal', said);
    check(!/Possessing #/.test(said.toasts), 'no toast repeating the bar', said);
    await page.screenshot({ path: path.join(shots, 'phone-possessing.png') });
    await page.tap('#possess-release');
    await page.waitForSelector('#possess-bar', { state: 'hidden', timeout: UI_MS });
    step(`possessing #${own}: the bar says so, no toast repeats it`);

    // 7. Split, up to the limit of four groups.
    const send = (m) => page.evaluate((m) => window.evo.socket.send(m), m);
    const groups = [0];
    for (const from of [0, 0, 1]) {
      const res = await send({ type: 'split_group', group: groups[from] });
      check(res.ok, 'split', res);
      groups.push(res.data.group);
    }
    await waitFor('four groups listed', () => page.evaluate(() => (window.evo.model.state.groups ?? []).filter((g) => g.members > 0).length === 4));
    await waitFor('Split disabled', () => page.evaluate(() => document.querySelector('#split-group').disabled));
    const title = await page.getAttribute('#split-group', 'title');
    step(`Split is disabled at four groups ("${title}")`);

    check(errors.length === 0, 'page errors', errors);
    step('no page errors');
    await context.close();
    console.log('phone e2e passed');
  } catch (e) {
    console.error(log.split('\n').slice(-20).join('\n'));
    throw e;
  } finally {
    await browser.close();
    server.kill();
  }
}

main().catch((e) => {
  console.error(`phone e2e failed: ${e.stack ?? e}`);
  process.exit(1);
});
