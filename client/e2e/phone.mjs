// End-to-end: the phone fixes from playtest 3 (finding 26 and the tap
// findings), on a 390 × 664 touch phone (iPhone 13 emulation in Chromium),
// with a fresh evo-server (fast, wild ecosystem, test hooks on):
//
//  1. the renown pill opens the Market in the bottom sheet;
//  2. the camera can go past the map's top edge by the toolbar (and any
//     notice): the top row of tiles comes out from under them;
//  3. a tap in a herd selects the animal under the finger and the "Here:"
//     chooser is one compact row, listing it first, that goes at the next
//     touch of the map;
//  4. with Follow on, the camera holds still while the herd mills about the
//     middle (a dead zone: before, it chased the server's view centre and
//     slid every animal under the finger), and taps on animals land on them;
//  5. with a catastrophe notice expanded, the open Tools menu is on top: the
//     point at the middle of Migrate… and of the group menu is the control,
//     not the notice, and a tap on Migrate… starts the migration mode; the
//     notice's fold is remembered over a reload (before the Follow checks a
//     megadrought would send the herd walking to water);
//  6. possessing says so once (the bar), not again in a toast;
//  7. Split is disabled at the four-group limit;
//  8. a returning player in a new browser is not shown the tour.
// Also (playtest 4): the builder previews compatibility before a name is
// typed and warns about useless wings once; the camera goes past the
// bottom edge too; the chooser fits the screen with its × in reach and no tip
// card over it; with two notices the second starts folded.
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

/** How much longer waits may take when scripts run side by side (`run-all`'s `E2E_TIME_SCALE`). */
const TIME_SCALE = Math.max(1, Number(process.env.E2E_TIME_SCALE) || 1);

async function waitFor(what, fn, timeoutMs = UI_MS) {
  const end = Date.now() + timeoutMs * TIME_SCALE;
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
    // The app has its own dialogs: a browser alert/confirm/prompt is a bug (playtest 6).
    page.on('dialog', (d) => { errors.push(`native ${d.type()} dialog: ${d.message()}`); void d.dismiss(); });
    await page.goto(`${base}/?tour=off`);
    await page.fill('#login-name', 'pocket_phone');
    await page.fill('#login-password', 'a long password');
    await page.tap('#register-submit');
    // 0. The builder: its compatibility preview works before a name is typed
    // (the water warning depends on it), and a too-heavy body's wings are
    // warned about once.
    const preview = await page.waitForResponse((res) => new URL(res.url()).pathname === '/api/design/compatibility', { timeout: UI_MS * TIME_SCALE });
    check(preview.ok(), `the builder's preview before a name is typed: ${preview.status()}`, await preview.text());
    await page.waitForSelector('#design-name', { timeout: UI_MS * TIME_SCALE });
    check((await page.inputValue('#design-name')) === '', 'no name typed yet');
    const genes = await page.$$eval('input[data-gene]', (els) => Object.fromEntries(els.map((el) => [el.dataset.gene, el.value])));
    const setGene = (g, v) => page.$eval(`input[data-gene="${g}"]`, (el, v) => { el.value = String(v); el.dispatchEvent(new Event('input')); }, v);
    for (const [g, v] of [['body_size', 1], ['armor_plates', 1], ['wings', 0.8]]) if (g in genes) await setGene(g, v);
    const wingWarnings = await waitFor('the wings warning', () => page.evaluate(() => {
      // (The warning sentence; the wings slider's own caption says "too heavy to fly" beside it, which is its label.)
      const n = (document.querySelector('.design-body')?.innerText ?? '').match(/Wings do nothing/gi)?.length ?? 0;
      return n > 0 ? n : null;
    }), 5000).catch(() => 0);
    for (const g of ['body_size', 'armor_plates', 'wings']) if (g in genes) await setGene(g, genes[g]);
    check(wingWarnings === 1, `a too-heavy body's wings should be warned about once, not ${wingWarnings} times`);
    step('the builder previews compatibility before a name is typed, and warns once about useless wings');
    // A reserved name is refused as it is typed, Found disabled (playtest 6: only at founding, as "bad_name: …").
    await page.fill('#design-name', 'G.M. Admin');
    const refused = await waitFor('the name refused inline', () => page.evaluate(() => {
      const w = document.querySelector('#design-name-warning');
      const p = (w && !w.hidden ? w.textContent : '') || (document.querySelector('#design-problem')?.textContent ?? '');
      return p && document.querySelector('#design-submit').disabled ? p : null;
    }), 8000);
    check(!/bad_name/.test(refused), 'the refusal in words, not its code', refused);
    step(`a reserved name is refused as it is typed ("${refused}"), Found disabled`);
    await page.fill('#design-name', 'Pocket herd');
    await waitFor('Found enabled again', () => page.evaluate(() => !document.querySelector('#design-submit').disabled), 8000);
    await page.tap('#design-submit');
    await page.waitForSelector('#world-canvas', { timeout: UI_MS * TIME_SCALE });
    await waitFor('the lineage', () => page.evaluate(() => (window.evo.model.state?.organisms.length ?? 0) > 1));
    const viewport = page.viewportSize();
    step(`founded on a ${viewport.width} × ${viewport.height} phone`);

    // Log out stays whole in the top bar, even with a big renown balance (playtest 11: "Lo / ou" at 390 px).
    const bar = await page.evaluate(() => {
      const b = document.querySelector('#balance');
      const was = b.textContent;
      b.textContent = '12,900 renown';
      // A late date and a world name, as in a long game on a server with two worlds.
      const w = document.querySelector('#world-name');
      w.textContent = 'Main world';
      w.hidden = false;
      const c = document.querySelector('#clock .clock-short');
      if (c) c.textContent = 'Y4 d412 14:00 autumn ☾';
      // And the connection pill in words (reconnecting), not the green dot.
      const conn = document.querySelector('#conn-state');
      const connWas = [conn.className, conn.textContent];
      conn.className = 'pill conn-closed';
      conn.textContent = 'reconnecting…';
      const out = document.querySelector('#logout');
      const r = out.getBoundingClientRect();
      const res = { right: r.right, width: r.width, vw: innerWidth, clipped: out.scrollWidth > out.clientWidth + 1, text: out.textContent };
      b.textContent = was;
      w.hidden = true;
      [conn.className, conn.textContent] = connWas;
      return res;
    });
    check(bar.right <= bar.vw && !bar.clipped && bar.width >= 50, 'Log out should fit in the top bar on a 390 px phone', bar);
    step('Log out fits the top bar beside a big renown balance, a world name and a worded connection pill');

    // A long toast shows two lines on a phone; a tap opens the rest (playtest 11: 3–4 line toasts covered a third of the map).
    await page.evaluate(() => window.evo.model.toast('info', 'Your lineage is being hunted: 7 of your animals were eaten on day 300, by a rival pack near 40, 52. Keep the herd together, move it away from the pack, or claim a territory where the hunters are fewer; the Food layer shows where the plants are.'));
    const clamp = await page.evaluate(() => new Promise((res) => requestAnimationFrame(() => requestAnimationFrame(() => {
      const el = document.querySelector('#toasts .toast:last-child');
      const lh = parseFloat(getComputedStyle(el).lineHeight) || 16;
      res({ h: el.clientHeight, lh, long: el.classList.contains('long') });
    }))));
    check(clamp.long && clamp.h <= clamp.lh * 2 + 12, 'a long toast should be cut to two lines on a phone', clamp);
    await page.tap('#toasts .toast:last-child');
    const opened = await page.evaluate(() => document.querySelector('#toasts .toast:last-child').clientHeight);
    check(opened > clamp.h, 'a tap opens the rest of the toast', { opened, clamp });
    await page.evaluate(() => { window.evo.model.toasts = []; window.evo.model.emit('toast'); });
    step(`a long toast shows two lines (${clamp.h} px); a tap opens it (${opened} px)`);

    // 1. The renown pill opens the Market.
    await page.tap('#balance');
    await waitFor('the Market in the sheet', () => page.evaluate(() => document.querySelector('.game')?.classList.contains('sheet-open')
      && !document.querySelector('#panel-market').hidden));
    step('the renown pill opens the Market in the bottom sheet');
    // The survey asks in the app's dialog, not a native confirm (playtest 7; the dialog guard catches a native one).
    await waitFor('the survey button', () => page.evaluate(() => !!document.querySelector('#shop-survey')));
    await page.evaluate(() => { const b = document.querySelector('#shop-survey'); b.disabled = false; b.click(); });
    await page.waitForSelector('#survey-confirm', { timeout: UI_MS * TIME_SCALE });
    await page.tap('#survey-confirm-cancel');
    await page.waitForSelector('#survey-confirm', { state: 'detached', timeout: 3000 * TIME_SCALE });
    step('the survey asks in the app\'s dialog (no native confirm)');
    await page.tap('#tab-market'); // a second tap on the open tab closes the sheet
    await waitFor('the sheet closed', () => page.evaluate(() => !document.querySelector('.game').classList.contains('sheet-open')));

    // 2. Past the top edge by the toolbar.
    const edge = await page.evaluate(async () => {
      const v = window.evo.screen().view;
      v.setFollow(false);
      v.cam.zoom = 24;
      v.cam.y = -100;
      for (let k = 0; k < 100 && v.cam.y < 0; k++) await new Promise((r) => setTimeout(r, 30));
      await new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r)));
      const canvas = document.querySelector('#world-canvas').getBoundingClientRect();
      const over = Math.max(document.querySelector('.toolbar').getBoundingClientRect().bottom,
        ...[...document.querySelectorAll('#map-notices > :not([hidden])')].map((n) => n.getBoundingClientRect().bottom));
      return { mapTop: canvas.top + v.worldToScreen(0, 0).y, over, pad: v.edgePad.top };
    });
    check(edge.mapTop >= edge.over, 'the map\'s top edge should come out from under the toolbar and notice', edge);
    step(`the camera goes past the top edge by ${edge.pad} px: the map starts at y ${edge.mapTop.toFixed(0)}, under the toolbar and notice (y ${edge.over.toFixed(0)})`);
    // And past the bottom edge by the hint line and the floating button (with an animal selected, so the button shows).
    const low = await page.evaluate(async () => {
      const v = window.evo.screen().view;
      const own = window.evo.model.state.organisms[0];
      window.evo.model.select(own.id);
      // (Once the animal's card shows, the room at the bottom edge follows it.)
      for (let k = 0; k < 50 && !(v.edgePad.bottom > 60); k++) await new Promise((r) => setTimeout(r, 50));
      v.cam.y = 10000;
      // (Until a frame has clamped it to the map.)
      for (let k = 0; k < 100 && v.cam.y > v.terrain.height; k++) await new Promise((r) => setTimeout(r, 30));
      await new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r)));
      const canvas = document.querySelector('#world-canvas').getBoundingClientRect();
      const tops = ['#hint', '#fab-possess', '#peek', '#possess-bar'].map((q) => document.querySelector(q).getBoundingClientRect()).filter((r) => r.height > 0).map((r) => r.top);
      const under = Math.min(...tops);
      const mapBottom = canvas.top + v.worldToScreen(0, v.terrain.height).y;
      const fabBottom = document.querySelector('#fab-possess').getBoundingClientRect().bottom;
      const peekTop = document.querySelector('#peek').getBoundingClientRect().top;
      window.evo.model.select(null);
      return { mapBottom, under, pad: v.edgePad.bottom, fab: tops.length, fabBottom, peekTop };
    });
    check(low.fab >= 2 && low.mapBottom <= low.under, 'the map\'s bottom edge should come out from above the animal\'s card and the Possess button over it', low);
    check(low.fabBottom <= low.peekTop, 'the Possess button sits above the animal\'s card', low);
    step(`and past the bottom edge by ${low.pad} px: the map ends at y ${low.mapBottom.toFixed(0)}, above the animal's card and Possess button (y ${low.under.toFixed(0)})`);

    // 3. A tap in a herd: the animal under the finger, and a compact chooser.
    await recordTaps(page);
    const canvasBox = await page.locator('#world-canvas').boundingBox();
    const herdTap = await waitFor('a tap on an animal with others in reach', async () => {
      const at = await page.evaluate(() => {
        const v = window.evo.screen().view;
        v.setFollow(false);
        v.cam.zoom = 6; // zoomed out: a crowd under the fingertip, so the chooser has more chips than fit a phone
        const s = window.evo.model.state;
        const own = s.organisms[0];
        v.centerOn(own.x, own.y);
        // An animal with others under the same fingertip, a wild one if there is one.
        const ids = [...s.nearby.filter((o) => o.owner === 0).map((o) => o.id), ...s.organisms.map((o) => o.id)];
        for (const id of ids) {
          const p = v.markerScreenPos(id);
          if (!p || p.y < 120 || p.y > window.innerHeight - 260) continue;
          if (v.hitTest(p.x, p.y, true) === id && v.hitAll(p.x, p.y, true).length >= 5) return { id, ...p };
        }
        return null;
      });
      if (!at) return null;
      await page.evaluate(() => { window.__taps = []; });
      await page.touchscreen.tap(canvasBox.x + at.x, canvasBox.y + at.y);
      const tap = await waitFor('the tap', () => page.evaluate(() => window.__taps[0]), 3000).catch(() => null);
      return tap && tap.organism !== null && tap.candidates.length >= 5 ? { at, tap } : null;
    });
    const picked = await page.evaluate(() => window.evo.model.selected);
    check(picked === herdTap.tap.organism, 'the tap should select the animal under the finger', { picked, tap: herdTap.tap });
    await page.waitForSelector('#pick-chooser:not([hidden])', { timeout: 3000 * TIME_SCALE });
    const chooser = await page.evaluate(() => {
      const c = document.querySelector('#pick-chooser');
      const b = c.getBoundingClientRect();
      const chips = [...c.querySelectorAll('.chip[data-org]')];
      const x = c.querySelector('.chip.close').getBoundingClientRect();
      // The tip card, if one is up, clear of it.
      const tip = document.querySelector('.hint-card');
      const t = tip ? tip.getBoundingClientRect() : null;
      return {
        height: b.height, width: b.width, left: b.left, right: b.right, vw: window.innerWidth, scrolls: c.scrollWidth > c.clientWidth,
        close: { left: x.left, right: x.right }, chips: chips.length, first: Number(chips[0]?.dataset.org),
        rows: new Set(chips.map((x) => Math.round(x.getBoundingClientRect().top))).size,
        tipOver: !!t && t.height > 0 && !(t.bottom <= b.top || t.top >= b.bottom),
        // The tapped animal's card, up with it (below it).
        peek: (() => { const p = document.querySelector('#peek').getBoundingClientRect(); return p.height > 0 ? p.top : null; })(), bottom: b.bottom,
      };
    });
    check(chooser.left >= 0 && chooser.right <= chooser.vw, 'the chooser should fit the screen', chooser);
    check(chooser.close.left >= chooser.left && chooser.close.right <= Math.min(chooser.right, chooser.vw) + 0.5, 'its × should be in reach', chooser);
    check(!chooser.tipOver, 'no tip card over the chooser', chooser);
    check(chooser.peek !== null && chooser.peek >= chooser.bottom, 'the animal\'s card shows below the chooser', chooser);
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
    await page.waitForSelector('#pick-chooser', { state: 'hidden', timeout: 3000 * TIME_SCALE });
    step(`a tap in a herd selects #${picked} (under the finger, of ${herdTap.tap.candidates.length} in reach); the chooser is one ${chooser.height.toFixed(0)} px row, ${chooser.width.toFixed(0)} px wide on a ${chooser.vw} px screen${chooser.scrolls ? ' (scrolling, × pinned)' : ''}, listing it first, gone at the next touch`);
    // The tapped animal's compact card, with the sheet down (playtest 7: nothing showed until the Animal tab opened).
    await page.evaluate((id) => window.evo.model.select(id), picked);
    const peek = await page.evaluate(() => {
      const p = document.querySelector('#peek');
      const b = p.getBoundingClientRect();
      const h = document.querySelector('#hint').getBoundingClientRect();
      return { shown: !p.hidden && b.height > 0, title: p.querySelector('.peek-title').textContent, line: p.querySelector('.peek-line').textContent,
        top: b.top, bottom: b.bottom, vh: window.innerHeight, sheet: document.querySelector('.game').classList.contains('sheet-open'), hint: h.height };
    });
    check(peek.shown && !peek.sheet && peek.title.startsWith(`#${picked} `), 'a tap shows the animal\'s card with the sheet down', peek);
    check(peek.bottom <= peek.vh && peek.hint === 0 && /adult|young/.test(peek.line), 'the card fits, in place of the hint line', peek);
    await page.screenshot({ path: path.join(shots, 'phone-peek.png') });
    await page.tap('#peek-details');
    await waitFor('the inspector in the sheet', () => page.evaluate(() => document.querySelector('.game').classList.contains('sheet-open')
      && !document.querySelector('#panel-organism').hidden && document.querySelector('#peek').hidden));
    await page.tap('#tab-organism'); // closes the sheet: the card is back
    await page.waitForSelector('#peek:not([hidden])', { timeout: 3000 * TIME_SCALE });
    await page.tap('#peek-close');
    await waitFor('the selection cleared', () => page.evaluate(() => window.evo.model.selected === null && document.querySelector('#peek').hidden));
    step(`the tapped animal's card: "${peek.title}" · "${peek.line}"; Details opens the inspector, × closes it`);

    // 4. Follow on: taps on (moving) animals land on them. Real frames for this.
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
    const window4s = () => page.evaluate(async () => {
      const v = window.evo.screen().view;
      const { width, height } = v.app.screen;
      // Follow's dead zone for a herd (worldView FOLLOW_DEAD: a quarter of half the shorter side), in pixels.
      const dead = 0.25 * Math.min(width, height) / 2;
      // Camera travel in frames that began with the herd's centre well inside
      // the dead zone (none is allowed there), and in all frames (for the record).
      let inside = 0, travel = 0, framesInside = 0;
      // Start with the camera on the herd's centre (a herd on the move, to
      // water say, can keep it at the zone's edge, where following is right).
      const c0 = window.evo.model.state.view;
      v.cam.x = c0.x;
      v.cam.y = c0.y;
      await new Promise((r) => requestAnimationFrame(r));
      const t0 = performance.now();
      while (performance.now() - t0 < 4000) {
        const s = window.evo.model.state.view;
        const before = { ...v.cam };
        const offPx = Math.max(Math.abs(s.x - before.x), Math.abs(s.y - before.y)) * before.zoom;
        await new Promise((r) => requestAnimationFrame(r));
        const moved = Math.hypot(v.cam.x - before.x, v.cam.y - before.y) * v.cam.zoom;
        travel += moved;
        if (offPx < dead * 0.9 && window.evo.model.state.view === s) {
          inside += moved;
          framesInside++;
        }
      }
      return { inside, travel, framesInside, dead };
    });
    // (A herd walking somewhere, to water say, is rightly followed: then wait for it to settle.)
    const still = { inside: 0, travel: 0, framesInside: 0, dead: 0, windows: 0 };
    while (still.framesInside < 20 && still.windows < 6) {
      const w = await window4s();
      still.inside += w.inside;
      still.travel += w.travel;
      still.framesInside += w.framesInside;
      still.dead = w.dead;
      still.windows++;
    }
    check(still.framesInside >= 20, 'the herd should spend some frames inside the dead zone within 24 s (else this checks nothing)', still);
    check(still.inside <= 1, 'the camera should hold still while the herd mills about inside the dead zone', still);
    step(`Follow holds the camera still while the herd mills about the middle (${still.inside.toFixed(1)} px of travel over ${still.framesInside} such frames; ${still.travel.toFixed(1)} px in all in ${still.windows * 4} s)`);
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

    // 5. A catastrophe notice, expanded, under the open Tools menu.
    await page.evaluate(() => { try { localStorage.removeItem('evo.notice.catastrophe'); } catch { /* blocked */ } });
    const r = await fetch(`${base}/api/e2e/catastrophe`, { method: 'POST', headers: { 'content-type': 'application/json' }, body: JSON.stringify({ kind: 'severe_winter', days: 60 }) });
    check(r.ok, 'the test hook started a catastrophe', await r.text());
    await page.waitForSelector('#catastrophe-alert:not([hidden])', { timeout: UI_MS * TIME_SCALE });
    // Its text in place (the notice has its full height).
    await waitFor('the notice text', () => page.evaluate(() => document.querySelector('#catastrophe-alert .cat-effect')?.textContent.length > 20));
    // On a phone a notice starts folded to one line (playtest 9: notices and toasts covered half the map); unfold it.
    check(await page.evaluate(() => document.querySelector('#catastrophe-alert').classList.contains('folded')), 'the notice starts folded on a phone');
    await page.tap('#catastrophe-alert .cat-fold');
    await waitFor('the notice unfolded', () => page.evaluate(() => !document.querySelector('#catastrophe-alert').classList.contains('folded')));
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
    await page.waitForSelector('#catastrophe-alert:not([hidden])', { timeout: UI_MS * TIME_SCALE });
    check(await page.evaluate(() => document.querySelector('#catastrophe-alert').classList.contains('folded')), 'the fold should be remembered over a reload');
    step('the notice\'s fold is remembered over a reload');
    // A drought notice as well as the catastrophe one: the second starts folded.
    await page.evaluate(() => { try { localStorage.removeItem('evo.notice.drought'); } catch { /* blocked */ } });
    const both = await waitFor('a drought notice beside the catastrophe', () => page.evaluate(() => {
      const m = window.evo.model;
      if (!m.state) return null;
      // (The world's drought can't be ordered: the state is given one, as a server push would.)
      m.state = { ...m.state, weather: { ...m.state.weather, drought: 0.6 } };
      m.emit('state');
      const d = document.querySelector('#drought-alert');
      return d && !d.hidden ? { folded: d.classList.contains('folded'), cat: !document.querySelector('#catastrophe-alert').hidden } : null;
    }));
    check(both.cat && both.folded, 'with two notices the drought one should start folded', both);
    step('with a catastrophe and a drought notice at once, the drought one starts folded');
    await waitFor('the lineage again', () => page.evaluate(() => (window.evo.model.state?.organisms.length ?? 0) > 1));

    // 6. Possessing says so once.
    const own = await page.evaluate(() => {
      const o = window.evo.model.state.organisms.filter((x) => x.matured).sort((a, b) => a.needs.thirst - b.needs.thirst)[0];
      window.evo.model.select(o.id);
      document.querySelector('#toasts')?.replaceChildren();
      return o.id;
    });
    await page.tap('#fab-possess');
    await page.waitForSelector('#possess-bar:not([hidden])', { timeout: UI_MS * TIME_SCALE });
    await page.waitForTimeout(500);
    const said = await page.evaluate(() => ({ bar: document.querySelector('#possess-bar').textContent, toasts: document.querySelector('#toasts')?.textContent ?? '' }));
    check(said.bar.includes(`#${own}`), 'the bar names the possessed animal', said);
    check(await page.locator('#fab-possess').isHidden(), 'one Release while possessing (the bar\'s), not a second floating one');
    check(!/Possessing #/.test(said.toasts), 'no toast repeating the bar', said);
    await page.screenshot({ path: path.join(shots, 'phone-possessing.png') });
    // The chooser while possessing (a long press in a herd selects and offers
    // the others): above the bar, not over its buttons (playtest 5).
    const stacked = await page.evaluate(async (own) => {
      const v = window.evo.screen().view;
      const ids = window.evo.model.state.organisms.map((o) => o.id).filter((id) => id !== own).slice(0, 4);
      const m = v.markers.get(ids[0]);
      await v.onClick({ x: m.x, y: m.y, organism: ids[0], candidates: ids, modifier: true, touch: true });
      await new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r)));
      const c = document.querySelector('#pick-chooser');
      const cb = c.getBoundingClientRect();
      const buttons = [...document.querySelectorAll('#possess-bar button')].map((b) => b.getBoundingClientRect()).filter((r) => r.height > 0);
      const over = buttons.some((r) => !(r.bottom <= cb.top || r.top >= cb.bottom || r.right <= cb.left || r.left >= cb.right));
      return { shown: !c.hidden, over, chooser: [cb.top, cb.bottom], bar: buttons.map((r) => [r.top, r.bottom]) };
    }, own);
    check(stacked.shown && !stacked.over, 'while possessing, the chooser should sit clear of the possession bar\'s buttons', stacked);
    await page.tap('#possess-release');
    await page.waitForSelector('#possess-bar', { state: 'hidden', timeout: UI_MS * TIME_SCALE });
    // Possessing again (the chooser still up from before): it goes.
    if (await page.locator('#pick-chooser').isHidden()) {
      await page.evaluate(async (own) => {
        const v = window.evo.screen().view;
        const ids = window.evo.model.state.organisms.map((o) => o.id).slice(0, 4);
        const m = v.markers.get(ids[0]);
        await v.onClick({ x: m.x, y: m.y, organism: ids[0], candidates: ids, modifier: false, touch: true });
        window.evo.model.select(own);
      }, own);
    }
    check(await page.locator('#pick-chooser').isVisible(), 'the chooser is up before possessing');
    await page.evaluate((own) => window.evo.model.select(own), own);
    await page.tap('#fab-possess');
    await page.waitForSelector('#possess-bar:not([hidden])', { timeout: UI_MS * TIME_SCALE });
    await page.waitForSelector('#pick-chooser', { state: 'hidden', timeout: 3000 * TIME_SCALE });
    await page.tap('#possess-release');
    await page.waitForSelector('#possess-bar', { state: 'hidden', timeout: UI_MS * TIME_SCALE });
    step(`possessing #${own}: the bar says so, no toast repeats it; the chooser sits above the bar and goes when a possession starts`);
    // Possess from the Animal tab in the sheet: the sheet closes, so the bar's Release is on top
    // (playtest 10: "Release is on the bar", hidden under the half-open sheet).
    await page.evaluate((own) => window.evo.model.select(own), own);
    await page.tap('#tab-organism');
    await page.waitForSelector('#btn-possess', { state: 'visible', timeout: UI_MS });
    await page.tap('#btn-possess');
    await page.waitForSelector('#possess-bar:not([hidden])', { timeout: UI_MS });
    const release = await waitFor('Release on top', () => page.evaluate(() => {
      const b = document.querySelector('#possess-release');
      const r = b.getBoundingClientRect();
      if (r.height === 0) return null;
      const at = document.elementFromPoint(r.left + r.width / 2, r.top + r.height / 2);
      return at && (at === b || b.contains(at)) ? { top: Math.round(r.top) } : null;
    }));
    await page.tap('#possess-release');
    await page.waitForSelector('#possess-bar', { state: 'hidden', timeout: UI_MS });
    step(`possessed from the Animal tab: the sheet closed and the bar's Release is on top (y ${release.top})`);

    // 7. Split, up to the limit of four groups.
    const send = (m) => page.evaluate((m) => window.evo.socket.send(m), m);
    const groups = [0];
    for (const from of [0, 0, 1]) {
      const res = await send({ type: 'split_group', group: groups[from] });
      check(res.ok, 'split', res);
      groups.push(res.data.group);
    }
    await waitFor('four groups listed', () => page.evaluate(() => (window.evo.model.state.groups ?? []).filter((g) => g.members > 0).length === 4));
    await waitFor('Split unavailable', () => page.evaluate(() => document.querySelector('#split-group').getAttribute('aria-disabled') === 'true'));
    const title = await page.getAttribute('#split-group', 'title');
    // A tap says why (phones show no tooltips), and splits nothing.
    await page.tap('#tools-more').catch(() => {});
    if (await page.isVisible('#split-group')) {
      // (aria-disabled: Playwright waits for "enabled" unless forced; a finger doesn't.)
      await page.tap('#split-group', { force: true });
      await waitFor('the reason as a toast', () => page.evaluate(() => [...document.querySelectorAll('#toasts > *')].some((t) => /4 groups, the most/.test(t.textContent))));
      await page.tap('#tools-more').catch(() => {});
    }
    check(await page.evaluate(() => (window.evo.model.state.groups ?? []).filter((g) => g.members > 0).length) === 4, 'still four groups');
    step(`Split is unavailable at four groups and a tap says why ("${title}")`);

    // 7b. A pinned view says so, and the button knows it after a reload.
    await page.tap('#tools-more');
    await page.tap('#pin-view');
    await waitFor('the view pinned', () => page.evaluate(() => window.evo.model.state?.view.pinned === true));
    await waitFor('the pinned hint', () => page.evaluate(() => /view is pinned/.test(document.querySelector('#hint')?.textContent ?? '')));
    check(await page.evaluate(() => document.querySelector('#pin-view').classList.contains('active')), 'the button shows Unpin view');
    // Pinned from elsewhere (an API call on this connection): the button follows the state.
    await page.tap('#tools-more');
    await page.tap('#pin-view');
    await waitFor('unpinned', () => page.evaluate(() => !window.evo.model.state?.view.pinned && !document.querySelector('#pin-view').classList.contains('active')));
    await page.evaluate(() => window.evo.socket.send({ type: 'set_view', x: window.evo.model.state.view.x, y: window.evo.model.state.view.y, radius: 20 }));
    await waitFor('the button pinned by the state', () => page.evaluate(() => document.querySelector('#pin-view').classList.contains('active')));
    await page.evaluate(() => window.evo.socket.send({ type: 'clear_view' }));
    step('a pinned view says so (hint, Unpin view), also when pinned by an API call');

    // 7c. View group: the view window goes to each group in turn (playtest 7: a second party saw nothing), then back.
    await waitFor('unpinned again', () => page.evaluate(() => !window.evo.model.state?.view.pinned));
    const centre = (g) => page.evaluate((g) => {
      const os = window.evo.model.state.organisms.filter((o) => (o.group ?? 0) === g);
      return os.length ? { x: os.reduce((a, o) => a + o.x, 0) / os.length, y: os.reduce((a, o) => a + o.y, 0) / os.length } : null;
    }, g);
    const order = await page.evaluate(() => window.evo.model.state.groups.filter((g) => g.members > 0).map((g) => g.id));
    const seen = [];
    const label = () => page.evaluate(() => document.querySelector('#view-group .tool-label').textContent);
    for (const g of order.slice(0, 2)) {
      const before = await label();
      await page.tap('#tools-more');
      await page.tap('#view-group');
      // The button names the new group: groups split from one another can stand
      // within a few tiles, so the view being near this group's middle alone
      // passed before the tap had landed.
      await waitFor(`the view on group ${g}`, async () => {
        const c = await centre(g);
        return page.evaluate(([c, before]) => {
          const v = window.evo.model.state?.view;
          const b = document.querySelector('#view-group');
          return !!c && v?.pinned && Math.hypot(v.x - c.x, v.y - c.y) <= 12 && b.classList.contains('active')
            && b.querySelector('.tool-label').textContent !== before;
        }, [c, before]);
      });
      seen.push(await label());
    }
    await waitFor('the hint names the viewed group', () => page.evaluate(() => /on Group B/.test(document.querySelector('#hint').textContent)));
    // Unpin view ends it.
    await page.tap('#tools-more');
    await page.tap('#pin-view');
    await waitFor('back to the lineage', () => page.evaluate(() => !window.evo.model.state?.view.pinned
      && !document.querySelector('#view-group').classList.contains('active') && document.querySelector('#view-group .tool-label').textContent === 'View group'));
    step(`View group moves the view window to each group in turn (${seen.join(', ')}); Unpin view goes back to the lineage`);

    // 7d. Naming a species: the price shows, and a refused name keeps the dialog open with why (playtest 7).
    await page.tap('#tab-world');
    const nameBtn = await waitFor('the Name it… button', async () => (await page.locator('[data-action="name-species"]').count()) > 0, UI_MS).catch(() => false);
    if (nameBtn) {
      await page.locator('[data-action="name-species"]').first().tap();
      await page.waitForSelector('#species-name', { timeout: UI_MS * TIME_SCALE });
      const body = await page.textContent('#species-name');
      check(/costs \d+ renown/.test(body), 'the dialog shows the price', body);
      await page.fill('#species-name-input', 'ClaudeBot');
      await page.tap('#species-name-ok');
      await page.waitForSelector('#species-name-error:not([hidden])', { timeout: UI_MS * TIME_SCALE });
      const why = await page.textContent('#species-name-error');
      check(await page.locator('#species-name').isVisible(), 'a refusal keeps the dialog open', why);
      check(!/Name species|name_species/i.test((await page.textContent('#toasts')) ?? ''), 'and is not toasted as well');
      await page.tap('#species-name-cancel');
      await page.waitForSelector('#species-name', { state: 'detached', timeout: 3000 * TIME_SCALE });
      step(`the species-name dialog shows the price; a refused name stays in it ("${why}")`);
    } else {
      step('(no species to name yet: the species-name dialog check skipped)');
    }
    await page.tap('#tab-world');

    // 8. A returning player in a new browser profile (no local progress): no
    // first-session tour (it came back in every new browser, playtest 4).
    const other = await browser.newContext({ ...chromiumDevice(devices, 'iPhone 13') });
    const again = await other.newPage();
    again.on('pageerror', (e) => errors.push(`second browser: ${e.message}`));
    await again.goto(`${base}/`); // (no ?tour=off)
    await again.fill('#login-name', 'pocket_phone');
    await again.fill('#login-password', 'a long password');
    await again.tap('#login-submit');
    await again.waitForSelector('#world-canvas', { timeout: UI_MS * TIME_SCALE });
    await waitFor('the game ready', () => again.evaluate(() => (window.evo?.model.state?.organisms.length ?? 0) > 0));
    await again.waitForTimeout(1500);
    check(!(await again.locator('#tour-title').isVisible().catch(() => false)), 'the tour should not start for a returning player');
    await other.close();
    step('a returning player in a new browser is not shown the first-session tour');

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
