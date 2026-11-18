// End-to-end: the attack order while possessing (`possess_attack`). Starts
// its own evo-server (fast, in memory, with the wild ecosystem).
//
// A player founds the starting design and possesses one of its adults; the
// possession bar has an Attack button that switches to attack mode (the hint
// says so; a plain tap would walk); a tap on one of their own animals in
// attack mode is refused in the interface and stays in attack mode; a tap on
// a wild animal in sight sends the order (a toast says it is hunting, and the
// possessed animal hunts) or, if the server refuses (in a burrow, too big),
// says why in a friendly toast and another is tried. The inspector of a
// wild animal offers "Attack with #id" while possessing. Over the socket: an
// attack on one's own animal is refused (`own_target`), and one on a second
// player's freshly founded animal is refused (`newcomer_grace`, or
// `target_out_of_sight` if it is too far). Releasing hides the bar and ends
// attack mode. Screenshots: e2e/out/attack-*.png.
//
//   npm run build && node e2e/attack.mjs     (env as for smoke.mjs)

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

async function register(page, base, name) {
  await page.goto(`${base}/?tour=off`);
  await page.fill('#login-name', name);
  await page.fill('#login-password', 'a long password');
  await page.click('#register-submit');
  await page.waitForSelector('#design-name', { timeout: UI_MS });
  await page.fill('#design-name', name);
  await page.click('#design-submit');
  await page.waitForSelector('#world-canvas', { timeout: UI_MS });
  return waitFor(`${name}'s lineage`, () => page.evaluate(() => {
    const s = window.evo.model.state;
    return s && s.lineage !== null && s.organisms.length > 0 ? s.lineage : null;
  }));
}

const send = (page, msg) => page.evaluate((msg) => window.evo.socket.send(msg), msg);
const toastText = (page) => page.evaluate(() => document.querySelector('#toasts')?.textContent ?? '');
/** The toast answering an attack order on `target`: "… is hunting #target …" or "Attack: …". */
const attackAnswer = (page, target) => page.evaluate((target) => [...document.querySelectorAll('#toasts > *')]
  .map((t) => t.textContent ?? '')
  .find((t) => t.includes(`is hunting #${target};`) || t.startsWith('Attack:')) ?? null, target);
const possessedNow = (page) => page.evaluate(() => {
  const o = window.evo.model.state.organisms.find((x) => x.possessed);
  return o ? { id: o.id, x: o.x, y: o.y, action: o.action } : null;
});

/** Possess a grown animal of ours, the least thirsty (founders here can die of thirst within the test): its id. */
async function possessOne(page) {
  const id = await page.evaluate(() => {
    const own = window.evo.model.state.organisms.filter((o) => o.matured)
      .sort((a, b) => a.needs.thirst - b.needs.thirst || b.health - a.health);
    return own[0]?.id ?? window.evo.model.state.organisms[0]?.id;
  });
  const r = await send(page, { type: 'possess', organism: id });
  check(r.ok, 'possess', r);
  await waitFor('the possession bar', () => page.locator('#possess-bar').isVisible());
  return id;
}

/**
 * Possessing someone, with the possession bar up. Possession ends on its own
 * when the possessed animal dies (the bar then hides, and so does Attack):
 * that is the world, not the interface, so the test possesses another and
 * says so rather than clicking a hidden button until it times out.
 */
async function ensurePossessed(page, why) {
  const p = await possessedNow(page);
  if (p && (await page.locator('#possess-bar').isVisible())) return p.id;
  const id = await possessOne(page);
  step(`possession had ended${why ? ` (${why})` : ''}; possessed #${id}`);
  return id;
}

/** Attack mode on, from the possession bar's button. */
async function attackMode(page) {
  for (let attempt = 0; attempt < 3; attempt++) {
    await ensurePossessed(page, 'before Attack');
    if ((await page.getAttribute('#possess-attack', 'aria-pressed')) === 'true') return;
    try {
      await page.click('#possess-attack', { timeout: 3000 });
    } catch (e) {
      const bar = await page.locator('#possess-bar').isVisible();
      step(`Attack click missed (possession bar ${bar ? 'visible' : 'hidden'}): retrying`);
      if (bar) throw e;
    }
  }
  check((await page.getAttribute('#possess-attack', 'aria-pressed')) === 'true', 'attack mode should be on');
}

/** Wild animals in sight of `p` (within 7 tiles), nearest first. */
const wildNear = (page, p) => page.evaluate(([px, py]) => window.evo.model.state.nearby
  .filter((o) => o.owner === 0 && Math.hypot(o.x - px, o.y - py) <= 7)
  .sort((a, b) => Math.hypot(a.x - px, a.y - py) - Math.hypot(b.x - px, b.y - py))
  .map((o) => o.id), [p.x, p.y]);

/** Click marker `id` on the canvas (zoomed in, centred, checked with the hit test). */
async function clickMarker(page, id) {
  const box = await page.locator('#world-canvas').boundingBox();
  for (let attempt = 0; attempt < 6; attempt++) {
    const p = await page.evaluate((id) => {
      const v = window.evo.screen().view;
      v.setFollow(false);
      v.cam.zoom = 16;
      const m = v.markers.get(id);
      if (!m) return null;
      v.centerOn(m.x, m.y);
      return true;
    }, id);
    if (!p) return false;
    await page.waitForTimeout(120);
    const at = await page.evaluate((id) => {
      const v = window.evo.screen().view;
      const p = v.markerScreenPos(id);
      return p && v.hitTest(p.x, p.y) === id ? p : null;
    }, id);
    if (!at) continue;
    await page.mouse.click(box.x + at.x, box.y + at.y);
    return true;
  }
  return false;
}

async function main() {
  if (!existsSync(bin)) throw new Error(`no server binary at ${bin} (cargo build --release -p evo-server)`);
  if (!existsSync(path.join(dist, 'index.html'))) throw new Error('no client build (npm run build)');
  mkdirSync(shots, { recursive: true });
  const port = await freePort();
  const base = `http://127.0.0.1:${port}`;
  // A slow clock with a state push every tick: under --fast alone (50 ticks a
  // second, state every 5 ticks) the positions the page shows lag the
  // server's by several tiles, so a target in sight on the page could be out
  // of sight on the server, and the possessed animal could die of thirst
  // between two steps of the test.
  const server = spawn(bin, ['--fast', '--tick-ms', '250', '--state-ms', '250', '--memory', '--size', '64', '--seed', '3', '--bind', `127.0.0.1:${port}`, '--static', dist], {
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
    const lineage = await register(page, base, `hunter_${Date.now().toString(36)}`);
    step(`founded lineage #${lineage}`);

    // Possess one, and attack mode from the possession bar.
    let me = await possessOne(page);
    check(await page.locator('#possess-attack').isVisible(), 'the possession bar should have an Attack button');
    await attackMode(page);
    check(await page.getAttribute('#possess-attack', 'aria-pressed') === 'true', 'Attack should be pressed');
    const hint = await page.textContent('#possess-bar');
    check(/click the animal to attack/i.test(hint), 'the possession bar should say how to attack', hint);
    await page.screenshot({ path: path.join(shots, 'attack-mode.png') });
    step(`possessed #${me}; Attack switches to attack mode ("${hint.trim()}")`);

    // One of ours in attack mode: refused in the interface, still in attack mode.
    // Animals move every frame, so the tap is checked for what it landed on
    // (the view's click: the organism under it and the others in reach): on
    // one of ours alone it must be refused; anything else (a miss, or a wild
    // animal beside ours, which the tap rightly attacks) is tried again.
    await page.evaluate(() => {
      const v = window.evo.screen().view;
      const orig = v.onClick;
      v.onClick = (c) => {
        window.__lastClick = { organism: c.organism, candidates: c.candidates ?? [] };
        return orig(c);
      };
    });
    let refused = null;
    for (let attempt = 0; attempt < 5 && !refused; attempt++) {
      await attackMode(page);
      me = await ensurePossessed(page, 'before tapping one of ours');
      // One of ours, preferably with no other player's or wild animal close by.
      const mate = await page.evaluate((me) => {
        const s = window.evo.model.state;
        const others = s.nearby.filter((o) => o.owner !== s.player);
        const clear = (o) => others.every((x) => Math.hypot(x.x - o.x, x.y - o.y) > 3);
        const mine = s.organisms.filter((o) => o.id !== me);
        return (mine.find(clear) ?? mine[0])?.id ?? null;
      }, me);
      if (mate === null) break;
      await page.evaluate(() => { document.querySelector('#toasts')?.replaceChildren(); window.__lastClick = null; });
      if (!(await clickMarker(page, mate))) continue;
      const tap = await waitFor('the tap', () => page.evaluate(() => window.__lastClick), 5000);
      const ours = await page.evaluate((ids) => ids.map((id) => window.evo.model.state.organisms.some((o) => o.id === id)), [tap.organism, ...tap.candidates].filter((x) => x !== null));
      if (tap.organism === null || !ours.every(Boolean)) {
        step(`the tap landed on ${tap.organism === null ? 'nothing' : `#${tap.organism} with ${JSON.stringify(tap.candidates)} in reach`}, not on one of ours alone: again`);
        continue;
      }
      await waitFor('the own-animal notice', async () => /one of yours/.test(await toastText(page)), UI_MS);
      check(await page.getAttribute('#possess-attack', 'aria-pressed') === 'true', 'attack mode should stay on after tapping one of ours');
      refused = tap.organism;
    }
    if (refused !== null) step(`a tap on our own #${refused} in attack mode is refused and attack mode stays on`);
    else step('no other animal of ours to tap: skipped the own-animal tap');

    // A wild animal in sight: walk toward the nearest wild one first if none.
    let ordered = null;
    for (let round = 0; round < 8 && !ordered; round++) {
      me = await ensurePossessed(page, 'looking for prey');
      const p = await possessedNow(page);
      if (!p) continue;
      const wild = await wildNear(page, p);
      if (wild.length === 0) {
        const far = await page.evaluate(([px, py]) => window.evo.model.state.nearby.filter((o) => o.owner === 0)
          .sort((a, b) => Math.hypot(a.x - px, a.y - py) - Math.hypot(b.x - px, b.y - py))[0], [p.x, p.y]);
        if (far) await send(page, { type: 'order_move', organism: p.id, x: far.x, y: far.y });
        await page.waitForTimeout(2500);
        continue;
      }
      for (const target of wild.slice(0, 4)) {
        await attackMode(page);
        await page.evaluate(() => { document.querySelector('#toasts')?.replaceChildren(); });
        if (!(await clickMarker(page, target))) continue;
        const said = await waitFor('an answer to the attack order', () => attackAnswer(page, target), 8000).catch(() => '');
        if (said.includes('is hunting')) {
          ordered = { target, said };
          break;
        }
        if (said) step(`refused, as the toast says: "${said.trim()}"`);
      }
    }
    check(ordered, 'no attack order went through on a wild animal');
    check(await page.getAttribute('#possess-attack', 'aria-pressed') === 'false', 'attack mode should end once the order is sent');
    const hunting = await waitFor('the possessed animal to hunt', async () => {
      const p = await possessedNow(page);
      return p && p.action === 'hunt' ? p : null;
    }, 8000).catch(() => null);
    await page.screenshot({ path: path.join(shots, 'attack-ordered.png') });
    step(`tapped wild #${ordered.target}: "${ordered.said.trim()}"${hunting ? `, and #${hunting.id} is hunting` : ' (the order already ended: caught, lost or broken off)'}`);

    // The inspector of a wild animal offers the attack while possessing.
    // The button goes with the possession (the inspector is rebuilt for the
    // new possessor), so a possession that ends mid-step is retried.
    let inspected = null;
    for (let attempt = 0; attempt < 8 && !inspected; attempt++) {
      await ensurePossessed(page, 'before the inspector');
      const p = await possessedNow(page);
      const wild = await wildNear(page, p);
      const other = (await page.evaluate(() => window.evo.model.state.nearby.filter((o) => o.owner === 0).map((o) => o.id)))[0];
      const pick = wild[0] ?? other;
      check(pick !== undefined, 'no wild animal in view for the inspector');
      await page.evaluate((id) => window.evo.model.select(id), pick);
      await page.click('#tab-organism');
      try {
        await page.waitForSelector('#btn-attack', { timeout: 5000 });
        const label = await page.textContent('#btn-attack');
        check(label.includes(`Attack with #${p.id}`), 'the inspector attack button', label);
        if (await page.locator('#btn-attack').isDisabled()) {
          // Out of reach (more than 8 tiles): the button says so instead of a pointless click.
          check(label.includes('too far'), 'a disabled attack button should say why', label);
          step(`the inspector of wild #${pick} offers "${label}", disabled: it is out of reach`);
          // Walk up to it (both ways: it was only sent along x), until within reach or a while has passed.
          const at = await page.evaluate((id) => window.evo.model.state.nearby.find((o) => o.id === id), pick);
          if (at) {
            await send(page, { type: 'order_move', organism: p.id, x: at.x, y: at.y });
            await waitFor('getting within reach', async () => {
              const [me, it] = await page.evaluate((id) => [window.evo.model.state.organisms.find((o) => o.possessed), window.evo.model.state.nearby.find((o) => o.id === id)], pick);
              return !me || !it || Math.hypot(me.x - it.x, me.y - it.y) <= 6;
            }, 10000).catch(() => null);
          }
          continue;
        }
        await page.evaluate(() => { document.querySelector('#toasts')?.replaceChildren(); });
        await page.click('#btn-attack', { timeout: 3000 });
        const said = await waitFor('an answer', () => attackAnswer(page, pick), 8000);
        inspected = { pick, label, said };
      } catch (e) {
        // The wild animal can die or wander out of sight before the
        // inspector shows it (the hunt above may have caught it): pick again.
        const gone = await page.evaluate((id) => !window.evo.model.state.nearby.some((o) => o.id === id), pick);
        // Or it wandered beyond reach: the button says so and is disabled (the interface is right).
        const far = await page.evaluate(() => {
          const b = document.querySelector('#btn-attack');
          return b?.disabled && /too far/.test(b.textContent ?? '') ? b.textContent : null;
        });
        if (far) {
          step(`wild #${pick} wandered out of reach ("${far}"): retrying`);
          continue;
        }
        if (gone) {
          step(`wild #${pick} went out of sight during the inspector step: retrying`);
          continue;
        }
        if (await possessedNow(page)) throw e;
        step('possession ended during the inspector step: retrying');
      }
    }
    check(inspected, 'the inspector attack never went through');
    check(!/`|unknown variant|expected/.test(inspected.said), 'the answer should read as a sentence', inspected.said);
    step(`the inspector of wild #${inspected.pick} offers "${inspected.label}": "${inspected.said.trim()}"`);

    // Over the socket: our own animal, and a second player's newcomer.
    const p2 = { id: await ensurePossessed(page, 'before the socket checks') };
    const ownTarget = await page.evaluate((me) => window.evo.model.state.organisms.find((o) => o.id !== me)?.id, p2.id);
    if (ownTarget !== undefined) {
      const r = await send(page, { type: 'possess_attack', organism: p2.id, target: ownTarget });
      check(!r.ok && r.error?.code === 'own_target', 'attacking our own animal should be refused', r);
      step(`attacking our own #${ownTarget} is refused (${r.error.code}: ${r.error.message})`);
    }
    const bobContext = await browser.newContext({ viewport: { width: 1000, height: 700 } });
    const bob = await bobContext.newPage();
    bob.on('pageerror', (e) => errors.push(e.message));
    await register(bob, base, `newcomer_${Date.now().toString(36)}`);
    const bobs = await bob.evaluate(() => window.evo.model.state.organisms.map((o) => o.id));
    let r = await send(page, { type: 'possess_attack', organism: p2.id, target: bobs[0] });
    if (['not_found', 'not_possessed'].includes(r.error?.code)) {
      // Our attacker died meanwhile.
      r = await send(page, { type: 'possess_attack', organism: await ensurePossessed(page, 'before the newcomer check'), target: bobs[0] });
    }
    check(!r.ok && ['newcomer_grace', 'target_out_of_sight'].includes(r.error?.code), "a newcomer's animal should be refused", r);
    step(`a second player's freshly founded #${bobs[0]} is refused (${r.error.code}: ${r.error.message})`);
    await bobContext.close();

    // Release: the bar goes, and attack mode with it.
    await attackMode(page);
    await page.click('#possess-release', { timeout: 5000 });
    await waitFor('the possession bar to hide', () => page.locator('#possess-bar').isHidden());
    await possessOne(page);
    check(await page.getAttribute('#possess-attack', 'aria-pressed') === 'false', 'attack mode should end with the possession');
    step('releasing hides the possession bar and ends attack mode (possessing again starts in walk mode)');
    check(errors.length === 0, 'page errors', errors);
    console.log('attack e2e: ok');
  } catch (e) {
    console.error(log.split('\n').slice(-30).join('\n'));
    throw e;
  } finally {
    await browser.close();
    server.kill();
  }
}

main().catch((e) => {
  console.error(`attack e2e failed: ${e.message}`);
  process.exit(1);
});
