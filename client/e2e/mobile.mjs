// End-to-end: the phone layout and touch input, in Chromium with Playwright
// device emulation (iPhone 13 and Pixel 7 viewports, touch, mobile UA), plus
// a desktop check that the wide layout is unchanged. Starts its own
// evo-server like smoke.mjs.
//
// Phone: the map fills the screen with a tab bar at the bottom; a tab tap
// opens the bottom sheet and a second tap closes it; a tap on an own
// organism selects it and shows the floating Possess button; the button and
// a long press possess; one-finger drag pans and a two-finger pinch zooms.
// Desktop: sidebar on the right, tabs on top, sprite cache stats, and a
// gallery of body-plan sprites for the screenshots. Pages open with
// `?tour=off` (e2e/onboarding.mjs tests the first-session tour).
// Worlds: on a server with two worlds the phone's login screen shows the
// world picker; the chosen world is the one played in, and the choice is
// remembered per user.
//
//   npm run build && node e2e/mobile.mjs     (env as for smoke.mjs)

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

/** A device descriptor for Chromium (the WebKit default of iPhones dropped). */
function device(name) {
  const { defaultBrowserType: _ignored, ...d } = devices[name];
  return d;
}

async function register(page, base, name, lineage) {
  await page.goto(`${base}/?tour=off`);
  // One world: no picker.
  await page.waitForTimeout(300);
  if (await page.isVisible('#world-box')) throw new Error('world picker shown on a single-world server');
  await page.fill('#login-name', name);
  await page.fill('#login-password', 'a long password');
  await page.click('#register-submit');
  await page.waitForSelector('#design-name', { timeout: 15000 });
  await page.fill('#design-name', lineage);
  await page.click('#design-submit');
  await page.waitForSelector('#world-canvas', { timeout: 15000 });
  await waitFor('own organisms', () => page.evaluate(() => (window.evo?.model.state?.organisms.length ?? 0) > 0));
  await waitFor('markers', () => page.evaluate(() => (window.evo.screen()?.view.spriteStats().markers ?? 0) > 0));
}

/** A screenshot, retried: a freshly loaded emulated-mobile page can refuse the first capture. */
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

const box = (page, sel) => page.locator(sel).boundingBox();

/**
 * Zoom in on one of our organisms with room around it (standing still if
 * possible) and return its screen position once the view's hit test agrees
 * (markers keep moving).
 */
async function findOwn(page, exclude = []) {
  for (let attempt = 0; attempt < 15; attempt++) {
    const pick = await page.evaluate((exclude) => {
      const v = window.evo.screen().view;
      v.setFollow(false);
      v.cam.zoom = 16;
      const own = window.evo.model.state.organisms.filter((o) => v.markers.has(o.id) && !exclude.includes(o.id));
      let best = null;
      for (const o of own) {
        const m = v.markers.get(o.id);
        let room = Infinity;
        for (const [id, n] of v.markers) if (id !== o.id) room = Math.min(room, Math.hypot(n.x - m.x, n.y - m.y));
        // The fast world moves walkers several tiles a second: prefer ones standing still.
        const still = ['rest', 'dormant', 'drink', 'idle', 'bask', 'sentry', 'guard'].includes(o.action) ? 4 : 0;
        const score = Math.min(room, 3) + (o.matured ? 3 : 0) + o.health / 100 + still;
        if (!best || score > best.score) best = { id: o.id, x: m.x, y: m.y, score };
      }
      if (best) v.centerOn(best.x, best.y);
      return best;
    }, exclude);
    if (!pick) throw new Error('no own marker');
    await page.waitForTimeout(150);
    const p = await page.evaluate((id) => {
      const v = window.evo.screen().view;
      const p = v.markerScreenPos(id);
      return p && v.hitTest(p.x, p.y, true) === id ? p : null;
    }, pick.id);
    if (p) return { id: pick.id, ...p };
  }
  throw new Error('could not find a tappable own organism');
}

/** Touch gestures through the DevTools protocol (Playwright has only tap). */
function touch(cdp, canvas) {
  const pt = (p, id) => ({ x: canvas.x + p.x, y: canvas.y + p.y, id, radiusX: 4, radiusY: 4, force: 1 });
  const send = (type, points) => cdp.send('Input.dispatchTouchEvent', { type, touchPoints: points });
  return {
    async longPress(p, ms = 900) {
      await send('touchStart', [pt(p, 1)]);
      await new Promise((r) => setTimeout(r, ms));
      await send('touchEnd', []);
    },
    async drag(from, to, steps = 8) {
      await send('touchStart', [pt(from, 1)]);
      for (let i = 1; i <= steps; i++) {
        const k = i / steps;
        await send('touchMove', [pt({ x: from.x + (to.x - from.x) * k, y: from.y + (to.y - from.y) * k }, 1)]);
      }
      await send('touchEnd', []);
    },
    async pinch(center, fromGap, toGap, steps = 8) {
      const pair = (gap) => [pt({ x: center.x - gap / 2, y: center.y }, 1), pt({ x: center.x + gap / 2, y: center.y }, 2)];
      await send('touchStart', pair(fromGap));
      for (let i = 1; i <= steps; i++) await send('touchMove', pair(fromGap + ((toGap - fromGap) * i) / steps));
      await send('touchEnd', []);
    },
  };
}

async function phoneChecks(browser, base, deviceName, tag, errors) {
  const context = await browser.newContext({ ...device(deviceName) });
  const page = await context.newPage();
  page.on('pageerror', (e) => errors.push(`${tag}: ${e.message}`));
  const vw = page.viewportSize();

  // Login screen fits the phone.
  await page.goto(`${base}/?tour=off`);
  await page.waitForSelector('#login-name');
  const form = await box(page, 'form.login');
  if (form.x < 0 || form.x + form.width > vw.width + 1) throw new Error(`${tag}: login form overflows ${JSON.stringify(form)}`);
  await shot(page, `${tag}-login.png`);

  await register(page, base, `m_${tag}_${Date.now().toString(36)}`, `${tag} hoppers`);
  await page.waitForFunction(() => document.querySelector('#conn-state')?.textContent === 'connected');

  // Layout: the map spans the width, the tab bar sits at the bottom, the sheet starts closed.
  const layout = await page.evaluate(() => {
    const r = (s) => document.querySelector(s).getBoundingClientRect();
    const panelsShown = [...document.querySelectorAll('.sidebar .panel')].filter((p) => getComputedStyle(p).display !== 'none').length;
    return { canvas: r('#world-canvas'), tabbar: r('#tabbar'), sidebar: r('#sidebar'), panelsShown, docW: document.documentElement.scrollWidth };
  });
  if (layout.canvas.width < vw.width - 1) throw new Error(`${tag}: map not full width ${JSON.stringify(layout.canvas)}`);
  if (Math.abs(layout.tabbar.bottom - vw.height) > 2) throw new Error(`${tag}: tab bar not at the bottom ${JSON.stringify(layout.tabbar)} vs ${vw.height}`);
  if (layout.sidebar.width < vw.width - 1 || layout.panelsShown !== 0) throw new Error(`${tag}: sheet not closed full-width ${JSON.stringify(layout)}`);
  if (layout.docW > vw.width + 1) throw new Error(`${tag}: page scrolls sideways (${layout.docW} > ${vw.width})`);
  step(`${tag}: ${vw.width}×${vw.height} phone layout: full-width map, bottom tab bar, sheet closed`);

  // Tab bar: a tap opens the sheet, a second tap on the same tab closes it.
  await page.tap('#tab-lineage');
  await waitFor('sheet open', () => page.evaluate(() => {
    const s = document.querySelector('#sidebar').getBoundingClientRect();
    const p = document.querySelector('#panel-lineage');
    return document.querySelector('.game').classList.contains('sheet-open') && getComputedStyle(p).display !== 'none' && s.height > window.innerHeight * 0.4;
  }), 5000);
  await page.waitForTimeout(300); // slide-up transition
  await shot(page, `${tag}-sheet-lineage.png`);
  await page.tap('#tab-lineage');
  await waitFor('sheet closed', () => page.evaluate(() => !document.querySelector('.game').classList.contains('sheet-open')), 5000);
  step(`${tag}: tab tap opens the bottom sheet, a second tap closes it`);

  // Seasons tab: the season under way, your record and the hall of fame fit
  // the phone; a finished season opens onto its standings.
  await page.tap('#tab-seasons');
  const fits = () => page.evaluate(() => {
    const p = document.querySelector('#panel-seasons');
    if (getComputedStyle(p).display === 'none' || !document.querySelector('#season-clock')) return null;
    const tabs = [...document.querySelectorAll('#tabbar .tab')].map((t) => t.getBoundingClientRect().right);
    return {
      finished: document.querySelectorAll('#season-history details').length,
      panelScrollW: p.scrollWidth, panelW: p.clientWidth, docW: document.documentElement.scrollWidth, tabsRight: Math.max(...tabs),
    };
  });
  const seasons = await waitFor('a finished season in the seasons tab', async () => {
    const f = await fits();
    return f && f.finished > 0 ? f : null;
  }, 20000);
  const overflow = (f) => f.docW > vw.width + 1 || f.panelScrollW > f.panelW + 1 || f.tabsRight > vw.width + 1;
  if (overflow(seasons)) throw new Error(`${tag}: seasons tab overflows ${JSON.stringify(seasons)} (viewport ${vw.width})`);
  await page.tap('#season-history details summary');
  await waitFor('season standings', () => page.evaluate(() => {
    // The list re-renders as new seasons end: open the newest again if a
    // re-render raced the tap.
    const d = document.querySelector('#season-history details');
    if (d && !document.querySelector('#season-history details[open]')) d.open = true;
    return document.querySelector('#season-history details[open] .standings table') !== null;
  }), 10000);
  const opened = await fits();
  if (overflow(opened)) throw new Error(`${tag}: season standings overflow ${JSON.stringify(opened)} (viewport ${vw.width})`);
  await page.waitForTimeout(300);
  await shot(page, `${tag}-sheet-seasons.png`);
  await page.tap('#tab-seasons');
  await waitFor('sheet closed', () => page.evaluate(() => !document.querySelector('.game').classList.contains('sheet-open')), 5000);
  step(`${tag}: seasons tab fits (${seasons.finished} finished seasons, standings open without sideways scroll)`);

  // Tap to select an own organism: the floating Possess button appears.
  const canvas = await box(page, '#world-canvas');
  let sel = null;
  for (let attempt = 0; attempt < 8 && !sel; attempt++) {
    const o = await findOwn(page);
    await page.evaluate(() => window.evo.model.select(null));
    await page.touchscreen.tap(canvas.x + o.x, canvas.y + o.y);
    const got = await waitFor('selection', () => page.evaluate(() => window.evo.model.selected), 3000).catch(() => null);
    if (got !== null && (await page.evaluate((id) => window.evo.model.state.organisms.some((x) => x.id === id), got))) sel = got;
  }
  if (sel === null) throw new Error(`${tag}: tap did not select an own organism`);
  await waitFor('fab', () => page.evaluate((id) => {
    const f = document.querySelector('#fab-possess');
    return !f.hidden && getComputedStyle(f).display !== 'none' && f.textContent.includes(`#${id}`);
  }, sel), 5000);
  step(`${tag}: tap selected own organism #${sel}; floating Possess button shown`);
  await shot(page, `${tag}-selected.png`);

  // The floating button possesses (and releases).
  const fabBox = await box(page, '#fab-possess'); // null if the organism just died
  if (fabBox) await page.touchscreen.tap(fabBox.x + fabBox.width / 2, fabBox.y + fabBox.height / 2);
  const possessed = !fabBox ? null : await waitFor('possessed via button', () => page.evaluate(() => window.evo.model.state.organisms.find((o) => o.possessed)?.id ?? null), 12000).catch(() => null);
  if (possessed !== null) {
    step(`${tag}: Possess button possessed #${possessed}`);
    await page.evaluate((id) => window.evo.socket.send({ type: 'release', organism: id }), possessed);
    await waitFor('released', () => page.evaluate(() => !window.evo.model.state.organisms.some((o) => o.possessed)), 12000);
  } else {
    // The organism may have died in between; the long press below covers possession.
    console.log(`  … ${tag}: #${sel} was not possessed via the button (likely died); continuing`);
  }

  // Long press on an own organism possesses it.
  // Record possess commands and replies, to explain a failed long press.
  await page.evaluate(() => {
    window.__cmds = [];
    const sock = window.evo.socket;
    const orig = sock.send.bind(sock);
    sock.send = async (m) => {
      const r = await orig(m);
      window.__cmds.push(`${m.type} #${m.organism ?? ''}: ${r.ok ? 'ok' : r.error?.code}`);
      return r;
    };
  });
  // And what each long press hit, and the pointer events the canvas saw (in
  // the fast world a marker can walk out from under the finger in between).
  await page.evaluate(() => {
    const v = window.evo.screen().view;
    const orig = v.onLongPress;
    v.onLongPress = (c) => {
      window.__cmds.push(`long press hit ${c.organism ?? 'nothing'}`);
      return orig(c);
    };
    for (const t of ['pointerdown', 'pointerup', 'pointercancel', 'contextmenu']) {
      v.app.canvas.addEventListener(t, (e) => {
        const r = v.app.canvas.getBoundingClientRect();
        const x = e.clientX - r.left, y = e.clientY - r.top;
        let near = '';
        if (t === 'pointerdown') {
          let best = Infinity, id = null;
          for (const [k] of v.markers) { const p = v.markerScreenPos(k); const d = Math.hypot(p.x - x, p.y - y); if (d < best) { best = d; id = k; } }
          near = ` at ${x.toFixed(0)},${y.toFixed(0)}, nearest #${id} ${best.toFixed(0)} px away`;
        }
        window.__cmds.push(`${t}${near}`);
      });
    }
  });
  const cdp = await context.newCDPSession(page);
  const t = touch(cdp, canvas);
  let lp = null;
  for (let attempt = 0; attempt < 6 && lp === null; attempt++) {
    const o = await findOwn(page);
    await t.longPress(o);
    // Generous: on a loaded machine the possess reply can take seconds.
    lp = await waitFor('possessed via long press', () => page.evaluate(() => window.evo.model.state.organisms.find((x) => x.possessed)?.id ?? null), 12000).catch(() => null);
    if (lp === null) {
      const why = await page.evaluate(() => window.__cmds.splice(0).join(', ') || 'no command sent');
      console.log(`  … ${tag}: long press attempt ${attempt + 1} on #${o.id} at ${o.x.toFixed(0)},${o.y.toFixed(0)} did not possess (${why}); retrying`);
    }
  }
  if (lp === null) {
    const toasts = await page.evaluate(() => window.evo.model.toasts.map((t) => t.text).join(' | '));
    throw new Error(`${tag}: long press did not possess (toasts: ${toasts})`);
  }
  step(`${tag}: long press possessed #${lp}`);
  await page.waitForTimeout(400);
  await shot(page, `${tag}-possessed.png`);
  await page.evaluate((id) => window.evo.socket.send({ type: 'release', organism: id }), lp);

  // One-finger drag pans (and stops following); a pinch zooms.
  // From the middle of the map, so the camera clamp at the edges can't eat the pan.
  await page.evaluate(() => { const v = window.evo.screen().view; v.setFollow(false); v.cam.zoom = 8; v.centerOn(32, 32); });
  await page.waitForTimeout(100);
  const before = await page.evaluate(() => ({ ...window.evo.screen().view.cam }));
  const mid = { x: canvas.width / 2, y: canvas.height / 2 };
  await t.drag({ x: mid.x + 60, y: mid.y }, { x: mid.x - 60, y: mid.y - 40 });
  const panned = await page.evaluate(() => ({ ...window.evo.screen().view.cam }));
  if (Math.abs(panned.x - before.x) < 5 || Math.abs(panned.y - before.y) < 2) throw new Error(`${tag}: drag did not pan ${JSON.stringify({ before, panned })}`);
  // A drag also stops following the view window.
  await page.evaluate(() => window.evo.screen().view.setFollow(true));
  await t.drag({ x: mid.x - 40, y: mid.y }, { x: mid.x + 40, y: mid.y });
  if (await page.evaluate(() => window.evo.screen().view.follow)) throw new Error(`${tag}: drag did not stop following`);
  await page.evaluate(() => window.evo.screen().view.centerOn(32, 32));
  await t.pinch(mid, 60, 220);
  const zoomed = await page.evaluate(() => window.evo.screen().view.cam.zoom);
  if (zoomed < before.zoom * 2) throw new Error(`${tag}: pinch did not zoom (${before.zoom} → ${zoomed})`);
  await t.pinch(mid, 220, 110);
  const unzoomed = await page.evaluate(() => window.evo.screen().view.cam.zoom);
  if (!(unzoomed < zoomed)) throw new Error(`${tag}: pinch-in did not zoom out (${zoomed} → ${unzoomed})`);
  step(`${tag}: drag panned ${(panned.x - before.x).toFixed(1)} tiles; pinch zoomed ${before.zoom} → ${zoomed.toFixed(1)} → ${unzoomed.toFixed(1)}`);

  // Selecting with the sheet open shows the inspector; the legend toggles.
  await page.evaluate(() => { const v = window.evo.screen().view; v.cam.zoom = 14; });
  await page.tap('#tab-organism');
  // (Any living one of ours: the long-pressed one may have died meanwhile.)
  await waitFor('inspector', () => page.evaluate(() => {
    const o = window.evo.model.state.organisms.find((x) => x.matured) ?? window.evo.model.state.organisms[0];
    if (window.evo.model.selected !== o.id) window.evo.model.select(o.id);
    return document.querySelector('#panel-organism').textContent.includes('Body plan');
  }), 8000);
  await page.waitForTimeout(300);
  await shot(page, `${tag}-sheet-organism.png`);
  await page.tap('#tab-organism');
  await page.tap('#legend-toggle');
  await waitFor('legend', () => page.evaluate(() => getComputedStyle(document.querySelector('.legends')).display !== 'none'), 3000);
  await shot(page, `${tag}-legend.png`);
  step(`${tag}: inspector in the sheet; legend toggle`);

  // Group tools in the scrolling toolbar: split, then claim a territory for
  // the new group with two taps on the map.
  await page.tap('#legend-toggle');
  await page.tap('#split-group');
  await waitFor('two groups', () => page.evaluate(() => window.evo.model.state.groups?.length === 2
    && document.querySelector('#group-select').value !== 'all'), 8000);
  await page.tap('#territory');
  const c = await box(page, '#world-canvas');
  const zoom = await page.evaluate(() => window.evo.screen().view.cam.zoom);
  await page.touchscreen.tap(c.x + c.width / 2, c.y + c.height / 2);
  await waitFor('territory draft', () => page.evaluate(() => window.evo.screen().view.overlayInfo().draft !== null), 3000);
  await page.touchscreen.tap(c.x + c.width / 2, c.y + c.height / 2 + 5 * zoom);
  const held = await waitFor('territory held', () => page.evaluate(() => window.evo.model.state.groups.find((g) => g.territory)?.territory), 8000);
  const tools = await page.evaluate(() => document.documentElement.scrollWidth);
  if (tools > vw.width + 1) throw new Error(`${tag}: the toolbar made the page scroll sideways (${tools})`);
  await shot(page, `${tag}-territory.png`);
  step(`${tag}: split from the toolbar; territory of radius ${held.radius.toFixed(1)} drawn with two taps`);
  await context.close();
}

async function desktopChecks(browser, base, errors) {
  const page = await browser.newPage({ viewport: { width: 1280, height: 800 }, deviceScaleFactor: 1 });
  page.on('pageerror', (e) => errors.push(`desktop: ${e.message}`));
  await register(page, base, `m_desk_${Date.now().toString(36)}`, 'Desk striders');
  const layout = await page.evaluate(() => {
    const r = (s) => document.querySelector(s).getBoundingClientRect();
    return {
      canvas: r('#world-canvas'), sidebar: r('#sidebar'), tabbar: r('#tabbar'),
      narrowShown: [...document.querySelectorAll('.narrow-only')].some((e) => getComputedStyle(e).display !== 'none'),
      fab: getComputedStyle(document.querySelector('#fab-possess')).display,
    };
  });
  if (!(layout.sidebar.left >= layout.canvas.right - 1 && layout.tabbar.top < 100 && !layout.narrowShown && layout.fab === 'none')) {
    throw new Error(`desktop layout: ${JSON.stringify(layout)}`);
  }
  step('desktop: sidebar on the right, tabs on top, phone-only controls hidden');

  // Sprites: markers carry body-plan keys; textures are shared per class.
  const stats = await page.evaluate(() => {
    const v = window.evo.screen().view;
    const s = v.spriteStats();
    const keys = new Set([...v.markers.keys()].map((id) => v.markerInfo(id).key.split('|')[0]));
    return { ...s, classes: keys.size };
  });
  if (stats.textures !== stats.made || stats.classes < 1 || stats.textures > stats.markers * 3 + 12) throw new Error(`sprite stats: ${JSON.stringify(stats)}`);
  step(`sprites: ${stats.markers} markers, ${stats.classes} body-plan classes, ${stats.textures} cached textures`);
  await page.evaluate(() => { const v = window.evo.screen().view; v.setFollow(true); v.cam.zoom = 14; });
  await page.waitForTimeout(1500);
  await shot(page, 'desktop-world.png');

  // A gallery of body plans drawn with the game's own sprite code, over the map.
  const gallery = await page.evaluate(() => {
    const v = window.evo.screen().view;
    const sprites = v.sprites;
    const Graphics = v.highlight.constructor;
    const Container = v.markerLayer.constructor;
    const layer = new Container();
    const bg = new Graphics();
    layer.addChild(bg);
    const base = { size: 2, diet: 'herbivore', warm: true, locomotion: 'walk', cover: 'fur', coloring: 'drab' };
    const rows = [
      [{}, { size: 0 }, { size: 1 }, { size: 3 }, { size: 2, juvenile: true }],
      [{ diet: 'omnivore' }, { diet: 'carnivore' }, { diet: 'carnivore', size: 3, cover: 'skin' }, { warm: false, cover: 'scales' }, { warm: false, cover: 'armor', size: 3 }],
      [{ locomotion: 'swim', warm: false, cover: 'scales' }, { locomotion: 'swim', diet: 'carnivore', size: 3, cover: 'skin' }, { locomotion: 'glide', size: 1 }, { locomotion: 'fly', size: 0 }, { locomotion: 'fly', size: 1, diet: 'carnivore', cover: 'skin' }],
      [{ coloring: 'camo' }, { coloring: 'bright' }, { coloring: 'warning', warm: false, cover: 'skin', size: 0 }, { coloring: 'camo', warm: false, cover: 'scales', diet: 'carnivore', size: 3 }, { coloring: 'bright', locomotion: 'fly', size: 0 }],
    ];
    const hints = ['sleep', 'flee', 'hunt', 'mate', null];
    const tints = [0xffd33d, 0x9aa4ae, 0x5ab0ff, 0xff7eb6, 0x7ee787];
    const cell = 120;
    bg.rect(20, 20, cell * 5 + 20, cell * rows.length + 20).fill({ color: 0x0d1117, alpha: 0.88 });
    rows.forEach((row, ry) => row.forEach((over, cx) => {
      const { juvenile = false, ...look } = over;
      const spec = { look: { ...base, ...look }, juvenile };
      const parts = sprites.marker();
      parts.ring.texture = sprites.ownerRing(spec, cx === 0);
      parts.ring.tint = tints[(cx + ry) % tints.length];
      parts.body.texture = sprites.body(spec);
      const h = hints[(cx + ry * 2) % hints.length];
      if (h) {
        parts.icon.visible = true;
        parts.icon.texture = sprites.hint(h);
        parts.icon.position.set(10, -22);
      }
      parts.root.position.set(30 + cell / 2 + cx * cell, 30 + cell / 2 + ry * cell);
      parts.root.scale.set(3);
      layer.addChild(parts.root);
    }));
    v.app.stage.addChild(layer);
    window.__gallery = layer;
    return v.spriteStats();
  });
  await page.waitForTimeout(200);
  await shot(page, 'desktop-sprite-gallery.png');
  await page.evaluate(() => window.__gallery.destroy({ children: true }));
  step(`sprite gallery: ${gallery.textures} textures cached`);
  await page.close();
}

/** The world picker on a phone, against a server hosting `main` and `isle`. */
async function worldChecks(browser, base, errors) {
  const context = await browser.newContext({ ...device('Pixel 7') });
  const page = await context.newPage();
  page.on('pageerror', (e) => errors.push(e.message));
  const online = async () => Object.fromEntries((await (await fetch(`${base}/api/worlds`)).json()).worlds.map((w) => [w.id, w.players_online]));
  await page.goto(`${base}/?tour=off`);
  await page.waitForSelector('#world-select', { state: 'visible', timeout: 10000 });
  const options = await page.$$eval('#world-select option', (os) => os.map((o) => [o.value, o.textContent]));
  if (JSON.stringify(options) !== JSON.stringify([['main', 'Mainland'], ['isle', 'Isle']])) throw new Error(`world options ${JSON.stringify(options)}`);
  const sel = await box(page, '#world-select');
  if (!sel || sel.x < 0 || sel.x + sel.width > 412 + 1) throw new Error(`world picker off screen: ${JSON.stringify(sel)}`);
  // Ann picks the isle and plays there.
  const ann = `ann_${Date.now().toString(36)}`;
  await page.fill('#login-name', ann);
  await page.fill('#login-password', 'a long password');
  await page.selectOption('#world-select', 'isle');
  if (!(await page.textContent('#world-info')).includes('48×48')) throw new Error(`world info: ${await page.textContent('#world-info')}`);
  await shot(page, 'pixel7-world-picker.png');
  await page.click('#register-submit');
  await page.waitForSelector('#design-name', { timeout: 15000 });
  await page.fill('#design-name', 'Isle walkers');
  await page.click('#design-submit');
  await page.waitForSelector('#world-canvas', { timeout: 15000 });
  await waitFor('own organisms', () => page.evaluate(() => (window.evo?.model.state?.organisms.length ?? 0) > 0));
  await waitFor('Ann online on the isle', async () => {
    const o = await online();
    return o.isle === 1 && o.main === 0;
  });
  await waitFor('world name in the top bar', async () => (await page.textContent('#world-name')) === 'Isle');
  // Founded on the isle, not the mainland (no wild lineages on either).
  const lineages = async (w) => (await (await fetch(`${base}/api/world?world=${w}`)).json()).lineages;
  if ((await lineages('isle')) !== 1 || (await lineages('main')) !== 0) throw new Error('the lineage was founded in the wrong world');
  step('world picker: Ann chose the isle on a phone and plays there');
  await page.evaluate(() => document.querySelector('#logout').click());
  await page.waitForSelector('#login-name');
  // Bob picks the mainland.
  const bob = `bob_${Date.now().toString(36)}`;
  await page.fill('#login-name', bob);
  await page.fill('#login-password', 'a long password');
  await page.waitForSelector('#world-select', { state: 'visible' });
  await page.selectOption('#world-select', 'main');
  await page.click('#register-submit');
  await page.waitForSelector('#design-name', { timeout: 15000 });
  await waitFor('Bob online on the mainland', async () => (await online()).main === 1);
  await page.click('text=Log out');
  await page.waitForSelector('#login-name');
  // Each name brings back its own world.
  const pickFor = async (name) => {
    await page.fill('#login-name', name);
    await page.dispatchEvent('#login-name', 'change');
    return page.inputValue('#world-select');
  };
  await page.waitForSelector('#world-select', { state: 'visible' });
  if ((await pickFor(ann)) !== 'isle') throw new Error('Ann\'s world was not remembered');
  if ((await pickFor(bob)) !== 'main') throw new Error('Bob\'s world was not remembered');
  step('world picker: the world is remembered per user');
  await context.close();
}

/**
 * Run `fn` against a fresh server: each device gets a young world, so its
 * lineage is not yet thinned out (the fast world runs a day a second, and a
 * long earlier section would leave the next one little to tap).
 */
async function withServer(seed, fn, extra = []) {
  const port = await freePort();
  const base = `http://127.0.0.1:${port}`;
  const server = spawn(bin, ['--fast', '--memory', '--size', '64', '--seed', String(seed), '--days-per-year', '480', '--bind', `127.0.0.1:${port}`, '--static', dist, ...extra], {
    stdio: ['ignore', 'pipe', 'pipe'],
  });
  let log = '';
  server.stdout.on('data', (d) => (log += d));
  server.stderr.on('data', (d) => (log += d));
  try {
    await waitFor('server', async () => (await fetch(`${base}/api/world`)).ok);
    step(`server up on ${base}`);
    await fn(base);
  } catch (e) {
    console.error(log.split('\n').slice(-20).join('\n'));
    throw e;
  } finally {
    server.kill('SIGINT');
  }
}

async function main() {
  if (!existsSync(bin)) throw new Error(`no server binary at ${bin}; run cargo build --release -p evo-server`);
  if (!existsSync(path.join(dist, 'index.html'))) throw new Error('no client/dist; run npm run build');
  mkdirSync(shots, { recursive: true });
  const browser = await chromium.launch({ headless: !process.env.HEADED });
  const errors = [];
  try {
    // The phone sections tap and hold their own organisms for minutes: no
    // wild predators, so the lineage outlives the test on a slow machine.
    // Two-day ranked seasons, so the hall of fame fills within seconds.
    await withServer(9, (base) => phoneChecks(browser, base, 'iPhone 13', 'iphone13', errors), ['--no-wild', '--season-days', '2']);
    await withServer(10, (base) => phoneChecks(browser, base, 'Pixel 7', 'pixel7', errors), ['--no-wild', '--season-days', '2']);
    await withServer(11, (base) => desktopChecks(browser, base, errors));
    await withServer(12, (base) => worldChecks(browser, base, errors), ['--no-wild', '--world', 'main,name=Mainland', '--world', 'isle,seed=5,size=48,name=Isle']);
    if (errors.length) throw new Error(`page errors: ${errors.join('; ')}`);
    console.log(`mobile test passed; screenshots in ${shots}`);
  } catch (e) {
    for (const c of browser.contexts()) for (const [k, p] of c.pages().entries()) await p.screenshot({ path: path.join(shots, `mobile-failure-${k}.png`) }).catch(() => {});
    throw e;
  } finally {
    await browser.close();
  }
}

main().catch((e) => {
  console.error(e);
  process.exit(1);
});
