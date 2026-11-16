// End-to-end: the body builder on its dev harness (`/?builder`), no login.
// Starts its own evo-server like smoke.mjs (only to serve client/dist).
//
// On a desktop (mouse + keyboard) and two phones (iPhone 13, Pixel 7; touch):
// the default eyes read 72% view, 20° depth, 100° blind behind; dragging the
// right eye to the side moves its mirrored partner and trades the rear blind
// spot for one straight ahead; adding a third (midline) eye closes the front
// one; undo / redo; a long snout hides depth; a preset sees all round; reset.
// The page never scrolls sideways and every handle is a 44 px target.
// Desktop also checks the keyboard: arrows move an eye, + / − add and remove.
// Screenshots: e2e/out/builder-*.png.
//
//   npm run build && node e2e/builder.mjs     (env as for smoke.mjs)

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

function check(cond, msg, detail) {
  if (!cond) throw new Error(`${msg}${detail === undefined ? '' : `: ${JSON.stringify(detail)}`}`);
}

/** The readouts and the value. */
const state = (page) => page.evaluate(() => {
  const num = (k) => Number(document.querySelector(`[data-stat="${k}"]`).dataset.value);
  const b = window.evoBuilder.builder;
  return {
    coverage: num('coverage'), depth: num('overlap'), rear: num('rear-blind'), front: num('front-blind'),
    value: b.value, blind: b.vision.blindDeg, canUndo: b.canUndo,
  };
});

/** Screen position of a point given in a drawing's own units. */
const toScreen = (page, svgId, x, y) => page.evaluate(([id, x, y]) => {
  const svg = document.getElementById(id);
  const p = new DOMPoint(x, y).matrixTransform(svg.getScreenCTM());
  return { x: p.x, y: p.y };
}, [svgId, x, y]);

const center = async (page, sel) => {
  const r = await page.locator(sel).boundingBox();
  return { x: r.x + r.width / 2, y: r.y + r.height / 2 };
};

/** A drag by mouse, or by one finger (CDP touch events, which the page sees as pointer events). */
async function drag(page, cdp, from, to) {
  const steps = 8;
  if (!cdp) {
    await page.mouse.move(from.x, from.y);
    await page.mouse.down();
    for (let i = 1; i <= steps; i++) await page.mouse.move(from.x + ((to.x - from.x) * i) / steps, from.y + ((to.y - from.y) * i) / steps);
    await page.mouse.up();
    return;
  }
  const send = (type, points) => cdp.send('Input.dispatchTouchEvent', { type, touchPoints: points });
  await send('touchStart', [{ x: from.x, y: from.y, id: 1 }]);
  for (let i = 1; i <= steps; i++) await send('touchMove', [{ x: from.x + ((to.x - from.x) * i) / steps, y: from.y + ((to.y - from.y) * i) / steps, id: 1 }]);
  await send('touchEnd', []);
}

async function press(page, touch, sel) {
  if (touch) {
    await page.locator(sel).scrollIntoViewIfNeeded();
    const c = await center(page, sel);
    await page.touchscreen.tap(c.x, c.y);
  } else {
    await page.click(sel);
  }
}

/** Polar point in the top view (0° = up, clockwise). */
const polar = (a, r) => [r * Math.sin((a * Math.PI) / 180), -r * Math.cos((a * Math.PI) / 180)];

async function checkLayout(page, tag) {
  const l = await page.evaluate(() => {
    const scroller = document.querySelector('.bb');
    const hits = [...document.querySelectorAll('.bb-handle .bb-hit')].map((c) => {
      const r = c.getBoundingClientRect();
      return { id: c.parentElement.id, w: r.width, h: r.height };
    });
    const buttons = [...document.querySelectorAll('.bb button')].filter((b) => b.offsetParent).map((b) => {
      const r = b.getBoundingClientRect();
      return { t: b.textContent || b.title, w: r.width, h: r.height };
    });
    return {
      vw: document.documentElement.clientWidth, docW: document.documentElement.scrollWidth,
      scrollW: scroller.scrollWidth, clientW: scroller.clientWidth, hits, buttons,
    };
  });
  check(l.docW <= l.vw + 1 && l.scrollW <= l.clientW + 1, `${tag}: scrolls sideways`, l);
  const small = l.hits.filter((x) => x.w < 44 || x.h < 44);
  check(small.length === 0, `${tag}: handle targets under 44 px`, small);
  const smallButtons = l.buttons.filter((x) => x.w < 24 || x.h < 24);
  check(smallButtons.length === 0, `${tag}: buttons under 24 px`, smallButtons);
  return l;
}

async function run(browser, base, tag, contextOpts, touch) {
  const context = await browser.newContext(contextOpts);
  const page = await context.newPage();
  const errors = [];
  page.on('pageerror', (e) => errors.push(e.message));
  const cdp = touch ? await context.newCDPSession(page) : null;
  await page.goto(`${base}/?builder&tour=off`);
  await page.waitForSelector('#bb-top', { timeout: UI_MS });

  let s = await state(page);
  check(s.rear === 100 && s.front === 0 && s.depth === 20 && Math.round(s.coverage * 100) === 72, `${tag}: default readouts`, s);
  const l = await checkLayout(page, tag);
  step(`${tag}: default eyes 72% view, 20° depth, 100° blind behind; no sideways scroll; ${l.hits.length} handles ≥ 44 px`);
  await page.screenshot({ path: path.join(shots, `builder-${tag}.png`), fullPage: true });

  // Drag the right eye from 60° to the side (90°).
  await page.locator('#bb-h-eye-0').scrollIntoViewIfNeeded();
  const leftBefore = await center(page, '#bb-h-eye-1');
  const from = await center(page, '#bb-h-eye-0');
  const to = await toScreen(page, 'bb-top', ...polar(90, 35));
  await drag(page, cdp, from, to);
  s = await waitFor('the eye to move', async () => {
    const st = await state(page);
    return Math.abs(st.value.eye_azimuth - 90) <= 3 ? st : null;
  }, UI_MS);
  const leftAfter = await center(page, '#bb-h-eye-1');
  check(leftAfter.y > leftBefore.y + 3 && leftAfter.x < from.x, `${tag}: the mirrored eye didn't follow`, { leftBefore, leftAfter });
  check(s.rear < 100 && s.front > 0, `${tag}: side eyes should trade the rear blind spot for one ahead`, s);
  const final = await page.evaluate(() => window.evoBuilder.changes.filter((c) => c.change.source === 'drag' && c.change.final).length);
  check(final === 1, `${tag}: one final change per drag`, final);
  step(`${tag}: dragged the eye to ${s.value.eye_azimuth}° (partner mirrored): blind behind ${s.rear}°, ahead ${s.front}°`);
  await page.screenshot({ path: path.join(shots, `builder-${tag}-side-eyes.png`), fullPage: true });

  // A third eye on the midline closes the front blind spot.
  const before = s;
  await press(page, touch, '#bb-eye-plus');
  s = await waitFor('a third eye', async () => {
    const st = await state(page);
    return st.value.eye_count === 3 ? st : null;
  }, UI_MS);
  check(s.front === 0 && s.blind < before.blind && s.rear === before.rear && s.depth > before.depth, `${tag}: third eye`, { before, s });
  check(await page.locator('.bb-handle[data-handle^="eye-"]').count() === 4, `${tag}: three eye handles and the size knob`);
  step(`${tag}: third eye: blind spot ${before.blind}° → ${s.blind}° (ahead ${before.front}° → 0°), depth ${s.depth}°`);
  await page.screenshot({ path: path.join(shots, `builder-${tag}-three-eyes.png`), fullPage: true });

  // Undo / redo.
  if (touch) await press(page, true, '#bb-undo');
  else {
    await page.focus('#bb-eye-plus');
    await page.keyboard.press('Control+z');
  }
  await waitFor('undo', async () => (await state(page)).value.eye_count === 2, UI_MS);
  if (touch) await press(page, true, '#bb-redo');
  else await page.keyboard.press('Control+Shift+z');
  await waitFor('redo', async () => (await state(page)).value.eye_count === 3, UI_MS);
  step(`${tag}: undo / redo ${touch ? 'buttons' : 'Ctrl+Z / Ctrl+Shift+Z'}`);

  if (!touch) {
    // Keyboard: the right eye's handle (index 1 now: the midline eye comes first).
    const az = (await state(page)).value.eye_azimuth;
    await page.focus('#bb-h-eye-1');
    await page.keyboard.press('ArrowDown');
    await page.keyboard.press('ArrowDown');
    check((await state(page)).value.eye_azimuth === az + 10, `${tag}: arrows move the eye`, await state(page));
    await page.keyboard.press('ArrowUp');
    await page.keyboard.press('ArrowUp');
    await page.keyboard.press('+');
    check((await state(page)).value.eye_count === 4, `${tag}: + adds an eye`);
    await page.keyboard.press('-');
    check((await state(page)).value.eye_count === 3, `${tag}: - removes an eye`);
    const vt = await page.getAttribute('#bb-h-eye-1', 'aria-valuetext');
    check(/view, .*depth/.test(vt), `${tag}: aria-valuetext`, vt);
    // Keyboard on a side-view handle: + / − add a pair of ears.
    await page.focus('#bb-h-ears');
    await page.keyboard.press('+');
    check((await state(page)).value.ear_count === 4, `${tag}: + on the ear handle adds a pair`);
    await page.keyboard.press('-');
    step(`${tag}: keyboard: arrows move the eye, + / − add and remove (“${vt}”)`);
  }

  // A long snout hides part of the depth zone.
  const depthBefore = (await state(page)).depth;
  const snout = await center(page, '#bb-h-snout');
  const snoutEnd = await toScreen(page, 'bb-side', 212 + 58 + 6, 76);
  await page.locator('#bb-h-snout').scrollIntoViewIfNeeded();
  await drag(page, cdp, await center(page, '#bb-h-snout'), { x: snoutEnd.x + (await center(page, '#bb-h-snout')).x - snout.x, y: (await center(page, '#bb-h-snout')).y });
  s = await waitFor('a long snout', async () => {
    const st = await state(page);
    return st.value.snout_length >= 0.95 ? st : null;
  }, UI_MS);
  check(Math.abs(s.depth - (depthBefore - 20 * (s.value.snout_length - 0.5) * 2)) < 0.01, `${tag}: snout occlusion`, { depthBefore, s });
  step(`${tag}: snout to ${s.value.snout_length}: depth ${depthBefore}° → ${s.depth}°`);

  // Side-view pickers and a leg drag.
  await press(page, touch, '[data-group="legs"][data-value="6"]');
  await press(page, touch, '[data-group="tail"][data-value="bushy"]');
  await press(page, touch, '[data-group="ear-shape"][data-value="pointed"]');
  await page.locator('#bb-h-legs').scrollIntoViewIfNeeded();
  const leg = await center(page, '#bb-h-legs');
  await drag(page, cdp, leg, { x: leg.x, y: leg.y + 200 });
  s = await waitFor('the side-view edits', async () => {
    const st = await state(page);
    return st.value.leg_count === 6 && st.value.tail_present && st.value.tail_shape === 'bushy' && st.value.ear_shape === 'pointed' && st.value.leg_length === 1 ? st : null;
  }, UI_MS);
  step(`${tag}: 6 legs, bushy tail, pointed ears, legs dragged to full length`);
  await page.screenshot({ path: path.join(shots, `builder-${tag}-body.png`), fullPage: true });

  // A preset that sees all round, then reset.
  await press(page, touch, '[data-preset="watcher"]');
  s = await waitFor('the watcher preset', async () => {
    const st = await state(page);
    return st.value.eye_count === 3 && st.value.eye_azimuth === 130 ? st : null;
  }, UI_MS);
  check(s.rear === 0 && s.front === 0 && s.coverage === 1, `${tag}: watcher sees all round`, s);
  await checkLayout(page, `${tag} (watcher)`);
  await page.screenshot({ path: path.join(shots, `builder-${tag}-watcher.png`), fullPage: true });
  await press(page, touch, '#bb-reset');
  s = await waitFor('reset', async () => {
    const st = await state(page);
    return st.value.eye_count === 2 && st.rear === 100 ? st : null;
  }, UI_MS);
  step(`${tag}: watcher preset sees all round; reset back to the default`);

  check(errors.length === 0, `${tag}: page errors`, errors);
  await context.close();
}

async function main() {
  if (!existsSync(bin)) throw new Error(`no server binary at ${bin}; run cargo build --release -p evo-server`);
  if (!existsSync(path.join(dist, 'index.html'))) throw new Error('no client/dist; run npm run build');
  mkdirSync(shots, { recursive: true });
  const port = await freePort();
  const base = `http://127.0.0.1:${port}`;
  const server = spawn(bin, ['--fast', '--memory', '--size', '32', '--seed', '7', '--bind', `127.0.0.1:${port}`, '--static', dist], {
    stdio: ['ignore', 'pipe', 'pipe'],
  });
  let serverLog = '';
  server.stdout.on('data', (d) => (serverLog += d));
  server.stderr.on('data', (d) => (serverLog += d));
  const browser = await chromium.launch({ headless: !process.env.HEADED });
  try {
    await waitFor('server', async () => (await fetch(`${base}/api/world`)).ok);
    step(`server up on ${base}`);
    await run(browser, base, 'desktop', { viewport: { width: 1280, height: 900 }, deviceScaleFactor: 1 }, false);
    await run(browser, base, 'iphone13', device('iPhone 13'), true);
    await run(browser, base, 'pixel7', device('Pixel 7'), true);
    console.log('builder e2e passed');
  } catch (e) {
    console.error(serverLog.slice(-2000));
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
