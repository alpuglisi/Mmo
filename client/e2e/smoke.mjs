// End-to-end smoke test: start evo-server (fast, in memory) serving the built
// client, then in headless Chromium register, found a lineage, open the
// world view, check the tile map and organisms are drawn, run a directed
// mating with a second player of the same body plan (preview its
// compatibility, request, accept, escort until the two meet, and check it
// completes and conceives), possess an organism and walk it, open the
// genetics dashboard, the seasonal leaderboard and the World tab.
//
// The world keeps running throughout, so every step waits on explicit state
// conditions (generous timeouts, no fixed sleeps for correctness) and retries
// what the living world can legitimately interrupt.
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
        window.__results.push({ type: m.type, ok: r.ok, error: r.error, data: r.data, x: m.x, y: m.y, mine: m.mine, partner: m.partner });
        return r;
      };
    });

    // Directed mating between two players through the panels. Bob founds
    // the same body plan and preferences as ours, so the pair is fully
    // fertile and inside both preferences (no override cost, no failure
    // chance): the mating completes and conceives once the two meet. This
    // runs early, while both lineages are young: later on a loaded machine
    // many more world days pass and a lineage can dwindle.
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
    // Hand back every possessed organism (an escort's, or bob's walker), so
    // the next round, and the possession step below, start from scratch.
    const releaseAll = (p) => p.evaluate(async () => {
      for (const o of window.evo.model.state.organisms.filter((o) => o.possessed)) await window.evo.socket.send({ type: 'release', organism: o.id });
    });
    // Bob accepts every incoming request the moment its Accept button
    // appears: a pending request doesn't hold his female (only his consent
    // does), so until he answers she can still conceive with one of her own,
    // and in fast mode a day passes every second.
    await bob.evaluate(() => {
      const accept = () => document.querySelectorAll('#incoming .offer button.accept').forEach((b) => {
        if (!b.dataset.clicked) { b.dataset.clicked = '1'; b.click(); }
      });
      new MutationObserver(accept).observe(document.body, { childList: true, subtree: true });
      accept();
    });
    let done = null;
    // The living world can spoil a round without the mating rules being at
    // fault: a predator takes one of the pair on the way (`died`), her own
    // kind gets her pregnant before bob's consent holds her
    // (`already_pregnant`), or the season turns and expires the agreement.
    // Those rounds are retried with another pair (up to 5 retries); any other
    // ending fails the test.
    const rounds = 6;
    for (let round = 1; round <= rounds && !done; round++) {
      if (round > 1) await Promise.all([releaseAll(page), releaseAll(bob)]);
      // Organisms keep breeding on their own, so a chosen female can conceive
      // before the request lands: pick again on those refusals.
      let pair = null;
      for (let attempt = 0; attempt < 12 && !pair; attempt++) {
        let seen = '';
        const cand = await waitFor('a pair of adults', async () => {
          const adults = (p) => p.evaluate(() => window.evo.model.state.organisms.filter((o) => o.matured && !o.pregnant)
            .map((o) => ({ id: o.id, sex: o.sex, urge: o.needs.breeding, x: o.x, y: o.y })));
          const [a, b] = [await adults(page), await adults(bob)];
          const tally = (l) => `${l.filter((o) => o.sex === 'female').length}F/${l.filter((o) => o.sex === 'male').length}M`;
          seen = `ann ${tally(a)}, bob ${tally(b)} open adults`;
          // Prefer the closest pair: predators pick off organisms on a long
          // trip to the meeting. Then our female with bob's male: our own
          // female is held from the moment we ask (our consent), bob's only
          // once he accepts, and males never become pregnant. Then the least
          // breeding urge in the female: a courting female tends to conceive
          // with one of her own before the request lands.
          let best = null;
          for (const x of a) {
            for (const y of b) {
              if (x.sex === y.sex) continue;
              const female = x.sex === 'female' ? x : y;
              const score = Math.hypot(x.x - y.x, x.y - y.y) + (x.sex === 'female' ? 0 : 8) + female.urge / 20;
              if (!best || score < best.score) best = { mine: x.id, partner: y.id, score };
            }
          }
          return best;
          // (In fast mode every adult female can be pregnant for a while.)
        }, 120000).catch((e) => { throw new Error(`${e.message}; ${seen}`); });
        await page.evaluate(() => { window.__results = []; });
        // The form lists living adults; if our pick died since, pick again.
        if (!(await page.selectOption('#mating-mine', String(cand.mine), { timeout: 3000 }).then(() => true, () => false))) {
          console.log(`  … #${cand.mine} is no longer in the form; picking again`);
          continue;
        }
        await page.fill('#mating-partner', String(cand.partner));
        await page.fill('#mating-price', '25');
        // (The list of adults is rebuilt when one dies: check the choice held.)
        if (Number(await page.inputValue('#mating-mine')) !== cand.mine) continue;
        await page.click('#mating-submit');
        const req = await waitFor('request result', () => page.evaluate(() => window.__results.find((r) => r.type === 'request_mating')));
        // If our pick died before the click, the form falls back to another
        // adult (which can be the partner's sex): retry then, too.
        const swapped = req.mine !== cand.mine || req.partner !== cand.partner;
        if (!req.ok) {
          if (!swapped && !['already_pregnant', 'not_mature', 'not_found', 'died'].includes(req.error?.code)) throw new Error(`request_mating failed: ${JSON.stringify(req.error)} for ${JSON.stringify(cand)}`);
          console.log(`  … request_mating refused (${req.error?.code}${swapped ? `, form sent #${req.mine} × #${req.partner}` : ''}); picking again`);
          await page.waitForTimeout(500);
          continue;
        }
        const id = req.data.offer.id;
        // Bob's page accepts it as soon as the request shows (see above).
        await waitFor('bob\'s answer', async () => {
          const o = await offerOf(bob, id);
          return o && o.status !== 'pending';
        }, 10000);
        // The form shows the pair's compatibility (server preview). If our
        // organism died meanwhile, the rebuilt list falls back to another
        // adult and the preview shows that pair: the offer then fails with
        // `died` and another pair is tried below.
        const compat = await waitFor('compatibility preview', async () => {
          if ((await offerOf(page, id))?.status === 'failed') return '(offer failed)';
          return page.evaluate((pair) => {
            const el = document.querySelector('#mating-compat');
            if (el?.dataset.pair === pair) return el.textContent;
            return document.querySelector('#mating-mine')?.value !== pair.split(':')[0] ? '(selection changed)' : null;
          }, `${req.mine}:${req.partner}`);
        }, 10000);
        // (A preview error means one of them just died; the offer then fails.)
        if (!compat.startsWith('(') && !compat.startsWith("Can't preview") && !compat.startsWith('Compatible: fertility')) throw new Error(`same designs not shown compatible: ${compat}`);
        pair = { mine: req.mine, partner: req.partner, compat, id };
      }
      if (!pair) throw new Error('no mating request went through');
      const id = pair.id;
      step(`ann asked for #${pair.mine} × bob's #${pair.partner} at price 25 (${pair.compat})`);
      const status = await waitFor('mating update on both sides', async () => {
        const [sa, sb] = [await offerOf(page, id), await offerOf(bob, id)];
        return sa && sb && sa.status !== 'pending' && sa.status === sb.status ? sa.status : null;
      });
      step(`bob accepted; both sides see the offer ${status}`);
      if (status === 'accepted') {
        // Escort: possess our organism and walk it to the partner, following
        // her as she moves (her position as bob sees it). The agreement can
        // resolve on its own before the click, which takes the button away.
        await page.click(`#open-offers .offer[data-offer="${id}"] button:has-text("Escort")`, { timeout: 5000 }).catch(() => {});
        const esc = await waitFor('escort possession', async () => {
          if ((await offerOf(page, id))?.status !== 'accepted') return 'closed';
          return page.evaluate((id) => window.evo.model.state.organisms.find((o) => o.id === id)?.possessed && 'possessed', pair.mine);
        });
        if (esc === 'possessed') step(`escort: #${pair.mine} possessed for the trip to the meeting`);
        // Bob walks his organism toward ours too: half the trip each.
        await bob.evaluate((organism) => window.evo.socket.send({ type: 'possess', organism }), pair.partner);
        const met = await waitFor('the meeting', async () => {
          const o = await offerOf(page, id);
          if (o && o.status !== 'accepted') return true;
          const p = await bob.evaluate((id) => window.evo.model.state.organisms.find((o) => o.id === id), pair.partner);
          if (p) await page.evaluate(({ organism, x, y }) => window.evo.socket.send({ type: 'order_move', organism, x, y }), { organism: pair.mine, x: p.x, y: p.y });
          const m = await page.evaluate((id) => window.evo.model.state.organisms.find((o) => o.id === id), pair.mine);
          if (m) await bob.evaluate(({ organism, x, y }) => window.evo.socket.send({ type: 'order_move', organism, x, y }), { organism: pair.partner, x: m.x, y: m.y });
          await new Promise((r) => setTimeout(r, 400));
          return false;
        }, 90000).catch(() => false);
        if (!met) {
          const [a, b] = await Promise.all([
            page.evaluate((id) => window.evo.model.state.organisms.find((o) => o.id === id), pair.mine),
            bob.evaluate((id) => window.evo.model.state.organisms.find((o) => o.id === id), pair.partner),
          ]);
          const gap = a && b ? `${Math.hypot(a.x - b.x, a.y - b.y).toFixed(1)} tiles apart` : 'one of them gone';
          throw new Error(`the escorted pair #${pair.mine} × #${pair.partner} never met in 90 s (${gap})`);
        }
      }
      const fin = await waitFor('final status on both sides', async () => {
        const [sa, sb] = [await offerOf(page, id), await offerOf(bob, id)];
        return sa && sb && !['pending', 'accepted'].includes(sa.status) && sa.status === sb.status ? sa : null;
      });
      if (fin.status === 'expired') {
        step(`the season turned and expired the agreement; trying another pair (round ${round}/${rounds})`);
        continue;
      }
      if (fin.status === 'failed' && ['died', 'already_pregnant'].includes(fin.error?.code)) {
        const why = fin.error.code === 'died' ? fin.error.message : 'she conceived before bob answered';
        step(`offer failed: ${why}; trying another pair (round ${round}/${rounds})`);
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
    if (!done) throw new Error(`every one of ${rounds} escorted matings lost an organism, expired, or its female conceived before bob answered`);
    await Promise.all([releaseAll(page), releaseAll(bob)]);
    await waitFor('release', () => page.evaluate(() => !window.evo.model.state.organisms.some((o) => o.possessed)));
    await bob.waitForTimeout(500);
    await bob.screenshot({ path: path.join(shots, 'matings-bob.png') });
    await page.screenshot({ path: path.join(shots, 'matings-ann.png') });

    // Stud marketplace: ann lists one of her females as a dam through the
    // Market tab (a listed dam is held from breeding on her own, and bob's
    // male can't become pregnant, so no pregnancy can spoil the pair
    // meanwhile), bob finds it with his male's compatibility and buys it (an
    // accepted agreement, the fee in escrow), both walk toward each other,
    // and the escrow must settle consistently: paid to ann if the mating
    // runs, refunded to bob on any other ending (a predator, a season
    // change, or bob cancelling after 12 s apart). It runs right after the directed
    // mating, while both lineages are young, and keeps its world time short.
    await bob.evaluate(() => {
      window.__results = [];
      const s = window.evo.socket;
      const orig = s.send.bind(s);
      s.send = async (m) => {
        const r = await orig(m);
        window.__results.push({ type: m.type, ok: r.ok, error: r.error, data: r.data, mine: m.mine, listing: m.listing });
        return r;
      };
    });
    const renown = (p) => p.evaluate(() => ({ balance: window.evo.model.state.balance, escrow: window.evo.model.state.escrow }));
    const ledgerOf = (p) => p.evaluate(async () => (await window.evo.socket.send({ type: 'ledger', limit: 100 })).data);
    const fee = 10;
    await page.click('#tab-market');
    await bob.click('#tab-market');
    let bought = null;
    let listingId = null;
    for (let attempt = 0; attempt < 8 && !bought; attempt++) {
      // Ann's adult female closest to one of bob's males. She may be
      // pregnant: once she has given birth the listing's hold keeps her from
      // conceiving again, so bob waits for her below.
      let seen = '';
      const cand = await waitFor('a pair for the market', async () => {
        const adults = (p) => p.evaluate(() => {
          const s = window.evo.model.state;
          return { all: s.organisms.length, adults: s.organisms.filter((o) => o.matured && !o.possessed).map((o) => ({ id: o.id, sex: o.sex, x: o.x, y: o.y, pregnant: o.pregnant })) };
        });
        const [a, b] = [await adults(page), await adults(bob)];
        const females = a.adults.filter((o) => o.sex === 'female');
        const males = b.adults.filter((o) => o.sex === 'male');
        seen = `ann ${a.all} organisms, ${females.length} adult females; bob ${b.all} organisms, ${males.length} adult males`;
        let best = null;
        for (const x of females) {
          for (const y of males) {
            const score = Math.hypot(x.x - y.x, x.y - y.y) + (x.pregnant ? 10 : 0);
            if (!best || score < best.score) best = { dam: x.id, buyer: y.id, score };
          }
        }
        return best;
      }, 60000).catch((e) => { throw new Error(`${e.message}; ${seen}`); });
      // List her (withdrawing an earlier attempt's listing first).
      if (listingId !== null) await page.evaluate((listing) => window.evo.socket.send({ type: 'withdraw_listing', listing }), listingId);
      await page.evaluate(() => { window.__results = []; });
      await page.click('#market-refresh');
      if (!(await page.selectOption('#market-list-organism', String(cand.dam), { timeout: 3000 }).then(() => true, () => false))) continue;
      await page.fill('#market-fee', String(fee));
      await page.click('#market-list-submit');
      const listed = await waitFor('create_listing result', () => page.evaluate(() => window.__results.find((r) => r.type === 'create_listing')));
      if (!listed.ok) {
        if (['not_found', 'not_mature', 'already_listed'].includes(listed.error?.code)) {
          console.log(`  … create_listing refused (${listed.error.code}); picking again`);
          continue;
        }
        throw new Error(`create_listing failed: ${JSON.stringify(listed.error)}`);
      }
      listingId = listed.data.listing.id;
      await page.waitForSelector(`#my-listings .listing[data-listing="${listingId}"]`, { timeout: 10000 });
      // Bob picks his male; the listing shows with its compatibility.
      if (!(await bob.selectOption('#market-mine', String(cand.buyer), { timeout: 3000 }).then(() => true, () => false))) continue;
      await bob.click('#market-refresh');
      const card = `#market-listings .listing[data-listing="${listingId}"]`;
      const compat = await waitFor('listing with compatibility', () => bob.evaluate((card) => document.querySelector(`${card} .compat`)?.textContent ?? null, card), 15000).catch(() => null);
      if (!compat) {
        console.log('  … the listing showed no compatibility (an organism died?); picking again');
        continue;
      }
      if (!compat.startsWith('Compatible: fertility')) throw new Error(`same designs not shown compatible in the market: ${compat}`);
      // Wait until the buy button is enabled: a pregnant dam first gives birth.
      const ready = await waitFor('the dam to be available', async () => {
        if (await bob.isEnabled(`${card} button.buy`).catch(() => false)) return 'ready';
        const why = (await bob.textContent(`${card} .row .muted`, { timeout: 1000 }).catch(() => null)) ?? 'listing gone';
        if (!why.includes('pregnant')) return why;
        await bob.click('#market-refresh');
        await new Promise((r) => setTimeout(r, 1000));
        return null;
      }, 25000).catch(() => 'still pregnant');
      if (ready !== 'ready') {
        if (/renown|slots/.test(ready)) throw new Error(`cannot buy: ${ready}; renown ${JSON.stringify(await renown(bob))}`);
        console.log(`  … buy not possible (${ready}); picking again`);
        continue;
      }
      const before = await renown(bob);
      await bob.evaluate(() => { window.__results = []; });
      // (A plain DOM click: the cards are rebuilt when what they show
      // changes, which can detach a button under a slower synthetic click.)
      const clicked = await bob.evaluate((sel) => {
        const b = document.querySelector(sel);
        if (!b || b.disabled) return false;
        b.click();
        return true;
      }, `${card} button.buy`);
      if (!clicked) {
        console.log('  … the buy button went away (listing closed or no longer buyable); picking again');
        continue;
      }
      const r = await waitFor('buy_listing result', () => bob.evaluate(() => window.__results.find((r) => r.type === 'buy_listing')));
      if (!r.ok) {
        if (['already_pregnant', 'not_found', 'died', 'not_mature', 'unknown_listing'].includes(r.error?.code)) {
          console.log(`  … buy_listing refused (${r.error.code}); picking again`);
          continue;
        }
        throw new Error(`buy_listing failed: ${JSON.stringify(r.error)}`);
      }
      bought = { id: r.data.offer.id, dam: cand.dam, buyer: r.mine, compat, before };
      await bob.screenshot({ path: path.join(shots, 'market-listing.png') });
    }
    if (!bought) throw new Error('no listing purchase went through');
    step(`ann listed her dam #${bought.dam} for ${fee} renown; bob bought it for #${bought.buyer} (${bought.compat})`);
    // The agreement is already accepted on both sides, the fee held in escrow.
    await waitFor('accepted agreement on both sides', async () => {
      const [sa, sb] = [await offerOf(page, bought.id), await offerOf(bob, bought.id)];
      return sa && sb && sa.status === sb.status && sa.status !== 'pending';
    });
    const held = await waitFor('escrow in bob\'s state', async () => {
      const o = await offerOf(bob, bought.id);
      const r = await renown(bob);
      // Already settled (they happened to meet at once) or held.
      return o && o.status !== 'accepted' ? { settled: true, ...r } : r.escrow >= fee ? r : null;
    });
    // (Daily income keeps arriving, so the balance itself is checked through
    // the ledger below.)
    if (!held.settled && held.escrow < fee) throw new Error(`escrow: ${JSON.stringify(bought.before)} → ${JSON.stringify(held)}`);
    await waitFor('ann told of the sale', () => page.evaluate(() => window.evo.model.feed.some((e) => e.kind === 'market' && e.text.includes('bought a mating'))), 10000);
    step(`purchase: offer ${bought.id} accepted at once, ${fee} renown in bob's escrow; ann alerted`);
    // Both walk toward each other; give up (cancel) after 12 s apart.
    await bob.evaluate((organism) => window.evo.socket.send({ type: 'possess', organism }), bought.buyer);
    await page.evaluate((organism) => window.evo.socket.send({ type: 'possess', organism }), bought.dam);
    const met = await waitFor('the market pair meeting', async () => {
      const o = await offerOf(bob, bought.id);
      if (o && o.status !== 'accepted') return true;
      const a = await page.evaluate((id) => window.evo.model.state.organisms.find((o) => o.id === id), bought.dam);
      const b = await bob.evaluate((id) => window.evo.model.state.organisms.find((o) => o.id === id), bought.buyer);
      if (a && b) {
        await page.evaluate(({ organism, x, y }) => window.evo.socket.send({ type: 'order_move', organism, x, y }), { organism: bought.dam, x: b.x, y: b.y });
        await bob.evaluate(({ organism, x, y }) => window.evo.socket.send({ type: 'order_move', organism, x, y }), { organism: bought.buyer, x: a.x, y: a.y });
      }
      await new Promise((r) => setTimeout(r, 400));
      return false;
    }, 12000).catch(() => false);
    if (!met) await bob.evaluate((request) => window.evo.socket.send({ type: 'cancel_mating', request }), bought.id);
    const fin = await waitFor('the purchase closing on both sides', async () => {
      const [sa, sb] = [await offerOf(page, bought.id), await offerOf(bob, bought.id)];
      return sa && sb && !['pending', 'accepted'].includes(sa.status) && sa.status === sb.status ? sa : null;
    });
    // Settlement: the ledgers must agree with the outcome.
    const [annLedger, bobLedger] = [await ledgerOf(page), await ledgerOf(bob)];
    const entry = (l, kind) => l.entries.find((e) => e.kind === kind && e.offer === bought.id);
    if (!entry(bobLedger, 'escrow') || entry(bobLedger, 'escrow').delta !== -fee) throw new Error(`no escrow entry: ${JSON.stringify(bobLedger.entries.slice(0, 5))}`);
    if (fin.status === 'completed') {
      if (entry(annLedger, 'sale')?.delta !== fee) throw new Error(`completed but ann not paid: ${JSON.stringify(annLedger.entries.slice(0, 5))}`);
      if (entry(bobLedger, 'refund')) throw new Error('completed and refunded');
      step(`market mating completed (${fin.outcome?.conceived ? 'conceived' : 'not conceived'}): ann paid ${fee} renown from escrow`);
    } else {
      if (entry(bobLedger, 'refund')?.delta !== fee) throw new Error(`${fin.status} but bob not refunded: ${JSON.stringify(bobLedger.entries.slice(0, 5))}`);
      if (entry(annLedger, 'sale')) throw new Error(`${fin.status} and paid`);
      step(`market agreement ${fin.status}${fin.error ? ` (${fin.error.code})` : ''}: bob refunded ${fee} renown`);
    }
    await waitFor('escrow released', async () => (await renown(bob)).escrow === 0);
    // The balance shows in the top bar; the ledger in the Market tab.
    const pill = await bob.textContent('#balance');
    if (!/\d+ renown/.test(pill ?? '')) throw new Error(`no balance in the top bar: ${pill}`);
    await waitFor('ledger rows', () => bob.evaluate(() => document.querySelectorAll('#market-ledger tr[data-seq]').length >= 3), 15000);
    // Ann withdraws the listing.
    await page.click(`#my-listings .listing[data-listing="${listingId}"] button.withdraw`);
    await waitFor('listing withdrawn', () => page.evaluate((id) => !document.querySelector(`#my-listings .listing[data-listing="${id}"]`), listingId), 10000);
    step(`ledgers agree; balance pill "${pill.trim()}"; ann withdrew listing ${listingId}`);
    await page.screenshot({ path: path.join(shots, 'market-ann.png') });
    await bob.screenshot({ path: path.join(shots, 'market-bob.png') });
    await Promise.all([releaseAll(page), releaseAll(bob)]);
    await waitFor('release after the market', () => page.evaluate(() => !window.evo.model.state.organisms.some((o) => o.possessed)));
    await page.click('#tab-matings');

    // Select by clicking, possess and walk one of our own organisms. The
    // world keeps running: markers move, organisms die (juveniles often), a
    // possessed organism may break off an order for danger, a severe need or
    // night torpor. So each part waits on explicit state and retries, and if
    // the organism itself is lost the whole round restarts with a new one.
    const canvasBox = await page.locator('#world-canvas').boundingBox();
    class Retry extends Error {}
    const alive = (id) => page.evaluate((id) => window.evo.model.state.organisms.some((o) => o.id === id), id);

    // Zoom in, stop following and click one of our own organisms: an adult
    // (they live longer) with the most room around its marker, checked with
    // the view's hit test; retry with a fresh pick if the selection isn't ours.
    async function clickOwn(exclude) {
      for (let attempt = 0; attempt < 10; attempt++) {
        const pick = await page.evaluate((exclude) => {
          const v = window.evo.screen().view;
          v.setFollow(false);
          v.cam.zoom = 14;
          const own = window.evo.model.state.organisms.filter((o) => v.markers.has(o.id) && !exclude.includes(o.id));
          let best = null;
          for (const o of own) {
            const m = v.markers.get(o.id);
            let room = Infinity;
            for (const [id, n] of v.markers) if (id !== o.id) room = Math.min(room, Math.hypot(n.x - m.x, n.y - m.y));
            const score = Math.min(room, 3) + (o.matured ? 3 : 0) + o.health / 100;
            if (!best || score > best.score) best = { id: o.id, x: m.x, y: m.y, score };
          }
          if (best) v.centerOn(best.x, best.y);
          return best;
        }, exclude);
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
        if (got !== null && (await alive(got))) return got;
      }
      throw new Retry('could not select an own organism by clicking it');
    }

    // Click-to-move: a dry-land point a few tiles away, clear of markers. The
    // check tracks the closest the organism got to the target over generous
    // time and counts an arrival (the server drops the order within 0.75
    // tiles); if it stalls, the order is re-issued from where it is now. A
    // click that lands on a marker that just moved there selects it instead
    // of ordering: that attempt is retried too.
    const pickSpot = (id) => page.evaluate((id) => {
      const v = window.evo.screen().view;
      const t = v.terrain;
      const o = window.evo.model.state.organisms.find((x) => x.id === id);
      if (!o) return null;
      // Land biomes (grass, forest, desert, tundra) with no water now.
      const dry = (x, y) => {
        const i = Math.floor(y) * t.width + Math.floor(x);
        return t.biome[i] >= 3 && t.biome[i] <= 6 && t.water[i] === 0;
      };
      const dirs = [];
      for (const r of [6, 5, 4]) for (let k = 0; k < 16; k++) dirs.push([r * Math.cos((k * Math.PI) / 8), r * Math.sin((k * Math.PI) / 8)]);
      let fallback = null;
      for (const [dx, dy] of dirs) {
        const x = Math.min(62, Math.max(1, o.x + dx));
        const y = Math.min(62, Math.max(1, o.y + dy));
        if (Math.hypot(x - o.x, y - o.y) < 3.5) continue;
        const p = v.worldToScreen(x, y);
        if (v.hitTest(p.x, p.y) !== null) continue;
        // Keep clear of every marker by 1.5 tiles: they move before the click.
        if ([...v.markers.values()].some((m) => Math.hypot(m.x - x, m.y - y) < 1.5)) continue;
        const spot = { x, y, p, from: { x: o.x, y: o.y } };
        if (dry(x, y) && dry((x + o.x) / 2, (y + o.y) / 2)) return spot;
        fallback ??= spot;
      }
      return fallback;
    }, id);

    async function walkTo(id) {
      const actions = new Set();
      const attempts = 5;
      for (let attempt = 1; attempt <= attempts; attempt++) {
        if (!(await alive(id))) throw new Retry(`#${id} died`);
        // Keep the possessed organism centred so the click lands on the map.
        await page.evaluate((id) => {
          const v = window.evo.screen().view;
          const p = v.markers.get(id);
          if (p) v.centerOn(p.x, p.y);
        }, id);
        await page.waitForTimeout(150);
        const spot = await pickSpot(id);
        if (!spot) throw new Retry('no empty spot to click');
        await page.evaluate(() => { window.__results = window.__results.filter((r) => r.type !== 'order_move'); });
        await page.mouse.click(canvasBox.x + spot.p.x, canvasBox.y + spot.p.y);
        const moveRes = await waitFor('order_move result', () => page.evaluate(() => window.__results.find((r) => r.type === 'order_move')), 5000).catch(() => null);
        if (!moveRes) {
          console.log(`  … walk attempt ${attempt}: the click sent no order (it hit a marker that moved there?); retrying`);
          await page.evaluate((id) => window.evo.model.select(id), id);
          continue;
        }
        if (!moveRes.ok) {
          if (moveRes.error?.code === 'not_found' || moveRes.error?.code === 'not_owned') throw new Retry(`order_move: ${moveRes.error.code}`);
          throw new Error(`order_move failed: ${JSON.stringify(moveRes.error)}`);
        }
        // Measure against the target actually sent (the camera may have
        // shifted a little between picking the spot and the click).
        const target = { x: moveRes.x, y: moveRes.y };
        const d0 = Math.hypot(spot.from.x - target.x, spot.from.y - target.y);
        let best = d0;
        let lost = null;
        let maxGap = 0;
        let speed = null;
        const walk = await waitFor('organism walks toward the target', async () => {
          const o = await page.evaluate(({ id, target }) => {
            const o = window.evo.model.state.organisms.find((x) => x.id === id);
            // The drawn (predicted, smoothed) position against the server's.
            const m = o && window.evo.screen()?.view.markerWorldPos(id);
            const gap = m ? Math.hypot(m.x - o.x, m.y - o.y) : 0;
            return o ? { d: Math.hypot(o.x - target.x, o.y - target.y), action: o.action, possessed: o.possessed, speed: o.speed, gap } : null;
          }, { id, target });
          if (!o || !o.possessed) {
            lost = o ? 'released' : 'died';
            return { lost };
          }
          actions.add(o.action);
          best = Math.min(best, o.d);
          maxGap = Math.max(maxGap, o.gap);
          if (o.speed !== undefined) speed = o.speed;
          // Closer by a tile, or arrived (order dropped near the target).
          return best < d0 - 1 || best <= 1.0 ? { d0, best, action: o.action, attempt, maxGap, speed } : null;
        }, 20000).catch(() => null);
        if (lost) throw new Retry(`#${id} ${lost} while walking`);
        if (walk) return walk;
        console.log(`  … walk attempt ${attempt}: closest ${best.toFixed(1)} of ${d0.toFixed(1)} tiles (actions seen: ${[...actions].join(', ')}); re-issuing the order`);
      }
      throw new Error(`organism #${id} did not walk toward any of ${attempts} targets (actions seen: ${[...actions].join(', ')})`);
    }

    const tried = [];
    let walked = null;
    for (let round = 1; round <= 4 && !walked; round++) {
      try {
        const selected = await clickOwn(tried);
        tried.push(selected);
        if (!(await page.waitForSelector('#btn-possess', { timeout: 5000 }).then(() => true, () => false))) throw new Retry(`#${selected} has no possess button (died?)`);
        step(`clicked own organism #${selected}; inspector open`);
        await page.click('#btn-possess', { timeout: 5000 }).catch(() => { throw new Retry('possess button went away'); });
        const possessed = await waitFor('possession', () => page.evaluate((id) => window.evo.model.state.organisms.find((o) => o.id === id && o.possessed)?.id, selected), 10000)
          .catch(() => { throw new Retry(`#${selected} was not possessed`); });
        step(`possessed #${possessed}`);
        const walk = await walkTo(possessed);
        walked = { id: possessed, ...walk };
      } catch (e) {
        if (!(e instanceof Retry)) throw e;
        console.log(`  … round ${round}: ${e.message}; starting over with another organism`);
      }
    }
    if (!walked) throw new Error('could not possess and walk an organism in 4 rounds');
    step(`order_move ok; #${walked.id} walked from ${walked.d0.toFixed(1)} to ${walked.best.toFixed(1)} tiles of the target (${walked.action}, attempt ${walked.attempt})`);
    // Prediction leads by at most half a tick of movement and eases
    // corrections, snapping beyond RECONCILE_SNAP (3 tiles) of the
    // prediction: so the drawn marker stays within about 3 tiles plus the
    // lead of the server position. 4 allows for that lead on a slow runner.
    if (!(walked.speed > 0)) throw new Error(`the possessed organism's state has no speed (${walked.speed})`);
    if (walked.maxGap > 4) throw new Error(`drawn #${walked.id} was ${walked.maxGap.toFixed(2)} tiles from the server position`);
    step(`prediction: speed ${walked.speed.toFixed(2)} tiles/tick, drawn marker at most ${walked.maxGap.toFixed(2)} tiles from the server position`);

    // Group command and a server-side error surfaced as a toast.
    await page.click('#migrate');
    await page.mouse.click(canvasBox.x + 200, canvasBox.y + 600);
    const mig = await waitFor('migrate result', () => page.evaluate(() => window.__results.find((r) => r.type === 'migrate')));
    if (!mig.ok) throw new Error(`migrate failed: ${JSON.stringify(mig.error)}`);
    await page.evaluate(() => window.evo.socket.send({ type: 'possess', organism: 999999 }));
    await page.waitForSelector('.toast.error:has-text("999999")', { timeout: 5000 });
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
    // Per-gene histograms and the allele-origin card.
    const genetics = await page.evaluate(() => ({
      hist: document.querySelectorAll('#gene-table svg rect.hist-bar').length,
      origins: document.querySelectorAll('#allele-origins rect.origin-seg').length,
      originText: document.querySelector('#allele-origins .origin-list')?.textContent ?? '',
    }));
    if (genetics.hist < 30 || genetics.origins < 1 || !genetics.originText.includes('you')) {
      throw new Error(`gene histograms / allele origins: ${JSON.stringify(genetics)}`);
    }
    step(`gene histograms: ${genetics.hist} bars; allele origins: ${genetics.originText.trim()}`);
    // Alert preferences: hide daily summaries; the feed drops them and the
    // server keeps the choice.
    await page.click('#alert-prefs summary');
    await page.click('#alert-prefs input[data-kind="daily_summary"][data-field="show"]');
    await waitFor('daily summaries hidden', () => page.evaluate(() => document.querySelectorAll('#alerts-feed .feed-item.kind-daily_summary').length === 0), 5000);
    const savedPrefs = await waitFor('alert preferences saved', () => page.evaluate(async () => {
      const { token } = JSON.parse(localStorage.getItem('evo.session'));
      const p = await (await fetch('/api/alerts/prefs', { headers: { authorization: `Bearer ${token}` } })).json();
      return p.hidden.includes('daily_summary') ? p : null;
    }), 5000);
    step(`alert preferences saved: ${JSON.stringify(savedPrefs)}`);
    await page.screenshot({ path: path.join(shots, 'genetics-dashboard.png') });
    await page.$eval('#panel-genetics', (el) => { el.scrollTop = el.scrollHeight; });
    await page.waitForTimeout(200);
    await page.screenshot({ path: path.join(shots, 'genetics-dashboard-feed.png') });

    // World tab: species detected from the sampled adults, era clock, achievements.
    await page.click('#tab-world');
    const world = await waitFor('world chronicle', () => page.evaluate(() => {
      const species = document.querySelectorAll('#species-list li.species').length;
      const era = document.querySelector('#era-clock')?.textContent ?? '';
      const players = document.querySelectorAll('#achievement-table tbody tr').length;
      return species > 0 && era && players > 0 ? { species, era, players } : null;
    }), 15000);
    step(`world tab: ${world.species} species, ${world.players} player tallies; ${world.era.split('.')[0]}`);
    await page.screenshot({ path: path.join(shots, 'world-chronicle.png') });

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
