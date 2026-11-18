// End-to-end smoke test: start evo-server (fast, in memory) serving the built
// client, then in headless Chromium register, found a lineage, open the
// world view, check the tile map and organisms are drawn, run a directed
// mating with a second player of the same body plan (preview its
// compatibility, request, accept, escort until the two meet, and check it
// completes and conceives), possess an organism and walk it, open the
// genetics dashboard, the seasonal leaderboard, the World tab and the
// Seasons tab (ranked seasons; three days long here).
//
// The world keeps running throughout, so every step waits on explicit state
// conditions (generous timeouts, no fixed sleeps for correctness) and retries
// what the living world can legitimately interrupt. The page opens with
// `?tour=off`: e2e/onboarding.mjs tests the first-session tour.
//
//   npm run build && npm run e2e
//
// Env: EVO_SERVER_BIN (default ../target/release/evo-server; build it with
// `cargo build --release -p evo-server`), SCREENSHOT_DIR (default e2e/out),
// HEADED=1 to watch, SERVER_LOG_FILE to save the whole server log. Uses the
// `playwright` package from NODE_PATH or node_modules (it is not a
// dependency: browsers come pre-installed).

import { spawn } from 'node:child_process';
import { createRequire } from 'node:module';
import { existsSync, mkdirSync } from 'node:fs';
import { createServer } from 'node:net';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { cappedBrowser, FPS } from './frames.mjs';

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
/**
 * Choose organism `id` (null: the first pickable one) in a searchable picker
 * (picker.ts) the way a player does: open it, search the id, click the row.
 * False if it isn't offered within `timeoutMs`.
 */
async function choosePick(p, sel, id, timeoutMs) {
  const end = Date.now() + timeoutMs;
  while (Date.now() < end) {
    const done = await p.evaluate(({ sel, id }) => {
      const root = document.querySelector(sel);
      if (!root) return false;
      const panel = root.querySelector('.picker-panel');
      if (panel.hidden) root.querySelector('.picker-button').click();
      const search = root.querySelector('.picker-search');
      const q = id === null ? '' : String(id);
      if (search.value !== q) {
        search.value = q;
        search.dispatchEvent(new Event('input', { bubbles: true }));
      }
      const row = [...root.querySelectorAll('.picker-row')].find((b) => !b.disabled && (id === null ? /^#\d+ /.test(b.textContent) : b.textContent.startsWith(`#${id} `)));
      if (!row) return false;
      row.click();
      return root.value === String(id ?? root.value) && root.value !== '';
    }, { sel, id });
    if (done) return true;
    await new Promise((r) => setTimeout(r, 200));
  }
  await p.evaluate((sel) => { const r = document.querySelector(sel); if (r && !r.querySelector('.picker-panel').hidden) r.querySelector('.picker-button').click(); }, sel);
  return false;
}

function step(msg) {
  console.log(`  ✓ ${msg} [${((Date.now() - started) / 1000).toFixed(0)} s]`);
}

async function main() {
  if (!existsSync(bin)) throw new Error(`no server binary at ${bin}; run cargo build --release -p evo-server`);
  if (!existsSync(path.join(dist, 'index.html'))) throw new Error('no client/dist; run npm run build');
  mkdirSync(shots, { recursive: true });
  const port = await freePort();
  const base = `http://127.0.0.1:${port}`;
  // 120-day seasons (two minutes in fast mode): a season change expires open
  // mating offers, which would race the mating step on a slow machine.
  const server = spawn(bin, ['--fast', '--memory', '--size', '64', '--seed', '7', '--days-per-year', '480', '--season-days', '3', '--bind', `127.0.0.1:${port}`, '--static', dist], {
    stdio: ['ignore', 'pipe', 'pipe'],
  });
  let serverLog = '';
  server.stdout.on('data', (d) => (serverLog += d));
  server.stderr.on('data', (d) => (serverLog += d));
  const browser = cappedBrowser(await chromium.launch({ headless: !process.env.HEADED }));
  const pages = [];
  try {
    await waitFor('server', async () => (await fetch(`${base}/api/world`)).ok);
    step(`server up on ${base}`);
    const page = await browser.newPage({ viewport: { width: 1280, height: 800 }, deviceScaleFactor: 1 });
    const pageErrors = [];
    pages.push(page);
    page.on('pageerror', (e) => pageErrors.push(e.message));
    await page.goto(`${base}/?tour=off`);

    // Register.
    const name = `e2e_${Date.now().toString(36)}`;
    await page.fill('#login-name', name);
    await page.fill('#login-password', 'a long password');
    await page.click('#register-submit');
    await page.waitForSelector('#design-name', { timeout: 45000 });
    step('registered; design editor shown');

    // Design: the live cost follows the sliders and refuses an over-budget design.
    await page.fill('#design-name', 'Smoke striders');
    const setTrait = (gene, v) => page.$eval(`input[data-gene="${gene}"]`, (el, v) => {
      el.value = String(v);
      el.dispatchEvent(new Event('input'));
    }, v);
    const before = await page.textContent('#budget-total');
    // Where the sliders start (the starting design: some traits are above their defaults).
    const start = await page.$$eval('input[data-gene]', (els) => Object.fromEntries(els.map((e) => [e.dataset.gene, Number(e.value)])));
    await setTrait('body_size', 0.45);
    await setTrait('fur', 0.4);
    await setTrait('keen_nose', 0.3);
    const after = await page.textContent('#budget-total');
    if (before === after) throw new Error(`cost did not change (${before})`);
    // The starting cost plus each trait's change in its rise above its
    // default, times its price, from the server's catalog (prices are tuned
    // in the sim).
    const catalog = await (await fetch(`${base}/api/genes`)).json();
    const gene = (n) => catalog.genes.find((g) => g.name === n);
    const set = { body_size: 0.45, fur: 0.4, keen_nose: 0.3 };
    const rise = (n, v) => Math.max(0, v - gene(n).default) * gene(n).cost;
    const expected = Number.parseFloat(before) + Object.entries(set).reduce((sum, [n, v]) => sum + rise(n, v) - rise(n, start[n] ?? gene(n).default), 0);
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
    for (const g of maxed) await setTrait(g.name, start[g.name] ?? g.default);
    step(`live cost ${after.trim()}; over-budget design refused client-side`);
    // Trait captions: every slider has a live line saying what its setting
    // does, linked to it for screen readers, and it follows the rest of the
    // design (fur blocks basking on a cold-blooded body).
    const captions = await page.$$eval('input[data-gene]', (els) => els.map((el) => {
      const id = el.getAttribute('aria-describedby');
      return [el.dataset.gene, id ? document.getElementById(id)?.textContent ?? '' : ''];
    }));
    const bare = captions.filter(([, t]) => !t.trim());
    if (bare.length) throw new Error(`sliders without a caption: ${bare.map(([g]) => g).join(', ')}`);
    const furCaption = () => page.textContent('#trait-fur-caption');
    const warmFur = await furCaption();
    await page.check('input[name="thermo"][value="cold"]');
    if (!(await furCaption()).includes('blocks basking')) throw new Error(`cold-blooded fur caption: ${await furCaption()}`);
    await page.check('input[name="thermo"][value="warm"]');
    if ((await furCaption()) !== warmFur) throw new Error(`warm-blooded fur caption: ${await furCaption()}`);
    // The info button opens the full low / middle / high text.
    await page.click('[data-trait="hooves"] .trait-more');
    if (!(await page.isVisible('#trait-hooves-info'))) throw new Error('hooves info did not open');
    if (!(await page.textContent('#trait-hooves-info')).includes('Middle:')) throw new Error('hooves info has no levels');
    step(`${captions.length} sliders captioned; captions follow thermoregulation; info opens`);

    // Founder preview: a described image that redraws as the design changes.
    const avatar = () => page.$eval('#design-avatar', (el) => ({
      role: el.getAttribute('role'), label: el.getAttribute('aria-label') ?? '', svg: el.innerHTML,
      w: el.getBoundingClientRect().width, h: el.getBoundingClientRect().height,
    }));
    const a0 = await avatar();
    if (a0.role !== 'img' || !a0.label.startsWith('Founder preview:') || !a0.svg.startsWith('<svg')) throw new Error(`avatar missing: ${JSON.stringify({ ...a0, svg: a0.svg.slice(0, 80) })}`);
    if (a0.w < 200 || a0.h < 120) throw new Error(`avatar too small on desktop: ${a0.w}×${a0.h}`);
    const sideBox = await page.locator('.design-side').boundingBox();
    const budgetBox = await page.locator('.budget').boundingBox();
    const avatarBox = await page.locator('.avatar-card').boundingBox();
    if (!(avatarBox.y < budgetBox.y && avatarBox.x >= sideBox.x - 1)) throw new Error(`avatar not above the budget: ${JSON.stringify({ avatarBox, budgetBox })}`);
    const saved = await page.$$eval('input[data-gene]', (els) => els.map((el) => [el.dataset.gene, el.value]));
    // The design's thermoregulation and birth, restored with the sliders (the
    // second player founds the starting design: both must stay one body plan).
    const savedThermo = await page.$eval('input[name="thermo"]:checked', (el) => el.value);
    const savedBirth = await page.$eval('input[name="birth"]:checked', (el) => el.value);
    await setTrait('wings', 0);
    const noWings = await avatar();
    await setTrait('wings', 0.6);
    const winged = await avatar();
    if (winged.svg === noWings.svg) throw new Error('avatar did not change with wings 0 → 0.6');
    if (!winged.svg.includes('data-part="wing-near"') || noWings.svg.includes('data-part="wing-near"')) throw new Error('wings not drawn at 0.6');
    if (!winged.label.includes('wings')) throw new Error(`avatar label misses wings: ${winged.label}`);
    // The preview stays in view while the sliders scroll.
    await page.$eval('[data-trait="vigilance"]', (el) => el.scrollIntoView({ block: 'center' }));
    const stuck = await page.locator('#design-avatar').boundingBox();
    if (!stuck || stuck.y < 0 || stuck.y > 400) throw new Error(`avatar scrolled away: ${JSON.stringify(stuck)}`);
    await page.$eval('.design-body', (el) => { el.scrollTop = 0; });
    // Contrasting designs, for the screenshots.
    const designs = {
      'furry-herbivore': { thermo: 'warm', birth: 'live', traits: { body_size: 0.12, fur: 0.9, large_ears: 0.7, large_eyes: 0.6, tail: 0.35, leg_length: 0.3, carnivory: 0, seasonal_coat: 0.7 } },
      'armored-carnivore': { thermo: 'cold', birth: 'abandoned_eggs', traits: { body_size: 0.8, armor_plates: 0.85, scales: 0.4, fangs: 0.95, carnivory: 1, muscle: 0.6, tail: 0.7, claws: 0.7 } },
      'winged-toxic': { thermo: 'warm', birth: 'incubated_eggs', traits: { body_size: 0.08, wings: 0.8, coloration: 0.9, toxins: 0.8, tail: 0.5, leg_length: 0.5 } },
    };
    const geneNames = saved.map(([g]) => g);
    const labels = new Set();
    for (const [tag, d] of Object.entries(designs)) {
      await page.check(`input[name="thermo"][value="${d.thermo}"]`);
      await page.check(`input[name="birth"][value="${d.birth}"]`);
      for (const g of geneNames) await setTrait(g, d.traits[g] ?? (g === 'body_size' ? 0.3 : catalog.genes.find((x) => x.name === g).default));
      const a = await avatar();
      labels.add(a.label);
      await page.locator('.avatar-card').screenshot({ path: path.join(shots, `avatar-${tag}.png`) });
    }
    if (labels.size !== 3) throw new Error(`contrasting designs gave the same avatar description: ${[...labels].join(' | ')}`);
    // Back to the design whose cost was checked.
    await page.check(`input[name="thermo"][value="${savedThermo}"]`);
    await page.check(`input[name="birth"][value="${savedBirth}"]`);
    for (const [g, v] of saved) await setTrait(g, Number(v));
    if ((await page.textContent('#budget-total')) !== after) throw new Error(`design not restored: ${await page.textContent('#budget-total')} vs ${after}`);
    step(`avatar preview: described image above the budget, sticky, redraws (wings 0 → 0.6); ${labels.size} avatar screenshots`);

    // Simple / Builder: Simple by default; the builder edits the same design
    // as the sliders, the avatar and the vision readout follow, a third eye
    // and a third leg pair cost the catalog's structure prices, a preset's
    // cost change is asked first, and Simple mode's sliders show what the
    // builder set.
    const morphPrice = (n) => catalog.morph.find((r) => r.name === n).cost;
    const points = (t) => Number.parseFloat(t);
    const readout = () => page.$eval('#vision-readout', (el) => Object.fromEntries([...el.querySelectorAll('[data-stat]')].map((s) => [s.dataset.stat, `${s.querySelector('dd').textContent} ${s.querySelector('.vision-delta').textContent}`])));
    const sliderVal = (g) => page.$eval(`input[data-gene="${g}"]`, (el) => Number(el.value));
    if ((await page.getAttribute('#design-mode-simple', 'aria-pressed')) !== 'true' || !(await page.isHidden('#design-builder'))) throw new Error('editor did not start in Simple mode');
    const r0 = await readout();
    if (!Object.values(r0).every((t) => t.endsWith('='))) throw new Error(`default body's readout not at the default: ${JSON.stringify(r0)}`);
    await page.click('#design-mode-builder');
    await page.waitForSelector('#design-bb', { state: 'visible' });
    if ((await page.getAttribute('#design-mode-builder', 'aria-pressed')) !== 'true') throw new Error('builder mode not shown as pressed');
    if ((await page.evaluate(() => localStorage.getItem('evo.designMode'))) !== 'builder') throw new Error('builder mode not remembered');
    // A preset that changes sizes: the cost change shows first, then applies.
    const costBefore = await page.textContent('#budget-total');
    await page.click('#design-builder [data-preset="strider"]');
    await page.waitForSelector('#design-preset-confirm', { state: 'visible' });
    const ask = await page.textContent('#design-preset-confirm');
    if (!ask.includes('Strider') || !ask.includes('→')) throw new Error(`preset cost prompt: ${ask}`);
    if ((await page.textContent('#budget-total')) !== costBefore) throw new Error('preset applied before it was confirmed');
    await page.click('#design-preset-apply');
    await waitFor('strider legs on the slider', async () => (await sliderVal('leg_length')) === 0.9);
    const costStrider = await page.textContent('#budget-total');
    if (costStrider === costBefore) throw new Error(`preset with longer legs did not change the cost (${costStrider})`);
    if ((await sliderVal('tail')) !== 0.4) throw new Error(`preset tail not on the slider: ${await sliderVal('tail')}`);
    // A third eye: a new avatar and readout, one eye's price more.
    const two = await avatar();
    const r1 = await readout();
    await page.click('#design-bb-eye-plus');
    await waitFor('three eyes on the avatar', async () => (await avatar()).label.includes('three eyes'));
    const three = await avatar();
    const r2 = await readout();
    if (three.svg === two.svg) throw new Error('avatar not redrawn for a third eye');
    if (JSON.stringify(r2) === JSON.stringify(r1)) throw new Error(`readout did not change for a third eye: ${JSON.stringify(r2)}`);
    if (!r2.depth.includes('↑')) throw new Error(`a midline eye should add depth vision: ${JSON.stringify(r2)}`);
    if ((await page.textContent('#design-bb-eye-count')) !== '3') throw new Error('builder eye count not 3');
    const costThree = await page.textContent('#budget-total');
    if (Math.abs(points(costThree) - points(costStrider) - morphPrice('eye_count')) > 0.051) throw new Error(`a third eye should cost ${morphPrice('eye_count')}: ${costThree} vs ${costStrider}`);
    // Six legs: one leg pair's price more.
    await page.click('#design-builder [data-group="legs"][data-value="6"]');
    await waitFor('six legs on the avatar', async () => (await avatar()).label.includes('six legs'));
    const costSix = await page.textContent('#budget-total');
    if (Math.abs(points(costSix) - points(costThree) - morphPrice('leg_count')) > 0.051) throw new Error(`six legs should cost ${morphPrice('leg_count')} more: ${costSix} vs ${costThree}`);
    const bodyPlan = morphPrice('eye_count') + morphPrice('leg_count');
    if (!(await page.textContent('#trait-large_eyes-caption')).includes('three eyes')) throw new Error(`eye caption ignores the eye count: ${await page.textContent('#trait-large_eyes-caption')}`);
    await page.locator('.avatar-card').screenshot({ path: path.join(shots, 'avatar-builder-three-eyes.png') });
    await page.$eval('#design-builder', (el) => el.scrollIntoView({ block: 'start' }));
    await page.screenshot({ path: path.join(shots, 'editor-builder-desktop.png') });
    // A slider moved in Builder mode: the builder's leg handle follows.
    await page.$eval('input[data-gene="leg_length"]', (el) => { el.value = '0.5'; el.dispatchEvent(new Event('input')); el.dispatchEvent(new Event('change')); });
    const legHandle = await page.getAttribute('#design-bb-h-legs', 'aria-valuenow');
    if (Number(legHandle) !== 0.5) throw new Error(`builder leg handle did not follow the slider: ${legHandle}`);
    // Back to Simple: the sliders and the avatar keep what the builder set.
    await page.click('#design-mode-simple');
    if (!(await page.isHidden('#design-builder'))) throw new Error('builder still shown in Simple mode');
    if ((await sliderVal('tail')) !== 0.4 || (await sliderVal('leg_length')) !== 0.5) throw new Error('Simple sliders lost the builder values');
    if (!(await avatar()).label.includes('three eyes')) throw new Error('Simple mode avatar lost the third eye');
    await page.screenshot({ path: path.join(shots, 'editor-builder-back-to-simple.png') });
    // Restore the checked traits (the third eye and the six legs stay, at their price).
    for (const [g, v] of saved) await setTrait(g, Number(v));
    const restored = await page.textContent('#budget-total');
    if (Math.abs(points(restored) - points(after) - bodyPlan) > 0.051) throw new Error(`design not restored after the builder: ${restored} vs ${after} + ${bodyPlan}`);
    step(`builder mode: preset cost ${costBefore.trim()} → ${costStrider.trim()} asked first; third eye +${morphPrice('eye_count')}, six legs +${morphPrice('leg_count')}; avatar and readout (${r2.depth}) follow; sliders in step both ways`);
    await page.screenshot({ path: path.join(shots, 'design-editor.png') });
    await page.$eval('[data-trait="leg_length"]', (el) => el.scrollIntoView({ block: 'start' }));
    await page.screenshot({ path: path.join(shots, 'design-editor-traits.png') });
    await page.$eval('.design-body', (el) => { el.scrollTop = 0; });
    const [founded] = await Promise.all([
      page.waitForResponse((r) => new URL(r.url()).pathname === '/api/lineage'),
      page.click('#design-submit'),
    ]);
    const body = await founded.json();
    if (founded.status() !== 201 || Math.abs(body.cost - expected - bodyPlan) > 0.06) throw new Error(`founding: ${founded.status()} ${JSON.stringify(body)} (expected ${expected + bodyPlan})`);
    // The body plan goes as `morph` only to a server whose /api/genes lists it.
    const sent = founded.request().postDataJSON();
    if ('morphology' in sent) throw new Error('the editor-only morphology was sent');
    // This server takes it (the morphology section is part of /api/genes).
    if (!Array.isArray(catalog.morph) || !catalog.morph.length) throw new Error('the server does not advertise morph');
    if (sent.morph?.eye_count !== 3 || sent.morph?.leg_count !== 6) throw new Error(`server takes morph, but the design sent ${JSON.stringify(sent.morph)}`);
    step(`founded a three-eyed, six-legged lineage ${body.lineage} with ${body.founders.length} founders; server cost ${body.cost.toFixed(2)} matches the client`);

    // World view with organisms.
    await page.waitForSelector('#world-canvas', { timeout: 45000 });
    await waitFor('own organisms in state', () => page.evaluate(() => (window.evo?.model.state?.organisms.length ?? 0) > 0));
    await waitFor('markers drawn', () => page.evaluate(() => {
      const v = window.evo.screen()?.view;
      if (!v || v.markers.size === 0 || !v.terrain) return false;
      const t = v.terrainStats();
      return t.width === v.terrain.width && t.baseSize[0] === v.terrain.width * t.baseTpx;
    }));
    const counts = await page.evaluate(() => ({ own: window.evo.model.state.organisms.length, nearby: window.evo.model.state.nearby.length, markers: window.evo.screen().view.markers.size }));
    step(`world view: ${counts.own} own + ${counts.nearby} nearby organisms, ${counts.markers} markers`);
    // States arrive as protocol 3 binary deltas (with own headings and eyes), rebuilt by the socket.
    const proto = await page.evaluate(() => window.evo.model.proto);
    if (proto !== 3) throw new Error(`the connection speaks protocol ${proto}, not 3`);
    // The third eye founded into the genome: every own organism looks
    // three-eyed (the server reads the expressed morphology), and so does a
    // near sprite's texture key (`creatureKey`: `-<eyes>…` after the class).
    const looks = await page.evaluate(() => window.evo.model.state.organisms.map((o) => o.look?.eyes));
    if (!looks.length || !looks.every((e) => e === 3)) throw new Error(`own organisms do not look three-eyed: ${JSON.stringify(looks)}`);
    const keys = await page.evaluate(() => {
      const v = window.evo.screen().view;
      const ids = window.evo.model.state.organisms.map((o) => o.id);
      return { far: v.spriteStats().far, keys: ids.map((id) => v.markerInfo(id)?.key).filter(Boolean) };
    });
    if (!keys.far && !keys.keys.some((k) => /^[^-]*-3/.test(k))) throw new Error(`no three-eyed sprite among own markers: ${JSON.stringify(keys)}`);
    step(`the third eye shows: ${looks.length} own organisms look three-eyed${keys.far ? '' : `; sprite ${keys.keys[0]}`}`);

    // The family tree (Family tab, `GET /api/lineage/tree`): the founding
    // stage at least, drawn with its portrait, three-eyed and six-legged.
    await page.click('#tab-family');
    const tree = await waitFor('the family tree', () => page.evaluate(() => {
      const nodes = [...document.querySelectorAll('#panel-family .ft-node')];
      if (!nodes.length) return null;
      return {
        nodes: nodes.length,
        kinds: nodes.map((n) => n.dataset.kind),
        portraits: document.querySelectorAll('#panel-family .ft-portrait svg').length,
        label: nodes[0].querySelector('.ft-portrait').getAttribute('aria-label'),
        summary: document.querySelector('#family-summary')?.textContent ?? '',
      };
    }), 20000);
    if (tree.kinds[0] !== 'founding' || tree.portraits !== tree.nodes || !/three eyes/.test(tree.label) || !/six legs/.test(tree.label)) {
      throw new Error(`family tree: ${JSON.stringify(tree)}`);
    }
    await page.locator('#sidebar').screenshot({ path: path.join(shots, 'family-tree-live.png') });
    step(`family tree: ${tree.nodes} stage(s) (${tree.kinds.join(', ')}), portrait "${tree.label.slice(0, 60)}…"`);
    await page.click('#tab-lineage');

    // Tile map: the terrain grid from /api/terrain, painted (blended and
    // textured) into a base texture of a few texels per tile, with detail
    // chunks over the part on screen (STYLE.md).
    const tiles = await page.evaluate(() => {
      const v = window.evo.screen().view;
      const t = v.terrain;
      const buf = v.terrainLayer.baseBuffer();
      const st = v.terrainStats();
      const colors = new Set();
      for (let i = 0; i < buf.length; i += 4 * 7) colors.add((buf[i] << 16) | (buf[i + 1] << 8) | buf[i + 2]);
      return { w: t.width, h: t.height, tpx: st.baseTpx, tex: st.baseSize, biomes: new Set(t.biome).size, colors: colors.size, tick: t.tick, baseMs: st.baseMs };
    });
    if (tiles.w !== 64 || tiles.h !== 64 || tiles.tex[0] !== 64 * tiles.tpx || tiles.tpx < 2 || tiles.biomes < 3 || tiles.colors < 200) throw new Error(`tile map: ${JSON.stringify(tiles)}`);
    await waitFor('a dynamic terrain refresh', () => page.evaluate((t0) => window.evo.screen().view.terrain.tick > t0, tiles.tick), 30000);
    if (!(await page.textContent('#terrain-legend')).includes('drought')) throw new Error('no terrain legend');
    const detail = await waitFor('detail chunks on screen', () => page.evaluate(() => { const t = window.evo.screen().view.terrainStats(); return t.detail === 0 || t.shown > 0 ? t : null; }), 20000);
    step(`tile map: ${tiles.w}×${tiles.h} tiles, ${tiles.biomes} biomes, painted at ${tiles.tpx} texels/tile (${tiles.colors} colors sampled, ${tiles.baseMs.toFixed(1)} ms); detail ${detail.detail} texels/tile, ${detail.shown} chunks shown, slowest frame's painting ${detail.maxFrameMs.toFixed(1)} ms; dynamic layers refreshed`);
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
    // the same body plan and preferences as ours, so the pair is fertile
    // (partly: egg-layers of different lineages, see below) and inside both preferences (no override cost, no failure
    // chance): the mating completes and conceives once the two meet. This
    // runs early, while both lineages are young: later on a loaded machine
    // many more world days pass and a lineage can dwindle.
    const bob = await browser.newPage({ viewport: { width: 1280, height: 800 }, deviceScaleFactor: 1 });
    pages.push(bob);
    bob.on('pageerror', (e) => pageErrors.push(`bob: ${e.message}`));
    await bob.goto(`${base}/?tour=off`);
    await bob.fill('#login-name', `${name}_b`);
    await bob.fill('#login-password', 'a long password');
    await bob.click('#register-submit');
    await bob.waitForSelector('#design-name', { timeout: 45000 });
    await bob.fill('#design-name', 'Bob striders');
    for (const [g, v] of Object.entries(set)) {
      await bob.$eval(`input[data-gene="${g}"]`, (el, v) => {
        el.value = String(v);
        el.dispatchEvent(new Event('input'));
      }, v);
    }
    await bob.click('#design-submit');
    await bob.waitForSelector('#world-canvas', { timeout: 45000 });
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
        // Our female with bob's male: our own female is held from the moment
        // we ask (our consent), bob's only once he accepts, and males never
        // become pregnant. But an open female in fast mode conceives within
        // seconds, and on a loaded machine a few Playwright round trips take
        // that long: so she is picked from ann's live state in the same page
        // task that fills the form and clicks Send (nothing can change in
        // between), against bob's males (which only a death can spoil).
        // With no such pair for a while, ann's male and bob's open female
        // (picked from bob's live state right before the request).
        let seen = '';
        const since = Date.now();
        const cand = await waitFor('a pair of adults', async () => {
          const adults = (p) => p.evaluate(() => window.evo.model.state.organisms.filter((o) => o.matured && !o.pregnant && !o.possessed)
            .map((o) => ({ id: o.id, sex: o.sex, urge: o.needs.breeding, action: o.action, x: o.x, y: o.y })));
          const b = await adults(bob);
          const bobMales = b.filter((o) => o.sex === 'male');
          const late = Date.now() - since > 20000;
          // Prefer the closest pair: predators pick off organisms on a long
          // trip to the meeting. Then the least breeding urge in the female,
          // and not one courting now: a courting female tends to conceive
          // with one of her own before the request lands.
          const sent = await page.evaluate(({ males, females, late }) => {
            const sel = document.querySelector('#mating-mine');
            const mine = window.evo.model.state.organisms.filter((o) => o.matured && !o.pregnant && !o.possessed);
            // (A female's breeding urge, and whether she is seeking a mate or courting now.)
            const score = (f, m, urge) => Math.hypot(f.x - m.x, f.y - m.y) + urge / 20 + (/court|mat/.test(f.action) ? 5 : 0);
            let best = null;
            for (const f of mine.filter((o) => o.sex === 'female')) {
              for (const m of males) {
                const s = score(f, m, f.needs.breeding);
                if (!best || s < best.score) best = { mine: f.id, partner: m.id, score: s };
              }
            }
            if (!best && late) {
              for (const m of mine.filter((o) => o.sex === 'male')) {
                for (const f of females) {
                  const s = score(f, m, f.urge);
                  if (!best || s < best.score) best = { mine: m.id, partner: f.id, score: s };
                }
              }
            }
            if (!best) return { seen: `ann ${mine.filter((o) => o.sex === 'female').length}F/${mine.filter((o) => o.sex === 'male').length}M` };
            sel.value = String(best.mine);
            sel.dispatchEvent(new Event('change', { bubbles: true }));
            const set = (q, v) => {
              const el = document.querySelector(q);
              el.value = v;
              el.dispatchEvent(new Event('input', { bubbles: true }));
            };
            set('#mating-partner', String(best.partner));
            set('#mating-price', '25');
            if (sel.value !== String(best.mine)) return { seen: 'the form did not take the pick' };
            window.__results = [];
            document.querySelector('#mating-submit').click();
            return { sent: best };
          }, { males: bobMales, females: b.filter((o) => o.sex === 'female'), late });
          seen = `${sent.seen ?? ''} open adults, bob ${b.length - bobMales.length}F/${bobMales.length}M open adults`;
          return sent.sent ?? null;
          // (In fast mode every adult female can be pregnant for a while.)
        }, 120000).catch((e) => { throw new Error(`${e.message}; ${seen}`); });
        const req = await waitFor('request_mating result', () => page.evaluate(() => window.__results.find((r) => r.type === 'request_mating') ?? null));
        // If our pick died before the click, the form falls back to another
        // adult (which can be the partner's sex): retry then, too.
        const swapped = req.mine !== cand.mine || req.partner !== cand.partner;
        if (!req.ok) {
          if (!swapped && !['already_pregnant', 'not_mature', 'not_found', 'died', 'out_of_sight'].includes(req.error?.code)) throw new Error(`request_mating failed: ${JSON.stringify(req.error)} for ${JSON.stringify(cand)}`);
          // A request names only an animal in sight, on the market or in an
          // offer: the lineages can be founded far apart, so bob lists it.
          if (req.error?.code === 'out_of_sight') {
            await bob.evaluate((id) => window.evo.socket.send({ type: 'create_listing', organism: id, fee: 0 }), req.partner);
          }
          const server = (await (await fetch(`${base}/api/world`)).json()).clock.tick;
          const look = (p, id) => p.evaluate((id) => { const s = window.evo.model.state; const o = s.organisms.find((o) => o.id === id); return `${s.clock.tick}:${o ? `${o.sex[0]}${o.pregnant ? 'P' : ''}${o.matured ? 'A' : ''}` : '-'}`; }, id);
          const diag = `server ${server}, ann ${await look(page, req.mine)}, bob ${await look(bob, req.partner)}`;
          console.log(`  … request_mating refused (${req.error?.code}${swapped ? `, form sent #${req.mine} × #${req.partner}` : ''}; ${diag}); picking again`);
          await page.waitForTimeout(500);
          continue;
        }
        const id = req.data.offer.id;
        // Bob's page accepts it as soon as the request shows (see above).
        await waitFor('bob\'s answer', async () => {
          const o = await offerOf(bob, id);
          return o && o.status !== 'pending';
        }, 30000);
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
        }, 20000);
        // (A preview error means one of them just died; the offer then fails.
        // Under load the agreed pair can also meet and conceive before the
        // preview is read: the preview then says she is already pregnant.)
        if (!compat.startsWith('(') && !compat.includes('already pregnant') && !compat.startsWith("Can't preview") && !compat.startsWith('Compatible: fertility')) throw new Error(`same designs not shown compatible: ${compat}`);
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
        // On a loaded machine the button can show late or be rebuilt under
        // the click: click it (a plain DOM click, found afresh) until our
        // organism is possessed or the agreement closes. Escort is
        // idempotent (it only possesses an organism not yet possessed).
        let lastClick = 0;
        const esc = await waitFor('escort possession', async () => {
          if ((await offerOf(page, id))?.status !== 'accepted') return 'closed';
          if (await page.evaluate((id) => window.evo.model.state.organisms.find((o) => o.id === id)?.possessed, pair.mine)) return 'possessed';
          if (Date.now() - lastClick > 2000) {
            const clicked = await page.evaluate((id) => {
              const b = [...document.querySelectorAll(`#open-offers .offer[data-offer="${id}"] button`)].find((b) => b.textContent.startsWith('Escort'));
              b?.click();
              return !!b;
            }, id);
            if (clicked) lastClick = Date.now();
          }
          return null;
        }, 30000);
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
      // The starter lays eggs (balance round 4), and egg-layers of different
      // lineages are only partly fertile (egg_hybrid), so two players' starter
      // animals give about 0.25: fertile, not fully.
      if (!(o.fertility > 0.15)) throw new Error(`same designs not fertile: ${JSON.stringify(o)}`);
      // Inside both preferences there is no override failure chance, so a
      // fully fertile pair always conceives (a partly fertile one may not).
      if (o.cost === 0 && o.fertility > 0.95 && !o.conceived) throw new Error(`no conception at no cost: ${JSON.stringify(o)}`);
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
    const ledgerOf = async (p) => {
      const r = await p.evaluate(() => window.evo.socket.send({ type: 'ledger', limit: 100 }));
      if (!r.ok) throw new Error(`ledger: ${JSON.stringify(r.error)}`);
      return r.data;
    };
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
      if (!(await choosePick(page, '#market-list-organism', cand.dam, 10000))) continue;
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
      await page.waitForSelector(`#my-listings .listing[data-listing="${listingId}"]`, { timeout: 20000 });
      // Bob picks his male; the listing shows with its compatibility.
      if (!(await choosePick(bob, '#market-mine', cand.buyer, 10000))) continue;
      await bob.click('#market-refresh');
      const card = `#market-listings .listing[data-listing="${listingId}"]`;
      const compat = await waitFor('listing with compatibility', () => bob.evaluate((card) => document.querySelector(`${card} .compat`)?.textContent ?? null, card), 20000).catch(() => null);
      if (!compat) {
        console.log('  … the listing showed no compatibility (an organism died?); picking again');
        continue;
      }
      if (!compat.startsWith('Compatible: fertility')) throw new Error(`same designs not shown compatible in the market: ${compat}`);
      // Wait until the buy button is enabled: a pregnant dam first gives birth.
      const ready = await waitFor('the dam to be available', async () => {
        if (await bob.isEnabled(`${card} button.buy`).catch(() => false)) return 'ready';
        // Why it can't be bought: the card's "Can't buy: …" line (older cards: a muted note in the row).
        const why = (await bob.evaluate((card) => {
          const c = document.querySelector(card);
          if (!c) return null;
          const w = c.querySelector('.why-not:not([hidden])') ?? c.querySelector('.row .muted');
          return w?.textContent ?? 'no reason shown';
        }, card).catch(() => null)) ?? 'listing gone';
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
    await waitFor('ann told of the sale', () => page.evaluate(() => window.evo.model.feed.some((e) => e.kind === 'market' && e.text.includes('bought a mating'))), 20000);
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
      // The seller gets the fee less the 10% market fee (floored; economy::market_fee).
      const net = fee - Math.floor((fee * 10 + 50) / 100); // the fee rounds to the nearest renown
      if (entry(annLedger, 'sale')?.delta !== net) throw new Error(`completed but ann not paid ${net}: ${JSON.stringify(annLedger.entries.slice(0, 5))}`);
      if (entry(bobLedger, 'refund')) throw new Error('completed and refunded');
      step(`market mating completed (${fin.outcome?.conceived ? 'conceived' : 'not conceived'}): ann paid ${fee - Math.floor((fee * 10) / 100)} of ${fee} renown from escrow`);
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
    // Ann withdraws the listing. The world may have closed it first (the
    // dam died: the server closes her listing and alerts ann); then ann
    // lists another adult and withdraws that one.
    const ownListings = () => page.evaluate(async () => {
      // A page of other players' listings is enough to ask for: `own` has all of ann's.
      const r = await window.evo.socket.send({ type: 'browse_market', limit: 1, hide_own: true, with_own: true });
      if (!r.ok) throw new Error(`browse_market: ${JSON.stringify(r.error)}`);
      if (r.data.listings.some((l) => l.owner === window.evo.model.player)) throw new Error('hide_own showed an own listing');
      return r.data.own.map((l) => l.id);
    });
    // Withdraw from the card in "My listings" (from `with_own`, whatever
    // page the market is on). The world keeps running: if the listing
    // closed (only a death closes it: it has no expiry), ann lists another
    // adult; if the card doesn't show, check the listing again and, on later
    // attempts, reopen the Market tab (a fresh render) before refreshing.
    let withdrawId = listingId;
    let withdrawOrg = bought.dam;
    let withdrawn = false;
    const relist = async () => {
      const dead = await waitFor(`#${withdrawOrg} gone from ann's state`, () => page.evaluate((id) => !window.evo.model.state.organisms.some((o) => o.id === id), withdrawOrg), 15000).catch(() => false);
      if (!dead) throw new Error(`listing ${withdrawId} of #${withdrawOrg} closed though she is alive`);
      console.log(`  … listing ${withdrawId} closed: #${withdrawOrg} died; listing another adult to withdraw`);
      await page.evaluate(() => { window.__results = []; });
      await page.click('#market-refresh');
      await page.selectOption('#market-list-kind', 'fixed');
      await choosePick(page, '#market-list-organism', null, 10000);
      await page.click('#market-list-submit');
      const relisted = await waitFor('create_listing result', () => page.evaluate(() => window.__results.find((r) => r.type === 'create_listing')));
      if (!relisted.ok && !['not_found', 'not_mature', 'already_listed'].includes(relisted.error?.code)) throw new Error(`create_listing failed: ${JSON.stringify(relisted.error)}`);
      if (relisted.ok) [withdrawId, withdrawOrg] = [relisted.data.listing.id, relisted.data.listing.organism];
    };
    for (let attempt = 0; attempt < 8 && !withdrawn; attempt++) {
      if (!(await ownListings()).includes(withdrawId)) {
        await relist();
        continue;
      }
      if (attempt > 0) {
        await page.click('#tab-matings');
        await page.click('#tab-market');
      }
      await page.click('#market-refresh');
      const card = `#my-listings .listing[data-listing="${withdrawId}"] button.withdraw`;
      if (!(await page.waitForSelector(card, { timeout: 15000 }).then(() => true, () => false))) {
        console.log(`  … the card of listing ${withdrawId} did not show; checking the listing again`);
        continue;
      }
      await page.evaluate(() => { window.__results = []; });
      if (!(await page.click(card, { timeout: 10000 }).then(() => true, () => false))) continue;
      const r = await waitFor('withdraw_listing result', () => page.evaluate(() => window.__results.find((r) => r.type === 'withdraw_listing')), 20000);
      if (!r.ok) {
        // Closed between the check and the click (the dam died).
        if (r.error?.code !== 'unknown_listing') throw new Error(`withdraw_listing failed: ${JSON.stringify(r.error)}`);
        continue;
      }
      withdrawn = true;
    }
    if (!withdrawn) throw new Error(`could not withdraw listing ${withdrawId} from its card`);
    await waitFor('listing withdrawn', () => page.evaluate((id) => !document.querySelector(`#my-listings .listing[data-listing="${id}"]`), withdrawId), 20000);
    if ((await ownListings()).includes(withdrawId)) throw new Error(`listing ${withdrawId} still open after withdrawing it`);
    step(`ledgers agree; balance pill "${pill.trim()}"; ann withdrew listing ${withdrawId}`);

    // An auction: ann auctions an adult from the Market tab, bob finds it
    // under "auctions" and bids the reserve through its card for one of his
    // adults of the other sex (held in escrow; the auction can then no
    // longer be withdrawn). The world runs fast (a simulated day is about a
    // second), so the auction gets the longest run, 30 days: about 30 s of
    // wall time, which a loaded machine can spend before bob's bid lands.
    //
    // So that the bid doesn't race the living world, the lot is preferably
    // one of ann's open females (a listed female is held from breeding) and
    // bob bids with a male (males never become pregnant): then nothing the
    // bid checks can change but by a death. In fast mode most adult females
    // are pregnant for a while: if none is open for a while, the lot is a
    // male (never pregnant) and bob bids with an open female. The lot is
    // picked from the live state right before the request; a refusal for
    // what the world changed meanwhile picks again. The button's state is
    // checked and the click made in one step in the page, and a bid that
    // can't be made or is refused is retried with another bidder. If the
    // auction closes before a bid lands (its deadline passed on a slow
    // machine, or the lot died), the round starts over with a new auction.
    // Only if every bid attempt fails on an auction that is still open does
    // ann withdraw it instead (logged as such).
    const reserve = 5; // a listing goes for at least 5 (LISTING_MIN_FEE)
    await page.selectOption('#market-list-kind', 'auction');
    if (!(await page.isVisible('#market-auction-days'))) throw new Error('the auction length field is hidden');
    const auctionOpen = async (id) => (await ownListings()).includes(id);
    // Create one auction from the form; returns { id, sex, organism }.
    const lotTried = [];
    const createLog = [];
    async function createAuction() {
      for (let attempt = 1; attempt <= 12; attempt++) {
        await page.click('#market-refresh');
        await page.fill('#market-fee', String(reserve));
        await page.fill('#market-auction-days', '30');
        // Wait for an adult that can be auctioned now, on the live state and
        // the form's options: an open female for the first 20 s, then a male
        // too; not tried before if possible. She is picked, chosen in the
        // form and Auction clicked in one page task, so she can't conceive
        // between the pick and the request (in fast mode an open female does
        // within seconds, about as long as a few round trips take on a
        // loaded machine).
        const since = Date.now();
        const lot = await waitFor('an adult of ann\'s to auction', () => page.evaluate(({ tried, males }) => {
          const sel = document.querySelector('#market-list-organism');
          const listed = new Set([...document.querySelectorAll('#my-listings .listing')].map((c) => c.dataset.organism));
          const open = window.evo.model.state.organisms
            .filter((o) => o.matured && !o.possessed && !listed.has(String(o.id)) && (o.sex === 'female' ? !o.pregnant : males))
            .sort((a, b) => (a.sex === 'female' ? 0 : 1) - (b.sex === 'female' ? 0 : 1) || b.health - a.health)
            .map((o) => String(o.id));
          const pick = open.find((id) => !tried.includes(id)) ?? open[0];
          if (!pick) return null;
          sel.value = pick;
          sel.dispatchEvent(new Event('change', { bubbles: true }));
          if (sel.value !== pick) return null;
          window.__results = [];
          document.querySelector('#market-list-submit').click();
          return pick;
        }, { tried: lotTried, males: Date.now() - since > 20000 }), 90000);
        lotTried.push(lot);
        const r = await waitFor('create_auction result', () => page.evaluate(() => window.__results.find((r) => r.type === 'create_auction') ?? null), 30000);
        if (r.ok) return { id: r.data.listing.id, sex: r.data.listing.sex, organism: r.data.listing.organism };
        if (!['already_pregnant', 'not_found', 'not_mature', 'already_listed', 'organism_busy'].includes(r.error?.code)) throw new Error(`create_auction failed: ${JSON.stringify(r.error)}`);
        const server = (await (await fetch(`${base}/api/world`)).json()).clock.tick;
        const diag = await page.evaluate((id) => { const s = window.evo.model.state; const o = s.organisms.find((o) => String(o.id) === id); return `${s.clock.tick}:${o ? `${o.sex[0]}${o.pregnant ? 'P' : ''}${o.matured ? 'A' : ''}` : '-'}`; }, lot);
        createLog.push(`#${lot} refused (${r.error?.code}; server ${server}, ann ${diag})`);
        console.log(`  … create_auction refused (${createLog.at(-1)}); picking again`);
      }
      throw new Error(`no auction could be created (${createLog.join('; ')})`);
    }
    // Bob's candidates: open adults of the other sex, males first (never
    // pregnant), then the healthiest (the least likely to die meanwhile).
    const bidders = (lotSex) => bob.evaluate((sex) => window.evo.model.state.organisms
      .filter((o) => o.matured && !o.possessed && !o.pregnant && o.sex !== sex)
      .sort((a, b) => (a.sex === 'male' ? 0 : 1) - (b.sex === 'male' ? 0 : 1) || b.health - a.health)
      .map((o) => o.id), lotSex);
    // What the living world can do to a bidder between the pick and the
    // server's check: those refusals are retried with another bidder. A
    // closed auction (`unknown_listing`, `auction_closed`) starts a new round.
    const retryBid = ['already_pregnant', 'not_found', 'not_mature', 'died', 'organism_busy'];
    const closedBid = ['unknown_listing', 'auction_closed'];
    const bidLog = [];
    let auctionId = null;
    let lotSex = null;
    let lotOrg = null;
    let bid = null;
    let bidder = null;
    let withdrawnAuction = false;
    const auctionRounds = 4;
    for (let round = 1; round <= auctionRounds && !bid?.ok && !withdrawnAuction; round++) {
      if (round > 1) await page.selectOption('#market-list-kind', 'auction');
      ({ id: auctionId, sex: lotSex, organism: lotOrg } = await createAuction());
      const opened = Date.now();
      bid = null;
      const closed = (why) => {
        bidLog.push(`round ${round}: auction ${auctionId} of #${lotOrg} closed ${why}, ${((Date.now() - opened) / 1000).toFixed(0)} s after it opened`);
        console.log(`  … auction ${auctionId} closed ${why} (${((Date.now() - opened) / 1000).toFixed(0)} s after it opened); auctioning again (round ${round}/${auctionRounds})`);
      };
      const ownCard = await waitFor('the auction in ann\'s listings', async () => {
        if (await page.$(`#my-listings .listing[data-listing="${auctionId}"][data-kind="auction"] .auction-line`)) return 'shown';
        if (!(await auctionOpen(auctionId))) return 'closed';
        await page.click('#market-refresh');
        await new Promise((r) => setTimeout(r, 1000));
        return null;
      }, 45000);
      if (ownCard === 'closed') {
        closed('before it showed in ann\'s listings');
        continue;
      }
      await bob.selectOption('#market-kind', 'auction');
      const bobCard = `#market-listings .listing[data-listing="${auctionId}"]`;
      // Bob's market is fetched on demand: refresh it while waiting.
      const line = await waitFor('the auction in bob\'s market', async () => {
        const t = await bob.evaluate((c) => document.querySelector(`${c} .auction-line`)?.textContent ?? null, bobCard);
        if (t) return t;
        if (!(await auctionOpen(auctionId))) return 'closed';
        await bob.click('#market-refresh');
        await new Promise((r) => setTimeout(r, 1000));
        return null;
      }, 45000);
      if (line === 'closed') {
        closed('before bob saw it');
        continue;
      }
      if (!line.includes(`reserve ${reserve}, no bids yet`)) throw new Error(`auction line: ${line}`);
      const kinds = await bob.$$eval('#market-listings .listing', (ls) => ls.map((l) => l.dataset.kind));
      if (kinds.some((k) => k !== 'auction')) throw new Error(`the auctions filter showed ${kinds}`);
      const triedBidders = [];
      let gone = false;
      for (let attempt = 1; attempt <= 6 && !bid?.ok && !gone; attempt++) {
        const cands = await waitFor(`one of bob's adults to bid with (attempt ${attempt})`, async () => {
          const l = await bidders(lotSex);
          return l.length ? l : null;
        }, 45000).catch(() => []);
        if (cands.length === 0) {
          bidLog.push(`round ${round} attempt ${attempt}: bob has no open adult of the other sex`);
          break;
        }
        const cand = cands.find((id) => !triedBidders.includes(id)) ?? cands[0];
        triedBidders.push(cand);
        bid = null;
        if (!(await choosePick(bob, '#market-mine', cand, 15000))) {
          bidLog.push(`round ${round} attempt ${attempt}: #${cand} not in bob's list`);
          continue;
        }
        // The card is rebuilt for the new pick: wait for its Bid button to be
        // enabled for it (refreshing bob's market meanwhile).
        let lastRefresh = Date.now();
        const ready = await waitFor(`the bid button enabled for #${cand}`, async () => {
          const s = await bob.evaluate(({ c, id }) => {
            const sel = document.querySelector('#market-mine');
            if (sel?.value !== String(id)) {
              // Still one of bob's adults (the options were rebuilt under
              // the pick): pick it again and keep waiting.
              if (window.evo.model.state?.organisms.some((x) => x.id === id && x.matured)) {
                sel.value = String(id);
                sel.dispatchEvent(new Event('change', { bubbles: true }));
                return null;
              }
              const o = window.evo.model.state?.organisms.find((o) => o.id === id);
              return `the pick changed to "${sel?.value}" (#${id} ${o ? `${o.matured ? 'adult' : 'juvenile'}${o.possessed ? ', possessed' : ''}` : 'not in the state'}, no longer in the list)`;
            }
            const b = document.querySelector(`${c} button.bid`);
            return b && !b.disabled ? 'ready' : null;
          }, { c: bobCard, id: cand });
          if (s) return s;
          if (Date.now() - lastRefresh > 3000) {
            if (!(await auctionOpen(auctionId))) return 'closed';
            await bob.click('#market-refresh');
            lastRefresh = Date.now();
          }
          return null;
        }, 30000).catch(() => null);
        if (ready === 'closed') {
          gone = true;
          break;
        }
        if (ready !== 'ready') {
          const why = ready ?? await bob.evaluate((c) => document.querySelector(`${c} button.bid`)?.title ?? 'no auction card', bobCard);
          bidLog.push(`round ${round} attempt ${attempt}: #${cand} could not bid (${why})`);
          continue;
        }
        // Check and click in one step in the page: no state update can land
        // between the two (a plain DOM click; the card may be rebuilt).
        const clicked = await bob.evaluate(({ c, id }) => {
          if (document.querySelector('#market-mine')?.value !== String(id)) return 'the pick changed';
          const b = document.querySelector(`${c} button.bid`);
          if (!b) return 'the card went away';
          if (b.disabled) return `the button was disabled again (${b.title})`;
          window.__results = [];
          b.click();
          return 'clicked';
        }, { c: bobCard, id: cand });
        if (clicked !== 'clicked') {
          bidLog.push(`round ${round} attempt ${attempt}: #${cand} not clicked: ${clicked}`);
          continue;
        }
        bid = await waitFor(`the bid result for #${cand} (attempt ${attempt})`, () => bob.evaluate(() => window.__results.find((r) => r.type === 'bid') ?? null), 30000);
        if (bid.mine !== cand) throw new Error(`the Bid button bid for #${bid.mine}, not the picked #${cand}`);
        if (bid.ok) {
          bidder = cand;
          bidLog.push(`round ${round} attempt ${attempt}: bid for #${cand} through the UI, ${((Date.now() - opened) / 1000).toFixed(0)} s after the auction opened`);
        } else if (retryBid.includes(bid.error?.code)) {
          bidLog.push(`round ${round} attempt ${attempt}: bid for #${cand} refused (${bid.error.code})`);
        } else if (closedBid.includes(bid.error?.code) && !(await auctionOpen(auctionId))) {
          gone = true;
        } else {
          throw new Error(`bid failed: ${JSON.stringify(bid.error)} (lot #${lotOrg} ${lotSex}, bidder #${cand})`);
        }
      }
      if (bid?.ok) break;
      if (gone || !(await auctionOpen(auctionId))) {
        closed('before a bid went through');
        continue;
      }
      // No bid could be made on an open auction: ann withdraws it.
      await page.click('#market-refresh');
      const withdraw = `#my-listings .listing[data-listing="${auctionId}"] button.withdraw`;
      await page.waitForSelector(`${withdraw}:not([disabled])`, { timeout: 30000 });
      await page.evaluate(() => { window.__results = []; });
      await page.click(withdraw, { timeout: 15000 });
      const w = await waitFor('withdraw_listing result for the auction', () => page.evaluate(() => window.__results.find((r) => r.type === 'withdraw_listing') ?? null), 30000);
      if (!w.ok) {
        // Its deadline passed (or the lot died) between the check and the click.
        if (w.error?.code === 'unknown_listing' && !(await auctionOpen(auctionId))) {
          closed('as ann withdrew it');
          continue;
        }
        throw new Error(`withdrawing auction ${auctionId} failed: ${JSON.stringify(w.error)}`);
      }
      await waitFor('auction withdrawn', async () => !(await auctionOpen(auctionId)), 30000);
      withdrawnAuction = true;
    }
    console.log(`  … auction bid path: ${bid?.ok ? 'bid through the UI' : withdrawnAuction ? 'no bid, ann withdrew' : 'no auction stayed open'} (${bidLog.join('; ')})`);
    if (bid?.ok) {
      await waitFor('bob\'s bid in escrow', async () => (await renown(bob)).escrow >= reserve, 30000);
      await page.click('#market-refresh');
      // (Unless it closed meanwhile: then it settled with bob's bid.)
      await waitFor('ann sees the bid', async () => {
        if (!(await auctionOpen(auctionId))) return true;
        const seen = await page.evaluate(([id, reserve]) => {
          const c = document.querySelector(`#my-listings .listing[data-listing="${id}"]`);
          return !!c && !!c.querySelector('.auction-line')?.textContent.includes(`high bid ${reserve}`) && !!c.querySelector('button.withdraw')?.disabled;
        }, [auctionId, reserve]);
        if (!seen) {
          await page.click('#market-refresh');
          await new Promise((r) => setTimeout(r, 1000));
        }
        return seen;
      }, 30000);
      step(`ann auctioned her ${lotSex} #${lotOrg} (listing ${auctionId}); bob bid ${reserve} renown for #${bidder} from the card (held in escrow, withdraw disabled)`);
    } else if (withdrawnAuction) {
      step(`ann auctioned listing ${auctionId} and withdrew it (no bid went through: ${bidLog.join('; ')})`);
    } else {
      throw new Error(`every one of ${auctionRounds} auctions closed before a bid went through (${bidLog.join('; ')})`);
    }
    await bob.selectOption('#market-kind', 'any');
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
        const got = await waitFor('selection', () => page.evaluate(() => window.evo.model.selected), 6000).catch(() => null);
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
        const moveRes = await waitFor('order_move result', () => page.evaluate(() => window.__results.find((r) => r.type === 'order_move')), 10000).catch(() => null);
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
        const samples = { predicted: 0, glided: 0 };
        let over = null;
        const walk = await waitFor('organism walks toward the target', async () => {
          // Sampled in an animation frame callback queued after the view's
          // own (Pixi's ticker queued it in the previous frame), so the drawn
          // position was computed from the very state read here. Outside a
          // frame, states that arrived since the last frame move the server
          // position but not yet the marker: on a loaded machine frames can
          // be 100+ ms apart, which in fast mode (20 ms ticks, up to a tile
          // per tick) is several tiles of honest staleness, not drift.
          const o = await page.evaluate(({ id, target }) => new Promise((resolve) => requestAnimationFrame(() => {
            const o = window.evo.model.state.organisms.find((x) => x.id === id);
            if (!o) return resolve(null);
            const v = window.evo.screen()?.view;
            // The drawn (predicted, smoothed) position against the server's.
            const m = v?.markerWorldPos(id);
            const gap = m ? Math.hypot(m.x - o.x, m.y - o.y) : 0;
            // Was it drawn from a prediction this frame, or from its glide?
            const predicted = !!v && v.predictor.id === id && v.predictor.goal(performance.now(), v.tickClock.msPerTick) !== null;
            resolve({ d: Math.hypot(o.x - target.x, o.y - target.y), action: o.action, possessed: o.possessed, speed: o.speed, gap, predicted });
          })), { id, target });
          if (!o || !o.possessed) {
            lost = o ? 'released' : 'died';
            return { lost };
          }
          actions.add(o.action);
          best = Math.min(best, o.d);
          maxGap = Math.max(maxGap, o.gap);
          if (o.speed !== undefined) speed = o.speed;
          // The bound from the motion design (client/src/world/motion.ts).
          // Predicted: the goal leads the server position by at most
          // LEAD_TICKS (0.5) ticks of movement, and the drawn marker eases
          // toward the goal, snapping to it beyond RECONCILE_SNAP (3 tiles):
          // so within 3 + 0.5 x speed. Glided (no prediction: no order, a
          // detour, a stall): the glide runs from where the marker was drawn
          // to the latest server position and snaps beyond SNAP_TILES (6,
          // Manhattan), and easing toward a point on that segment never
          // moves away from the server position: so within 6.
          const bound = o.predicted ? 3 + 0.5 * (o.speed ?? 0) : 6;
          samples[o.predicted ? 'predicted' : 'glided']++;
          if (o.gap > bound + 0.01) over ??= { gap: o.gap, bound, predicted: o.predicted, speed: o.speed };
          // Closer by a tile, or arrived (order dropped near the target).
          return best < d0 - 1 || best <= 1.0 ? { d0, best, action: o.action, attempt, maxGap, speed, samples, over } : null;
        }, 20000).catch(() => null);
        if (lost) throw new Retry(`#${id} ${lost} while walking`);
        if (walk) return walk;
        console.log(`  … walk attempt ${attempt}: closest ${best.toFixed(1)} of ${d0.toFixed(1)} tiles (actions seen: ${[...actions].join(', ')}); re-issuing the order`);
      }
      throw new Error(`organism #${id} did not walk toward any of ${attempts} targets (actions seen: ${[...actions].join(', ')})`);
    }

    // The motion check compares the drawn marker with the server position
    // every frame: at the browser's own frame rate, not the e2e cap.
    await page.evaluate(() => window.__e2eFrameCap?.(0));
    const tried = [];
    let walked = null;
    for (let round = 1; round <= 4 && !walked; round++) {
      try {
        const selected = await clickOwn(tried);
        tried.push(selected);
        if (!(await page.waitForSelector('#btn-possess', { timeout: 10000 }).then(() => true, () => false))) throw new Retry(`#${selected} has no possess button (died?)`);
        step(`clicked own organism #${selected}; inspector open`);
        await page.click('#btn-possess', { timeout: 10000 }).catch(() => { throw new Retry('possess button went away'); });
        const possessed = await waitFor('possession', () => page.evaluate((id) => window.evo.model.state.organisms.find((o) => o.id === id && o.possessed)?.id, selected), 15000)
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
    // The drawn marker stayed within the motion design's bound of the
    // server position (see the sampling above).
    if (!(walked.speed > 0)) throw new Error(`the possessed organism's state has no speed (${walked.speed})`);
    if (walked.over) {
      const o = walked.over;
      throw new Error(`drawn #${walked.id} was ${o.gap.toFixed(2)} tiles from the server position (${o.predicted ? 'predicted' : 'glided'}, speed ${o.speed?.toFixed(2)}; bound ${o.bound.toFixed(2)})`);
    }
    step(`prediction: speed ${walked.speed.toFixed(2)} tiles/tick, drawn marker at most ${walked.maxGap.toFixed(2)} tiles from the server position (${walked.samples.predicted} predicted, ${walked.samples.glided} glided frames sampled)`);
    await page.evaluate((fps) => window.__e2eFrameCap?.(fps), FPS);

    // Group command and a server-side error surfaced as a toast.
    await page.click('#migrate');
    await page.mouse.click(canvasBox.x + 200, canvasBox.y + 600);
    const mig = await waitFor('migrate result', () => page.evaluate(() => window.__results.find((r) => r.type === 'migrate')));
    if (!mig.ok) throw new Error(`migrate failed: ${JSON.stringify(mig.error)}`);
    await page.evaluate(() => window.evo.socket.send({ type: 'possess', organism: 999999 }));
    await page.waitForSelector('.toast.error:has-text("999999")', { timeout: 15000 });
    step('migrate ok; command errors shown as toasts');

    // Group commands: split the herd from the toolbar, draw a territory for
    // the new group with two taps, see it on the map and in the Lineage
    // tab, then merge the groups back.
    await page.evaluate(() => { window.__results = []; });
    await page.click('#split-group');
    const split = await waitFor('split_group result', () => page.evaluate(() => window.__results.find((r) => r.type === 'split_group')));
    if (!split.ok) throw new Error(`split_group failed: ${JSON.stringify(split.error)}`);
    const newGroup = split.data.group;
    await waitFor('the new group picked in the group menu', () => page.evaluate((g) => {
      const sel = document.querySelector('#group-select');
      return sel.value === String(g) && sel.options.length === 3 && window.evo.model.state.groups.length === 2;
    }, newGroup), 20000);
    await page.click('#territory');
    await page.mouse.click(canvasBox.x + canvasBox.width / 2, canvasBox.y + canvasBox.height / 2);
    const draft = await waitFor('a territory draft', () => page.evaluate(() => window.evo.screen().view.overlayInfo().draft));
    if (draft.group !== newGroup) throw new Error(`draft for group ${draft.group}, want ${newGroup}`);
    const zoom = await page.evaluate(() => window.evo.screen().view.cam.zoom);
    await page.mouse.click(canvasBox.x + canvasBox.width / 2 + 4 * zoom, canvasBox.y + canvasBox.height / 2);
    const claim = await waitFor('claim_territory result', () => page.evaluate(() => window.__results.find((r) => r.type === 'claim_territory')));
    if (!claim.ok) throw new Error(`claim_territory failed: ${JSON.stringify(claim.error)}`);
    const shown = await waitFor('the territory in the state and on the map', () => page.evaluate((g) => {
      const t = window.evo.model.state.groups.find((x) => x.id === g)?.territory;
      const drawn = window.evo.screen().view.overlayInfo().territories;
      return t && drawn.some((d) => d.group === g) ? { t, drawn: drawn.length } : null;
    }, newGroup), 20000);
    if (Math.abs(shown.t.radius - claim.data.radius) > 1e-3 || shown.t.radius < 3) throw new Error(`territory ${JSON.stringify(shown.t)} vs claim ${JSON.stringify(claim.data)}`);
    await page.waitForTimeout(400);
    await page.screenshot({ path: path.join(shots, 'territory.png') });
    // The only territory is the player's home range (shielded while they
    // are offline): flagged in the state, ringed on the map, explained on
    // the group's card.
    await waitFor('the territory as the home range', () => page.evaluate((g) =>
      window.evo.model.state.groups.find((x) => x.id === g)?.home_range === true
        && window.evo.screen().view.overlayInfo().home === g, newGroup), 20000);
    await page.click('#tab-lineage');
    await page.waitForSelector(`#group-list .group-card[data-group="${newGroup}"] button[data-action="abandon"]`, { timeout: 15000 });
    const homeNote = await waitFor('the home range on its group card', () => page.evaluate((g) =>
      document.querySelector(`#group-list .group-card[data-group="${g}"] .home-range`)?.textContent ?? null, newGroup), 15000);
    if (!/^Home range: when you log off/.test(homeNote)) throw new Error(`home range card note: ${homeNote}`);
    const cards = await page.evaluate(() => document.querySelectorAll('#group-list .group-card').length);
    // Two claims at once: the second lands within the first's cooldown (the
    // fast test world runs a day a second, so only back-to-back claims hit it).
    const cooldown = await page.evaluate(async (group) => {
      const claim = (x) => window.evo.socket.send({ type: 'claim_territory', group, x, y: 10, radius: 4 });
      const [a, b] = await Promise.all([claim(10), claim(12)]);
      return [a, b].find((r) => !r.ok) ?? null;
    }, newGroup);
    if (!cooldown || cooldown.error.code !== 'claim_cooldown') throw new Error(`back-to-back claims: ${JSON.stringify(cooldown)}`);
    // Rally the new group from its card with one map tap; it replaces the
    // territory, shows on the map and on the card, and is cancelled.
    await page.click(`#group-list .group-card[data-group="${newGroup}"] button[data-action="rally"]`);
    await page.mouse.click(canvasBox.x + canvasBox.width / 2, canvasBox.y + canvasBox.height / 2);
    const rally = await waitFor('rally result', () => page.evaluate(() => window.__results.find((r) => r.type === 'rally')));
    if (!rally.ok) throw new Error(`rally failed: ${JSON.stringify(rally.error)}`);
    // (The fast test world runs a day a second, so a rally lapses in about
    // three: look for it, and its cancel button on the card, while it stands,
    // and give it again if it lapsed before the page showed it.)
    await waitFor('the rally in the state, on the map and on the card', () => page.evaluate((g) => {
      const x = window.evo.model.state.groups.find((y) => y.id === g);
      if (!x?.rally) void window.evo.socket.send({ type: 'rally', group: g, x: 20, y: 20 });
      return x?.rally && !x.territory && window.evo.screen().view.overlayInfo().rallies.some((d) => d.group === g)
        && document.querySelector(`#group-list .group-card[data-group="${g}"] button[data-action="cancel-rally"]`) !== null;
    }, newGroup), 20000);
    // Cancelled straight after a fresh rally (before it can lapse).
    const cancelled = await page.evaluate(async (group) => {
      await window.evo.socket.send({ type: 'rally', group, x: 20, y: 20 });
      return window.evo.socket.send({ type: 'cancel_rally', group });
    }, newGroup);
    if (!cancelled.ok) throw new Error(`cancel_rally failed: ${JSON.stringify(cancelled.error)}`);
    await waitFor('no rally', () => page.evaluate((g) => !window.evo.model.state.groups.find((y) => y.id === g)?.rally, newGroup), 20000);
    await page.click(`#group-list .group-card[data-group="${newGroup}"] button[data-action="merge"]`);
    const merged = await waitFor('merge_groups result', () => page.evaluate(() => window.__results.find((r) => r.type === 'merge_groups')));
    if (!merged.ok) throw new Error(`merge_groups failed: ${JSON.stringify(merged.error)}`);
    await waitFor('one group again', () => page.evaluate(() => window.evo.model.state.groups.length === 1
      && document.querySelector('#group-select').value === 'all' && !window.evo.model.state.organisms.some((o) => o.group)), 20000);
    step(`groups: split ${split.data.moved} organisms into group ${newGroup}, territory radius ${shown.t.radius.toFixed(1)} drawn (${cards} group cards), cooldown refused a re-claim, rallied (radius ${rally.data.radius.toFixed(1)}) and cancelled, merged back (${merged.data.moved})`);

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
    await page.waitForSelector('#dashboard svg.chart', { timeout: 20000 });
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
    await waitFor('daily summaries hidden', () => page.evaluate(() => document.querySelectorAll('#alerts-feed .feed-item.kind-daily_summary').length === 0), 15000);
    const savedPrefs = await waitFor('alert preferences saved', () => page.evaluate(async () => {
      const { token } = JSON.parse(localStorage.getItem('evo.session'));
      const p = await (await fetch('/api/alerts/prefs', { headers: { authorization: `Bearer ${token}` } })).json();
      return p.hidden.includes('daily_summary') ? p : null;
    }), 15000);
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

    // Seasons tab (three-day ranked seasons here): the season under way, the
    // hall of fame with finished seasons' winners, your own record, and a
    // finished season's full standings.
    await page.click('#tab-seasons');
    const ranked = await waitFor('ranked seasons', () => page.evaluate(() => {
      const clock = document.querySelector('#season-clock')?.textContent ?? '';
      const finished = document.querySelectorAll('#season-history details').length;
      const record = document.querySelector('#season-record')?.textContent ?? '';
      return clock && finished > 0 && record.includes('ranked') ? { clock, finished, record } : null;
    }), 20000);
    const summary = await page.evaluate(() => document.querySelector('#season-history details p')?.textContent ?? '');
    if (!/^Season \d+ \(days \d+–\d+\): /.test(summary)) throw new Error(`season summary: ${summary}`);
    await page.click('#season-history details summary');
    await waitFor('a finished season\'s standings', () => page.evaluate(() => {
      // The list re-renders as new seasons end (every three days, about
      // three seconds here): open the newest again if a re-render raced the click.
      const d = document.querySelector('#season-history details');
      if (d && !document.querySelector('#season-history details[open]')) d.open = true;
      return document.querySelector('#season-history details[open] .standings table.board') !== null;
    }), 20000);
    const api = await page.evaluate(async () => {
      const me = window.evo.model.player;
      const [list, mine] = await Promise.all([(await fetch('/api/seasons')).json(), (await fetch(`/api/seasons/player/${me}`)).json()]);
      return { total: list.total, mine: mine.seasons.length };
    });
    if (api.total < 1 || api.mine < 1) throw new Error(`seasons API: ${JSON.stringify(api)}`);
    step(`seasons tab: ${ranked.clock.split('·')[0].trim()} under way, ${ranked.finished} finished shown (${api.total} stored, ${api.mine} with you ranked)`);
    await page.screenshot({ path: path.join(shots, 'seasons.png') });

    if (pageErrors.length) throw new Error(`page errors: ${pageErrors.join('; ')}`);
    // Every delta state applied (none needed a resync).
    const decodeErrors = await Promise.all(pages.map((p) => p.evaluate(() => window.evo?.socket.decodeErrors ?? 0)));
    if (decodeErrors.some((n) => n > 0)) throw new Error(`delta states that did not decode: ${decodeErrors}`);
    step('every delta state decoded (protocol 3, no resyncs)');
    console.log(`smoke test passed; screenshots in ${shots}`);
  } catch (e) {
    console.error(serverLog.split('\n').slice(-20).join('\n'));
    for (const [k, p] of pages.entries()) await p.screenshot({ path: path.join(shots, `failure-${k}.png`) }).catch(() => {});
    throw e;
  } finally {
    await browser.close();
    server.kill('SIGINT');
    if (process.env.SERVER_LOG_FILE) (await import('node:fs')).writeFileSync(process.env.SERVER_LOG_FILE, serverLog);
  }
}

main().catch((e) => {
  console.error(e);
  process.exit(1);
});
