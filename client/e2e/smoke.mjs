// End-to-end smoke test: start evo-server (fast, in memory) serving the built
// client, then in headless Chromium register, found a lineage, open the
// world view, check the tile map and organisms are drawn, possess one and
// walk it, open the genetics dashboard and the seasonal leaderboard, and
// run a directed mating between two players of the same body plan: preview
// its compatibility, request, accept, escort until the two meet, and check
// it completes and conceives.
//
//   npm run build && npm run e2e
//
// Env: EVO_SERVER_BIN (default ../target/release/evo-server; build it with
// `cargo build --release -p evo-server`), SCREENSHOT_DIR (default e2e/out),
// HEADED=1 to watch. Uses the `playwright` package from NODE_PATH or
// node_modules (it is not a dependency: browsers come pre-installed).

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

async function main() {
  if (!existsSync(bin)) throw new Error(`no server binary at ${bin}; run cargo build --release -p evo-server`);
  if (!existsSync(path.join(dist, 'index.html'))) throw new Error('no client/dist; run npm run build');
  mkdirSync(shots, { recursive: true });
  const port = await freePort();
  const base = `http://127.0.0.1:${port}`;
  // 120-day seasons (two minutes in fast mode): a season change expires open
  // mating offers, which would race the mating step on a slow machine.
  const server = spawn(bin, ['--fast', '--memory', '--size', '64', '--seed', '7', '--days-per-year', '480', '--bind', `127.0.0.1:${port}`, '--static', dist], {
    stdio: ['ignore', 'pipe', 'pipe'],
  });
  let serverLog = '';
  server.stdout.on('data', (d) => (serverLog += d));
  server.stderr.on('data', (d) => (serverLog += d));
  const browser = await chromium.launch({ headless: !process.env.HEADED });
  const pages = [];
  try {
    await waitFor('server', async () => (await fetch(`${base}/api/world`)).ok);
    step(`server up on ${base}`);
    const page = await browser.newPage({ viewport: { width: 1280, height: 800 }, deviceScaleFactor: 1 });
    const pageErrors = [];
    pages.push(page);
    page.on('pageerror', (e) => pageErrors.push(e.message));
    await page.goto(base);

    // Register.
    const name = `e2e_${Date.now().toString(36)}`;
    await page.fill('#login-name', name);
    await page.fill('#login-password', 'a long password');
    await page.click('#register-submit');
    await page.waitForSelector('#design-name', { timeout: 15000 });
    step('registered; design editor shown');

    // Design: the live cost follows the sliders and refuses an over-budget design.
    await page.fill('#design-name', 'Smoke striders');
    const setTrait = (gene, v) => page.$eval(`input[data-gene="${gene}"]`, (el, v) => {
      el.value = String(v);
      el.dispatchEvent(new Event('input'));
    }, v);
    const before = await page.textContent('#budget-total');
    await setTrait('body_size', 0.45);
    await setTrait('fur', 0.4);
    await setTrait('keen_nose', 0.3);
    const after = await page.textContent('#budget-total');
    if (before === after) throw new Error(`cost did not change (${before})`);
    // The starting cost plus each trait's rise above its default times its
    // price, from the server's catalog (prices are tuned in the sim).
    const catalog = await (await fetch(`${base}/api/genes`)).json();
    const gene = (n) => catalog.genes.find((g) => g.name === n);
    const set = { body_size: 0.45, fur: 0.4, keen_nose: 0.3 };
    const expected = Number.parseFloat(before) + Object.entries(set).reduce((sum, [n, v]) => sum + Math.max(0, v - gene(n).default) * gene(n).cost, 0);
    if (Math.abs(Number.parseFloat(after) - expected) > 0.051) throw new Error(`unexpected cost ${after} (expected ${expected.toFixed(1)})`);
    // Max out the priciest traits until the design is over budget.
    const pricey = [...catalog.genes].filter((g) => g.cost > 0).sort((a, b) => b.cost * (1 - b.default) - a.cost * (1 - a.default));
    const maxed = [];
    let extra = 0;
    for (const g of pricey) {
      if (expected + extra > catalog.budget + 10) break;
      if (g.name in set) continue;
      maxed.push(g);
      extra += g.cost * (1 - g.default);
      await setTrait(g.name, 1);
    }
    if (!(await page.isDisabled('#design-submit'))) throw new Error('over-budget design was not refused');
    if (!(await page.textContent('#design-problem')).includes('budget')) throw new Error('no over-budget message');
    for (const g of maxed) await setTrait(g.name, g.default);
    step(`live cost ${after.trim()}; over-budget design refused client-side`);
    await page.screenshot({ path: path.join(shots, 'design-editor.png') });
    const [founded] = await Promise.all([
      page.waitForResponse((r) => r.url().endsWith('/api/lineage')),
      page.click('#design-submit'),
    ]);
    const body = await founded.json();
    if (founded.status() !== 201 || Math.abs(body.cost - expected) > 0.06) throw new Error(`founding: ${founded.status()} ${JSON.stringify(body)}`);
    step(`founded lineage ${body.lineage} with ${body.founders.length} founders; server cost ${body.cost.toFixed(2)} matches the client`);

    // World view with organisms.
    await page.waitForSelector('#world-canvas', { timeout: 15000 });
    await waitFor('own organisms in state', () => page.evaluate(() => (window.evo?.model.state?.organisms.length ?? 0) > 0));
    await waitFor('markers drawn', () => page.evaluate(() => {
      const v = window.evo.screen()?.view;
      return v && v.markers.size > 0 && v.terrain && v.mapSprite.texture.width === v.terrain.width;
    }));
    const counts = await page.evaluate(() => ({ own: window.evo.model.state.organisms.length, nearby: window.evo.model.state.nearby.length, markers: window.evo.screen().view.markers.size }));
    step(`world view: ${counts.own} own + ${counts.nearby} nearby organisms, ${counts.markers} markers`);

    // Tile map: the terrain grid from /api/terrain, one texture pixel per tile.
    const tiles = await page.evaluate(() => {
      const v = window.evo.screen().view;
      const t = v.terrain;
      const buf = v.terrainBuf;
      const colors = new Set();
      for (let i = 0; i < t.width * t.height; i++) colors.add((buf[i * 4] << 16) | (buf[i * 4 + 1] << 8) | buf[i * 4 + 2]);
      return { w: t.width, h: t.height, tex: [v.mapSprite.texture.width, v.mapSprite.texture.height], biomes: new Set(t.biome).size, colors: colors.size, tick: t.tick };
    });
    if (tiles.w !== 64 || tiles.h !== 64 || tiles.tex[0] !== 64 || tiles.biomes < 3 || tiles.colors < 20) throw new Error(`tile map: ${JSON.stringify(tiles)}`);
    await waitFor('a dynamic terrain refresh', () => page.evaluate((t0) => window.evo.screen().view.terrain.tick > t0, tiles.tick), 15000);
    if (!(await page.textContent('#terrain-legend')).includes('drought')) throw new Error('no terrain legend');
    step(`tile map: ${tiles.w}×${tiles.h} tiles, ${tiles.biomes} biomes, ${tiles.colors} tile colors; dynamic layers refreshed`);
    await page.waitForFunction(() => document.querySelector('#conn-state')?.textContent === 'connected');

    // Record command results.
    await page.evaluate(() => {
      window.__results = [];
      const s = window.evo.socket;
      const orig = s.send.bind(s);
      s.send = async (m) => {
        const r = await orig(m);
        window.__results.push({ type: m.type, ok: r.ok, error: r.error, data: r.data });
        return r;
      };
    });

    // Zoom in, stop following and click one of our own organisms. Organisms
    // keep moving and foreign markers can overlap ours, so pick the own
    // organism with the most room around its marker, check that a click there
    // hits it, click, and retry with a fresh pick if the selection isn't ours.
    const canvasBox = await page.locator('#world-canvas').boundingBox();
    // Predators and thirst don't spare the organism under test: if it dies
    // before the walk is checked, start over with another one.
    const died = (id) => Object.assign(new Error(`#${id} died`), { died: true });
    for (let life = 0; ; life++) {
      try {
        let selected = null;
        for (let attempt = 0; attempt < 8 && selected === null; attempt++) {
          const pick = await page.evaluate(() => {
            const v = window.evo.screen().view;
            v.setFollow(false);
            v.cam.zoom = 14;
            const own = window.evo.model.state.organisms.filter((o) => v.markers.has(o.id));
            let best = null;
            for (const o of own) {
              const m = v.markers.get(o.id);
              let room = Infinity;
              for (const [id, n] of v.markers) if (id !== o.id) room = Math.min(room, Math.hypot(n.x - m.x, n.y - m.y));
              if (!best || room > best.room) best = { id: o.id, x: m.x, y: m.y, room };
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
          const got = await waitFor('selection', () => page.evaluate(() => window.evo.model.selected), 3000).catch(() => null);
          const own = await page.evaluate((id) => window.evo.model.state.organisms.some((o) => o.id === id), got);
          if (got !== null && own) selected = got;
        }
        if (selected === null) throw new Error('could not select an own organism by clicking it');
        await page.waitForSelector('#btn-possess', { timeout: 5000 });
        step(`clicked own organism #${selected}; inspector open`);

        await page.click('#btn-possess');
        const possessed = await waitFor('possession', () => page.evaluate(() => window.evo.model.state.organisms.find((o) => o.possessed)?.id));
        step(`possessed #${possessed}`);

        // Click-to-move: a point a few tiles away with no marker under it.
        const start = await page.evaluate((id) => window.evo.model.state.organisms.find((o) => o.id === id), possessed);
        if (!start) throw died(possessed);
        // An organism can wander under the cursor between the check and the
        // click (which then selects it): click a fresh spot until a move order goes out.
        const alive = () => page.evaluate((id) => window.evo.model.state.organisms.some((o) => o.id === id && o.possessed), possessed);
        let spot = null;
        let moveRes = null;
        for (let click = 0; click < 3 && !moveRes; click++) {
          spot = await page.evaluate(({ id }) => {
            const v = window.evo.screen().view;
            const o = window.evo.model.state.organisms.find((x) => x.id === id);
            if (!o) return 'died';
            for (const [dx, dy] of [[6, 0], [-6, 0], [0, 6], [0, -6], [5, 5], [-5, -5], [5, -5], [-5, 5]]) {
              const x = Math.min(62, Math.max(1, o.x + dx));
              const y = Math.min(62, Math.max(1, o.y + dy));
              const p = v.worldToScreen(x, y);
              if (v.hitTest(p.x, p.y) === null) return { x, y, p };
            }
            return null;
          }, { id: possessed });
          if (spot === 'died') throw died(possessed);
          if (!spot) throw new Error('no empty spot to click');
          await page.evaluate(() => { window.__results = window.__results.filter((r) => r.type !== 'order_move'); });
          await page.mouse.click(canvasBox.x + spot.p.x, canvasBox.y + spot.p.y);
          moveRes = await waitFor('order_move result', () => page.evaluate(() => window.__results.find((r) => r.type === 'order_move')), 4000).catch(() => null);
          if (!moveRes && !(await alive())) throw died(possessed);
        }
        if (!moveRes) throw new Error('clicking the map sent no order_move');
        if (!moveRes.ok) throw new Error(`order_move failed: ${JSON.stringify(moveRes.error)}`);
        const d0 = Math.hypot(start.x - spot.x, start.y - spot.y);
        const closer = await waitFor('organism walks toward the target', () => page.evaluate(({ id, spot, d0 }) => {
          const o = window.evo.model.state.organisms.find((x) => x.id === id);
          if (!o) return 'died';
          const d = Math.hypot(o.x - spot.x, o.y - spot.y);
          return d < d0 - 1 ? { d, action: o.action } : null;
        }, { id: possessed, spot, d0 }), 15000);
        if (closer === 'died') throw died(possessed);
        step(`order_move ok; #${possessed} walked from ${d0.toFixed(1)} to ${closer.d.toFixed(1)} tiles of the target (${closer.action})`);
        break;
      } catch (e) {
        if (!e.died || life >= 2) throw e;
        step(`${e.message} during the test; picking another organism`);
      }
    }

    // Group command and a server-side error surfaced as a toast.
    await page.click('#migrate');
    await page.mouse.click(canvasBox.x + 200, canvasBox.y + 600);
    const mig = await waitFor('migrate result', () => page.evaluate(() => window.__results.find((r) => r.type === 'migrate')));
    if (!mig.ok) throw new Error(`migrate failed: ${JSON.stringify(mig.error)}`);
    await page.evaluate(() => window.evo.socket.send({ type: 'possess', organism: 999999 }));
    await page.waitForSelector('.toast.error', { timeout: 5000 });
    step('migrate ok; command errors shown as toasts');

    await page.evaluate(() => window.evo.screen().view.setFollow(true));
    await page.click('#tab-organism');
    await page.waitForTimeout(1200);
    await page.screenshot({ path: path.join(shots, 'world-view.png') });
    await page.click('#tab-lineage');
    await page.waitForSelector('#season-boards table.board', { timeout: 15000 });
    await page.waitForTimeout(300);
    await page.screenshot({ path: path.join(shots, 'world-lineage.png') });
    step('lineage tab: seasonal leaderboard shown');

    // Genetics dashboard: charts from /api/lineage/stats and the alerts feed.
    await page.click('#tab-genetics');
    await page.waitForSelector('#dashboard svg.chart', { timeout: 10000 });
    const dash = await waitFor('dashboard with recorded days', () => page.evaluate(() => {
      const rows = document.querySelectorAll('#drift-list .drift-row').length;
      const charts = document.querySelectorAll('#dashboard svg.chart').length;
      const genes = document.querySelectorAll('#gene-table tbody tr').length;
      const paths = [...document.querySelectorAll('#dashboard svg.chart path')].filter((p) => (p.getAttribute('d') ?? '').includes('L')).length;
      const feed = document.querySelectorAll('#alerts-feed .feed-item').length;
      return rows > 0 && paths >= 2 && feed > 0 ? { rows, charts, genes, paths, feed } : null;
    }), 20000);
    if (dash.charts < 4 || dash.genes < 30) throw new Error(`dashboard: ${JSON.stringify(dash)}`);
    step(`genetics dashboard: ${dash.charts} charts, ${dash.rows} drift sparklines, ${dash.genes} genes, ${dash.feed} alerts`);
    await page.screenshot({ path: path.join(shots, 'genetics-dashboard.png') });
    await page.$eval('#panel-genetics', (el) => { el.scrollTop = el.scrollHeight; });
    await page.waitForTimeout(200);
    await page.screenshot({ path: path.join(shots, 'genetics-dashboard-feed.png') });

    // Directed mating between two players through the panels. Bob founds
    // the same body plan and preferences as ours, so the pair is fully
    // fertile and inside both preferences (no override cost, no failure
    // chance): the mating completes and conceives once the two meet.
    const bob = await browser.newPage({ viewport: { width: 1280, height: 800 }, deviceScaleFactor: 1 });
    pages.push(bob);
    bob.on('pageerror', (e) => pageErrors.push(`bob: ${e.message}`));
    await bob.goto(base);
    await bob.fill('#login-name', `${name}_b`);
    await bob.fill('#login-password', 'a long password');
    await bob.click('#register-submit');
    await bob.waitForSelector('#design-name');
    await bob.fill('#design-name', 'Bob striders');
    for (const [g, v] of Object.entries(set)) {
      await bob.$eval(`input[data-gene="${g}"]`, (el, v) => {
        el.value = String(v);
        el.dispatchEvent(new Event('input'));
      }, v);
    }
    await bob.click('#design-submit');
    await bob.waitForSelector('#world-canvas');
    await waitFor('bob state', () => bob.evaluate(() => (window.evo?.model.state?.organisms.length ?? 0) > 0), 40000);
    await page.click('#tab-matings');
    await bob.click('#tab-matings');
    const offerOf = (p, id) => p.evaluate((id) => {
      const e = window.evo.model.offers.get(id);
      return e ? { status: e.offer.status, outcome: e.outcome ?? null, error: e.error ?? null } : null;
    }, id);
    let done = null;
    // An agreed female is held from breeding on her own, but predators don't
    // wait: if an organism dies on the way, try another pair.
    for (let round = 0; round < 5 && !done; round++) {
      // Organisms keep breeding on their own, so a chosen female can conceive
      // before the request lands: pick again on those refusals.
      let pair = null;
      for (let attempt = 0; attempt < 12 && !pair; attempt++) {
        const cand = await waitFor('a pair of adults', async () => {
          const adults = (p) => p.evaluate(() => window.evo.model.state.organisms.filter((o) => o.matured && !o.pregnant)
            .map((o) => ({ id: o.id, sex: o.sex, urge: o.needs.breeding })));
          const [a, b] = [await adults(page), await adults(bob)];
          // Prefer our male (males never become pregnant) and females with
          // the least breeding urge: in fast mode a courting female tends to
          // conceive with one of her own before the request lands (the
          // state is a second, a whole day, old).
          a.sort((x, y) => (x.sex === 'male' ? -1 : 1) - (y.sex === 'male' ? -1 : 1) || x.urge - y.urge);
          b.sort((x, y) => x.urge - y.urge);
          for (const x of a) {
            const y = b.find((o) => o.sex !== x.sex);
            if (y) return { mine: x.id, partner: y.id };
          }
          return null;
        }, 60000);
        await page.evaluate(() => { window.__results = []; });
        // Request right away: a female left waiting conceives with one of her
        // own (the request is refused, and we pick again).
        // (Gone from the list if it just died: pick again.)
        if (!(await page.selectOption('#mating-mine', String(cand.mine), { timeout: 3000 }).catch(() => null))) continue;
        await page.fill('#mating-partner', String(cand.partner));
        await page.fill('#mating-price', '25');
        // (The list of adults is rebuilt when one dies: check the choice held.)
        if (Number(await page.inputValue('#mating-mine')) !== cand.mine) continue;
        await page.click('#mating-submit');
        const req = await waitFor('request result', () => page.evaluate(() => window.__results.find((r) => r.type === 'request_mating')));
        if (!req.ok) {
          console.log(`    (#${cand.mine} × #${cand.partner}: ${req.error?.code}, picking again)`);
          if (!['already_pregnant', 'not_mature', 'not_found', 'died'].includes(req.error?.code)) throw new Error(`request_mating failed: ${JSON.stringify(req.error)}`);
          await page.waitForTimeout(500);
          continue;
        }
        // The form shows the pair's compatibility (server preview).
        const compat = await waitFor('compatibility preview', () => page.evaluate((pair) => {
          const el = document.querySelector('#mating-compat');
          return el?.dataset.pair === pair ? el.textContent : null;
        }, `${cand.mine}:${cand.partner}`), 10000);
        // (A preview error means one of them just died; the offer then fails.)
        if (!compat.startsWith("Can't preview") && !compat.startsWith('Compatible: fertility')) throw new Error(`same designs not shown compatible: ${compat}`);
        pair = { ...cand, compat, id: req.data.offer.id };
      }
      if (!pair) throw new Error('no mating request went through');
      const id = pair.id;
      step(`ann asked for #${pair.mine} × bob's #${pair.partner} at price 25 (${pair.compat})`);
      // (Unless it already failed: one of them died.)
      const shown = await waitFor('the request on bob\'s side', async () => {
        if ((await offerOf(page, id))?.status === 'failed') return 'failed';
        return (await bob.$(`#incoming .offer[data-offer="${id}"] button.accept`)) && 'shown';
      }, 10000);
      if (shown === 'shown') await bob.click(`#incoming .offer[data-offer="${id}"] button.accept`, { timeout: 5000 }).catch(() => {});
      const status = await waitFor('mating update on both sides', async () => {
        const [sa, sb] = [await offerOf(page, id), await offerOf(bob, id)];
        return sa && sb && sa.status !== 'pending' && sa.status === sb.status ? sa.status : null;
      });
      step(`bob accepted; both sides see the offer ${status}`);
      if (status === 'accepted') {
        // Escort: possess our organism and walk it to the partner, following
        // her as she moves (her position as bob sees it).
        await page.click(`#open-offers .offer[data-offer="${id}"] button:has-text("Escort")`, { timeout: 5000 }).catch(() => {});
        // Possessed, unless it died on the way to the button (the offer then fails).
        const esc = await waitFor('escort possession', async () => {
          if ((await offerOf(page, id))?.status !== 'accepted') return 'closed';
          return page.evaluate((id) => window.evo.model.state.organisms.find((o) => o.id === id)?.possessed && 'possessed', pair.mine);
        });
        if (esc === 'possessed') step(`escort: #${pair.mine} possessed for the trip to the meeting`);
        await waitFor('the meeting', async () => {
          const o = await offerOf(page, id);
          if (o && o.status !== 'accepted') return true;
          const p = await bob.evaluate((id) => window.evo.model.state.organisms.find((o) => o.id === id), pair.partner);
          if (p) await page.evaluate(({ organism, x, y }) => window.evo.socket.send({ type: 'order_move', organism, x, y }), { organism: pair.mine, x: p.x, y: p.y });
          await new Promise((r) => setTimeout(r, 400));
          return false;
        }, 90000);
      }
      const fin = await waitFor('final status on both sides', async () => {
        const [sa, sb] = [await offerOf(page, id), await offerOf(bob, id)];
        return sa && sb && !['pending', 'accepted'].includes(sa.status) && sa.status === sb.status ? sa : null;
      });
      if (fin.status === 'failed' && fin.error?.code === 'died') {
        step(`offer failed: ${fin.error.message}; trying another pair`);
        continue;
      }
      if (fin.status !== 'completed' || !fin.outcome) throw new Error(`mating ended ${JSON.stringify(fin)}`);
      const o = fin.outcome;
      if (!(o.fertility > 0.5)) throw new Error(`same designs not fertile: ${JSON.stringify(o)}`);
      // Inside both preferences there is no override failure chance, so a
      // fertile pair always conceives.
      if (o.cost === 0 && !o.conceived) throw new Error(`no conception at no cost: ${JSON.stringify(o)}`);
      done = o;
      step(`mating completed: ${o.conceived ? 'conceived' : 'not conceived'} (fertility ${o.fertility.toFixed(2)}, mismatch ${o.mismatch.toFixed(2)}, cost ${o.cost.toFixed(2)})`);
    }
    if (!done) throw new Error('every escorted mating lost an organism to predators');
    await bob.waitForTimeout(500);
    await bob.screenshot({ path: path.join(shots, 'matings-bob.png') });
    await page.screenshot({ path: path.join(shots, 'matings-ann.png') });

    if (pageErrors.length) throw new Error(`page errors: ${pageErrors.join('; ')}`);
    console.log(`smoke test passed; screenshots in ${shots}`);
  } catch (e) {
    console.error(serverLog.split('\n').slice(-20).join('\n'));
    for (const [k, p] of pages.entries()) await p.screenshot({ path: path.join(shots, `failure-${k}.png`) }).catch(() => {});
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
