// End-to-end check of the vision cones: start evo-server (fast, in memory)
// serving the built client, register, found a lineage, and in headless
// Chromium check that
//   - states come over protocol 3 with every own organism's heading and
//     eyes, and no other organism's;
//   - clicking one of your organisms draws its cones on the map (the cone
//     layer with hatched blind spots and a dotted depth zone), and the
//     organism panel shows the view / depth / blind numbers, says whether
//     directional vision is on, and its "Show vision" toggle hides and
//     shows the cones (remembered);
//   - possessed and walked in different directions, the cones turn with the
//     server's heading and follow the marker, and the sprite faces its way.
// Screenshot: e2e/out/vision-cones.png.
//
//   npm run build && node e2e/vision.mjs
//
// Env: EVO_SERVER_BIN (default ../target/release/evo-server), SCREENSHOT_DIR
// (default e2e/out), HEADED=1 to watch.

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

async function waitFor(what, fn, timeoutMs = 45000) {
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

const started = Date.now();
function step(msg) {
  console.log(`  ✓ ${msg} [${((Date.now() - started) / 1000).toFixed(0)} s]`);
}

/** Smallest difference of two angles in degrees, 0–180. */
const angleGap = (a, b) => {
  const d = (((a - b) % 360) + 360) % 360;
  return Math.min(d, 360 - d);
};

async function main() {
  if (!existsSync(bin)) throw new Error(`no server binary at ${bin}; run cargo build --release -p evo-server`);
  if (!existsSync(path.join(dist, 'index.html'))) throw new Error('no client/dist; run npm run build');
  mkdirSync(shots, { recursive: true });
  const port = await freePort();
  const base = `http://127.0.0.1:${port}`;
  const server = spawn(bin, ['--fast', '--memory', '--size', '64', '--seed', '11', '--bind', `127.0.0.1:${port}`, '--static', dist], {
    stdio: ['ignore', 'pipe', 'pipe'],
  });
  let serverLog = '';
  server.stdout.on('data', (d) => (serverLog += d));
  server.stderr.on('data', (d) => (serverLog += d));
  const browser = await chromium.launch({ headless: !process.env.HEADED });
  let page;
  try {
    await waitFor('server', async () => (await fetch(`${base}/api/world`)).ok);
    step(`server up on ${base}`);
    page = await browser.newPage({ viewport: { width: 1280, height: 800 }, deviceScaleFactor: 1 });
    const pageErrors = [];
    page.on('pageerror', (e) => pageErrors.push(e.message));
    await page.goto(`${base}/?tour=off`);
    await page.fill('#login-name', `vis_${Date.now().toString(36)}`);
    await page.fill('#login-password', 'a long password');
    await page.click('#register-submit');
    await page.waitForSelector('#design-name', { timeout: 45000 });
    await page.fill('#design-name', 'Lookouts');
    await page.$eval('input[data-gene="large_eyes"]', (el) => { el.value = '0.5'; el.dispatchEvent(new Event('input')); });
    await page.click('#design-submit');
    await page.waitForSelector('#world-canvas', { timeout: 45000 });
    await waitFor('own organisms drawn', () => page.evaluate(() => {
      const v = window.evo.screen()?.view;
      return (window.evo.model.state?.organisms.length ?? 0) > 0 && v && v.markers.size > 0;
    }));
    step('registered and founded; world view up');

    // Protocol 3: heading and eyes on every own organism, none on others.
    const wire = await waitFor('a state with neighbors', () => page.evaluate(() => {
      const s = window.evo.model.state;
      if (s.nearby.length === 0) return null;
      return {
        proto: window.evo.model.proto,
        own: s.organisms.length,
        withVision: s.organisms.filter((o) => Number.isInteger(o.heading) && o.heading >= 0 && o.heading < 32 && o.vision?.eyes >= 1).length,
        leaks: s.nearby.filter((o) => 'heading' in o || 'vision' in o).length,
        nearby: s.nearby.length,
      };
    }), 60000);
    if (wire.proto !== 3) throw new Error(`the connection speaks protocol ${wire.proto}, not 3`);
    if (wire.withVision !== wire.own) throw new Error(`own organisms without heading or vision: ${JSON.stringify(wire)}`);
    if (wire.leaks) throw new Error(`other organisms carry heading or vision: ${JSON.stringify(wire)}`);
    step(`protocol 3: all ${wire.own} own organisms carry heading and eyes; none of ${wire.nearby} neighbors do`);

    // Click one of ours (zoomed in, the marker with the most room around it).
    const canvasBox = await page.locator('#world-canvas').boundingBox();
    const alive = (id) => page.evaluate((id) => window.evo.model.state.organisms.some((o) => o.id === id), id);
    let id = null;
    for (let attempt = 0; attempt < 10 && id === null; attempt++) {
      const pick = await page.evaluate(() => {
        const v = window.evo.screen().view;
        v.setFollow(false);
        v.cam.zoom = 14;
        let best = null;
        for (const o of window.evo.model.state.organisms) {
          const m = v.markers.get(o.id);
          if (!m) continue;
          let room = Infinity;
          for (const [k, n] of v.markers) if (k !== o.id) room = Math.min(room, Math.hypot(n.x - m.x, n.y - m.y));
          const score = Math.min(room, 3) + (o.matured ? 3 : 0) + o.health / 100;
          if (!best || score > best.score) best = { id: o.id, x: m.x, y: m.y, score };
        }
        if (best) v.centerOn(best.x, best.y);
        return best;
      });
      if (!pick) throw new Error('no own marker to click');
      await page.waitForTimeout(150);
      const p = await page.evaluate((id) => {
        const v = window.evo.screen().view;
        const p = v.markerScreenPos(id);
        return p && v.hitTest(p.x, p.y) === id ? p : null;
      }, pick.id);
      if (!p) continue;
      await page.evaluate(() => window.evo.model.select(null));
      await page.mouse.click(canvasBox.x + p.x, canvasBox.y + p.y);
      const got = await waitFor('selection', () => page.evaluate(() => window.evo.model.selected), 6000).catch(() => null);
      const own = got !== null && (await page.evaluate((id) => window.evo.model.state.organisms.some((o) => o.id === id), got));
      if (own) id = got;
    }
    if (id === null) throw new Error('could not select an own organism by clicking it');
    await page.click('#tab-organism');
    const cones = await waitFor('vision cones on the map', () => page.evaluate((id) => {
      const i = window.evo.screen().view.visionInfo();
      return i && i.organism === id ? i : null;
    }, id), 15000);
    if (cones.eyes !== 2 || cones.covered < 1 || cones.blind < 1 || cones.overlap < 1) throw new Error(`cone layout: ${JSON.stringify(cones)}`);
    if (cones.hatch < 5 || cones.dots < 5) throw new Error(`cones not patterned: ${JSON.stringify(cones)}`);
    // On the marker as it moves (read right after a frame; a state landing
    // in between can move the marker a few pixels).
    const on = await page.evaluate(async (id) => {
      const v = window.evo.screen().view;
      await new Promise((r) => requestAnimationFrame(() => r()));
      const at = v.markerScreenPos(id);
      const i = v.visionInfo();
      return { at, cones: i && { x: i.x, y: i.y } };
    }, id);
    if (!on.cones || Math.hypot(on.at.x - on.cones.x, on.at.y - on.cones.y) > 12) throw new Error(`cones not on the marker: ${JSON.stringify(on)}`);
    step(`#${id} selected: cones drawn (r ${cones.radius} px, ${cones.covered} seen / ${cones.blind} blind / ${cones.overlap} depth arcs, ${cones.hatch} hatch strokes, ${cones.dots} dots)`);

    // The panel: numbers from the server's descriptor, the mechanic's state.
    const panel = await waitFor('vision numbers in the panel', () => page.evaluate((id) => {
      const el = document.querySelector('#inspector-vision');
      const v = window.evo.model.state.organisms.find((o) => o.id === id)?.vision;
      if (!el || !v || !el.textContent.includes('View')) return null;
      return { text: el.textContent, v, directional: document.querySelector('#vision-mechanic')?.dataset.directional };
    }, id), 10000);
    for (const want of [`${panel.v.coverage}%`, `${panel.v.binocular}°`, `${panel.v.blind}°`, 'Depth', 'Blind behind', 'Show vision']) {
      if (!panel.text.includes(want)) throw new Error(`panel lacks "${want}": ${panel.text}`);
    }
    // Directional vision is on by default (docs/morphology.md decision 17).
    if (panel.directional !== 'true' || !panel.text.includes('Directional vision is on')) throw new Error(`mechanic label: ${panel.directional} ${panel.text}`);
    step(`panel: view ${panel.v.coverage}%, depth ${panel.v.binocular}°, blind ${panel.v.blind}°; labelled "directional vision is on"`);

    // The toggle hides and shows the cones, and is remembered.
    await page.click('#vision-toggle');
    await waitFor('cones hidden', () => page.evaluate(() => window.evo.screen().view.visionInfo() === null), 5000);
    if ((await page.evaluate(() => localStorage.getItem('evo.showVision'))) !== '0') throw new Error('the toggle was not remembered');
    await page.click('#vision-toggle');
    await waitFor('cones shown again', () => page.evaluate(() => window.evo.screen().view.visionInfo() !== null), 5000);
    step('"Show vision" hides and shows the cones (remembered)');

    // Possess it and walk it in different directions: the cones turn with
    // the server heading and the sprite faces its way. The world runs a day
    // a second, so a short walk takes a fraction of a second: every frame's
    // cones are sampled in the page and checked afterwards.
    await page.evaluate(() => {
      window.__vs = [];
      const loop = () => {
        const v = window.evo.screen().view;
        const i = v.visionInfo();
        if (i) {
          const o = window.evo.model.state.organisms.find((o) => o.id === i.organism);
          window.__vs.push({ id: i.organism, h: i.heading, d: i.drawn, f: v.markerInfo(i.organism)?.facing, sh: o && o.heading * 11.25, t: performance.now() });
          if (window.__vs.length > 5000) window.__vs.splice(0, 1000);
        }
        requestAnimationFrame(loop);
      };
      requestAnimationFrame(loop);
    });
    const r = await page.evaluate((id) => window.evo.socket.send({ type: 'possess', organism: id }), id);
    if (!r.ok) throw new Error(`possess: ${JSON.stringify(r.error)}`);
    const seen = [];
    const tries = [];
    const spread = () => seen.some((a) => seen.some((b) => angleGap(a.heading, b.heading) >= 90));
    for (let attempt = 0; attempt < 12 && !spread(); attempt++) {
      if (!(await alive(id))) {
        // If it died, carry on with another of ours (selected, so its cones show).
        tries.push(`#${id} died`);
        seen.length = 0;
        id = await page.evaluate(() => {
          const v = window.evo.screen().view;
          const o = window.evo.model.state.organisms.filter((o) => v.markers.has(o.id)).sort((a, b) => Number(b.matured) - Number(a.matured) || b.health - a.health)[0];
          if (o) window.evo.model.select(o.id);
          return o?.id ?? null;
        });
        if (id === null) throw new Error('every organism died');
        await page.evaluate((id) => window.evo.socket.send({ type: 'possess', organism: id }), id);
      }
      // A dry spot a few tiles off, as far round from where it faces now as
      // the land allows (the sea or the map edge rule some directions out).
      const target = await page.evaluate((id) => {
        const v = window.evo.screen().view;
        const o = window.evo.model.state.organisms.find((x) => x.id === id);
        v.centerOn(o.x, o.y);
        const t = v.terrain;
        const land = (x, y) => {
          const i = Math.floor(y) * t.width + Math.floor(x);
          return t.biome[i] >= 3 && t.biome[i] <= 6 && t.water[i] === 0;
        };
        const facing = o.heading * 11.25;
        let best = null;
        for (let k = 0; k < 16; k++) {
          const dir = k * 22.5;
          const [dx, dy] = [Math.cos((dir * Math.PI) / 180), Math.sin((dir * Math.PI) / 180)];
          for (const d of [5, 4, 3]) {
            const x = o.x + dx * d, y = o.y + dy * d;
            if (x < 1 || y < 1 || x > 62 || y > 62 || !land(x, y) || !land(o.x + dx * d / 2, o.y + dy * d / 2)) continue;
            const turn = Math.min(Math.abs(dir - facing), 360 - Math.abs(dir - facing));
            if (!best || turn > best.turn) best = { x, y, dir, turn };
            break;
          }
        }
        return best && { ...best, t0: performance.now() };
      }, id);
      if (!target) {
        tries.push('no dry spot');
        await page.waitForTimeout(500);
        continue;
      }
      const order = await page.evaluate(({ id, t }) => window.evo.socket.send({ type: 'order_move', organism: id, x: t.x, y: t.y }), { id, t: target });
      if (!order.ok) {
        // Between connections (the socket reconnects by itself): wait for it.
        // Released (e.g. the possession lapsed): take it back.
        tries.push(`${target.dir}: ${order.error?.code}`);
        await page.waitForFunction(() => document.querySelector('#conn-state')?.textContent === 'connected', null, { timeout: 30000 });
        await page.evaluate((id) => window.evo.socket.send({ type: 'possess', organism: id }), id);
        continue;
      }
      // Cones facing the way it walks, the drawn ones (eased) close behind.
      const hit = await waitFor(`cones at ${target.dir}°`, () => page.evaluate(({ id, dir, t0 }) => {
        const gap = (a, b) => Math.min(Math.abs(a - b) % 360, 360 - (Math.abs(a - b) % 360));
        return window.__vs.find((s) => s.t > t0 && s.id === id && gap(s.h, dir) <= 40 && gap(s.d, s.h) < 15) ?? null;
      }, { id, dir: target.dir, t0: target.t0 }), 8000).catch(() => null);
      if (hit) seen.push({ dir: target.dir, heading: hit.h, drawn: hit.d, facing: hit.f, stateHeading: hit.sh });
      else tries.push(`${target.dir}: last heading ${await page.evaluate((id) => window.evo.model.state.organisms.find((o) => o.id === id)?.heading, id)}`);
    }
    if (!spread()) throw new Error(`the cones did not turn: ${JSON.stringify(seen)}; ${tries.join(', ')}`);
    // The sprite faces the way of the heading it was last set from (checked
    // where that is clearly left or right).
    const facings = await page.evaluate((id) => window.__vs.filter((s) => s.id === id && s.sh !== undefined && Math.abs(Math.cos((s.sh * Math.PI) / 180)) > 0.5)
      .map((s) => ({ f: s.f, want: Math.cos((s.sh * Math.PI) / 180) > 0 ? 1 : -1 })), id);
    const wrong = facings.filter((s) => s.f !== s.want);
    if (facings.length === 0 || wrong.length) throw new Error(`sprite facing: ${wrong.length} of ${facings.length} frames face away from the heading`);
    step(`walked: cones turned to ${seen.map((s) => `${s.heading}°`).join(', ')}; the sprite faced its heading in ${facings.length} frames`);

    // Screenshot, zoomed in on the cones.
    await page.evaluate((id) => {
      const v = window.evo.screen().view;
      v.cam.zoom = 18;
      const m = v.markers.get(id);
      v.centerOn(m.x, m.y);
    }, id);
    await page.waitForTimeout(400);
    await page.screenshot({ path: path.join(shots, 'vision-cones.png') });
    step(`screenshot ${path.join(shots, 'vision-cones.png')}`);

    if (pageErrors.length) throw new Error(`page errors: ${pageErrors.join('; ')}`);
    const decodeErrors = await page.evaluate(() => window.evo?.socket.decodeErrors ?? 0);
    if (decodeErrors > 0) throw new Error(`${decodeErrors} delta states did not decode`);
    console.log('vision e2e passed');
  } catch (e) {
    console.error(serverLog.split('\n').slice(-20).join('\n'));
    if (page) await page.screenshot({ path: path.join(shots, 'failure-vision.png') }).catch(() => {});
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
