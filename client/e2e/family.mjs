// End-to-end: the family tree view on its dev harness (`/?builder=family`,
// a made-up lineage: `src/ui/familyDemo.ts`), no login. Starts its own
// evo-server like builder.mjs (only to serve client/dist).
//
// On a desktop and two phones (iPhone 13, iPhone SE): every stage and branch
// is drawn with an art-library portrait, the stages run founding → today
// with change labels in the players' trait names, the extinct branch is
// marked and greyed, the split of a split sits under its parent, and the page
// never scrolls sideways. The real game's Family tab is checked by smoke.mjs.
// Screenshots: e2e/out/family-*.png (2× device pixels).
//
//   npm run build && node e2e/family.mjs     (env as for smoke.mjs)

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

const step = (msg) => console.log(`  ✓ ${msg}`);
const check = (cond, msg, detail) => {
  if (!cond) throw new Error(`${msg}${detail === undefined ? '' : `: ${JSON.stringify(detail)}`}`);
};

function device(name) {
  const { defaultBrowserType: _ignored, ...d } = devices[name];
  return d;
}

async function run(browser, base, tag, ctxOpts) {
  const ctx = await browser.newContext(ctxOpts);
  const page = await ctx.newPage();
  const errors = [];
  page.on('pageerror', (e) => errors.push(String(e)));
  await page.goto(`${base}/?builder=family&tour=off`);
  await page.waitForSelector('#panel-family .ft-node');
  const s = await page.evaluate(() => {
    const nodes = [...document.querySelectorAll('#panel-family .ft-node')];
    const trunk = [...document.querySelectorAll('.ft-trunk > .ft-row > .ft-stem > .ft-node')];
    const branch = (id) => document.querySelector(`.ft-branch[data-species="${id}"]`);
    const rect = (el) => el.getBoundingClientRect();
    const marsh = branch(7);
    return {
      nodes: nodes.length,
      portraits: document.querySelectorAll('#panel-family .ft-portrait svg').length,
      labelled: [...document.querySelectorAll('#panel-family .ft-portrait')].every((p) => (p.getAttribute('aria-label') ?? '').includes('-blooded')),
      kinds: trunk.map((n) => n.dataset.kind),
      chips: [...document.querySelectorAll('.ft-trunk > .ft-row > .ft-stem .ft-chip')].map((c) => c.textContent),
      branches: [...document.querySelectorAll('.ft-branch')].map((b) => b.dataset.species),
      marshExtinct: marsh?.querySelector('.ft-node').classList.contains('ft-extinct') && marsh.textContent.includes('Extinct'),
      marshGrey: marsh ? getComputedStyle(marsh.querySelector('.ft-portrait')).filter.includes('grayscale') : false,
      nested: rect(branch(12)).left > rect(branch(9)).left,
      docW: document.documentElement.scrollWidth,
      vw: window.innerWidth,
      summary: document.querySelector('#family-summary')?.textContent ?? '',
    };
  });
  check(s.nodes >= 5, `${tag}: nodes`, s);
  check(s.portraits === s.nodes && s.labelled, `${tag}: every node has a described portrait`, s);
  check(s.kinds[0] === 'founding' && s.kinds[s.kinds.length - 1] === 'now', `${tag}: founding → today`, s.kinds);
  check(s.chips.includes('Stripes appeared') && s.chips.includes('Antlers appeared') && s.chips.some((c) => c.startsWith('Ear size ↑')), `${tag}: change labels`, s.chips);
  check(s.branches.join() === '7,9,12', `${tag}: branches`, s.branches);
  check(s.marshExtinct && s.marshGrey, `${tag}: the extinct branch is marked and greyed`, s);
  check(s.nested, `${tag}: a split of a split sits under its parent`, s);
  check(s.docW <= s.vw + 1, `${tag}: no sideways scroll`, s);
  check(errors.length === 0, `${tag}: page errors`, errors);
  await page.screenshot({ path: path.join(shots, `family-${tag}.png`), fullPage: true });
  step(`${tag}: ${s.nodes} nodes (${s.kinds.join(' → ')}), ${s.branches.length} branches, extinct marked; "${s.summary.slice(0, 70)}…"`);
  await ctx.close();
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
    await run(browser, base, 'desktop', { viewport: { width: 1280, height: 900 }, deviceScaleFactor: 2 });
    await run(browser, base, 'iphone13', device('iPhone 13'));
    await run(browser, base, 'iphone-se', device('iPhone SE'));
    console.log('family tree e2e passed');
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
