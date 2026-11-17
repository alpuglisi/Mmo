#!/usr/bin/env node
// Marketing capture: records scripted scenes of the real game as short
// vertical (9:16) and square videos, plus the share cards, with a manifest.
// See tools/marketing/README.md.
//
//   node tools/marketing/capture.mjs [--scenes evolution,boom-bust,builder,catastrophe]
//       [--formats vertical,square] [--seconds 12] [--fps 12] [--out tools/marketing/out]
//       [--no-ffmpeg] [--keep-frames]
//
// Needs the release server (`cargo build --release -p evo-server`) and the
// built client (`cd client && npm run build`). Each scene starts its own
// in-memory server with a fast clock (`--tick-ms`), drives the client in
// headless Chromium (Playwright) and screenshots it frame by frame: a scene
// is a time-lapse, its frames as far apart in game time as the world moved
// while one was taken. With ffmpeg the frames become an MP4 (H.264) and a
// GIF; without it they stay a PNG sequence and the GIF comes from gif.mjs.

import { spawn, spawnSync } from 'node:child_process';
import { createRequire } from 'node:module';
import { existsSync, mkdirSync, rmSync, writeFileSync } from 'node:fs';
import { createServer } from 'node:net';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { encodeGif } from './gif.mjs';

const here = path.dirname(fileURLToPath(import.meta.url));
const repo = path.resolve(here, '../..');
const require = createRequire(path.join(repo, 'client') + '/');
const { chromium } = require('playwright');

// --- options ---------------------------------------------------------------

const SCENES = ['evolution', 'boom-bust', 'builder', 'catastrophe'];
const FORMATS = {
  vertical: { viewport: { width: 540, height: 960 }, scale: 2, gifWidth: 360 },
  square: { viewport: { width: 600, height: 600 }, scale: 1.8, gifWidth: 400 },
};

function parseArgs(argv) {
  const o = { scenes: SCENES, formats: Object.keys(FORMATS), seconds: 12, fps: 12, out: path.join(here, 'out'), ffmpeg: true, keepFrames: false };
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    const v = () => argv[++i];
    if (a === '--scenes') o.scenes = v().split(',').filter(Boolean);
    else if (a === '--formats') o.formats = v().split(',').filter(Boolean);
    else if (a === '--seconds') o.seconds = Number(v());
    else if (a === '--fps') o.fps = Number(v());
    else if (a === '--out') o.out = path.resolve(v());
    else if (a === '--no-ffmpeg') o.ffmpeg = false;
    else if (a === '--keep-frames') o.keepFrames = true;
    else if (a === '-h' || a === '--help') {
      console.log('usage: node tools/marketing/capture.mjs [--scenes a,b] [--formats vertical,square] [--seconds N] [--fps N] [--out DIR] [--no-ffmpeg] [--keep-frames]');
      process.exit(0);
    } else throw new Error(`unknown option ${a}`);
  }
  for (const s of o.scenes) if (!SCENES.includes(s)) throw new Error(`unknown scene ${s} (${SCENES.join(', ')})`);
  for (const f of o.formats) if (!FORMATS[f]) throw new Error(`unknown format ${f} (${Object.keys(FORMATS).join(', ')})`);
  if (!(o.seconds > 0) || !(o.fps > 0)) throw new Error('--seconds and --fps must be positive');
  return o;
}

const opts = parseArgs(process.argv.slice(2));
const bin = process.env.EVO_SERVER_BIN ?? path.join(repo, 'target/release/evo-server');
const dist = path.join(repo, 'client/dist');
const hasFfmpeg = opts.ffmpeg && spawnSync('ffmpeg', ['-version'], { stdio: 'ignore' }).status === 0;

const log = (m) => console.log(`  ${m}`);

// --- the server --------------------------------------------------------------

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

async function withServer(args, fn) {
  const port = await freePort();
  const base = `http://127.0.0.1:${port}`;
  const server = spawn(bin, ['--memory', '--no-rate-limits', '--no-push', '--bind', `127.0.0.1:${port}`, '--static', dist, ...args], {
    stdio: ['ignore', 'ignore', 'pipe'],
    env: { ...process.env, EVO_E2E_HOOKS: '1', RUST_LOG: 'warn' },
  });
  let err = '';
  server.stderr.on('data', (d) => (err = (err + d).slice(-4000)));
  try {
    for (let i = 0; ; i++) {
      try {
        if ((await fetch(`${base}/api/world`)).ok) break;
      } catch {
        /* not up yet */
      }
      if (i > 100) throw new Error(`server did not start: ${err}`);
      await sleep(100);
    }
    return await fn(base);
  } finally {
    // Wait for it to exit: the next scene's server may get the same port.
    const exited = new Promise((r) => server.once('exit', r));
    server.kill('SIGINT');
    const late = setTimeout(() => server.kill('SIGKILL'), 10000);
    await exited;
    clearTimeout(late);
  }
}

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

async function api(base, method, p, body, token) {
  const res = await fetch(base + p, {
    method,
    headers: { 'content-type': 'application/json', ...(token ? { authorization: `Bearer ${token}` } : {}) },
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  const text = await res.text();
  if (!res.ok) throw new Error(`${method} ${p}: ${res.status} ${text}`);
  return text ? JSON.parse(text) : null;
}

/** Register a player and found a lineage; returns the session and the lineage index. */
async function player(base, name, design) {
  const auth = await api(base, 'POST', '/api/register', { name, password: 'a long marketing password' });
  const founded = await api(base, 'POST', '/api/lineage', design, auth.token);
  return { auth, lineage: founded.lineage };
}

/** Wait until the spectator page has data. */
async function spectating(page) {
  await page.waitForFunction(() => window.evoSpectator?.last() && document.querySelector('#sp-map'), null, { timeout: 30000 });
  await sleep(1500);
}

// --- recording ---------------------------------------------------------------

/**
 * Screenshot `frames` frames of `page` into `dir` (frame-0000.png …),
 * calling `step(i)` before each; returns the PNG buffers.
 */
async function record(page, dir, frames, step) {
  mkdirSync(dir, { recursive: true });
  const shots = [];
  for (let i = 0; i < frames; i++) {
    if (step) await step(i);
    const png = await page.screenshot({ type: 'png' });
    writeFileSync(path.join(dir, `frame-${String(i).padStart(4, '0')}.png`), png);
    shots.push(png);
  }
  return shots;
}

/** Show `pngDataUrl` letterboxed in the page for `frames` frames (a card at the end of a scene). */
async function holdImage(page, dir, start, frames, pngDataUrl) {
  await page.setContent(`<html><body style="margin:0;background:#0d1117;display:grid;place-items:center;height:100vh">
    <img src="${pngDataUrl}" style="max-width:100vw;max-height:100vh;object-fit:contain"></body></html>`);
  await page.waitForFunction(() => document.querySelector('img')?.complete);
  const png = await page.screenshot({ type: 'png' });
  const shots = [];
  for (let i = 0; i < frames; i++) {
    writeFileSync(path.join(dir, `frame-${String(start + i).padStart(4, '0')}.png`), png);
    shots.push(png);
  }
  return shots;
}

/** RGBA of PNG frames, scaled to `width` wide, decoded in a blank page (no image library needed). */
async function decodeFrames(browser, pngs, width) {
  const page = await browser.newPage();
  const out = [];
  let height = 0;
  for (const png of pngs) {
    const r = await page.evaluate(async ([b64, w]) => {
      const img = new Image();
      img.src = `data:image/png;base64,${b64}`;
      await img.decode();
      const h = Math.round((img.height * w) / img.width);
      const c = document.createElement('canvas');
      c.width = w;
      c.height = h;
      const ctx = c.getContext('2d');
      ctx.imageSmoothingQuality = 'high';
      ctx.drawImage(img, 0, 0, w, h);
      const data = ctx.getImageData(0, 0, w, h).data;
      let s = '';
      for (let i = 0; i < data.length; i += 0x8000) s += String.fromCharCode(...data.subarray(i, i + 0x8000));
      return { h, b64: btoa(s) };
    }, [png.toString('base64'), width]);
    height = r.h;
    out.push({ rgba: Buffer.from(r.b64, 'base64') });
  }
  await page.close();
  return { frames: out, height };
}

/** Turn a scene's frames into its media; returns the file names (relative to the output dir). */
async function encode(browser, dir, base, pngs, fmt) {
  const files = {};
  const pattern = path.join(dir, 'frame-%04d.png');
  if (hasFfmpeg) {
    const mp4 = `${base}.mp4`;
    const r = spawnSync('ffmpeg', ['-y', '-loglevel', 'error', '-framerate', String(opts.fps), '-i', pattern,
      '-vf', 'scale=trunc(iw/2)*2:trunc(ih/2)*2', '-c:v', 'libx264', '-pix_fmt', 'yuv420p', '-crf', '22', '-movflags', '+faststart',
      path.join(opts.out, mp4)]);
    if (r.status !== 0) throw new Error(`ffmpeg mp4 failed: ${r.stderr}`);
    files.mp4 = mp4;
    const gif = `${base}.gif`;
    const g = spawnSync('ffmpeg', ['-y', '-loglevel', 'error', '-framerate', String(opts.fps), '-i', pattern,
      '-vf', `fps=${Math.min(opts.fps, 10)},scale=${fmt.gifWidth}:-1:flags=lanczos,split[a][b];[a]palettegen=max_colors=128[p];[b][p]paletteuse=dither=bayer`,
      '-loop', '0', path.join(opts.out, gif)]);
    if (g.status !== 0) throw new Error(`ffmpeg gif failed: ${g.stderr}`);
    files.gif = gif;
  } else {
    // Every other frame at most 10 fps keeps the GIF small.
    const stride = Math.max(1, Math.round(opts.fps / 10));
    const picked = pngs.filter((_, i) => i % stride === 0);
    const { frames, height } = await decodeFrames(browser, picked, fmt.gifWidth);
    const gif = `${base}.gif`;
    writeFileSync(path.join(opts.out, gif), encodeGif(frames, fmt.gifWidth, height, (1000 / opts.fps) * stride));
    files.gif = gif;
  }
  if (!hasFfmpeg || opts.keepFrames) files.frames = path.relative(opts.out, dir);
  else rmSync(dir, { recursive: true, force: true });
  return files;
}

// --- scenes ------------------------------------------------------------------

const TAGS = ['#EvolutionMMO', '#indiegame', '#gamedev', '#evolution', '#simulation'];

/** A weird but valid design (the server checks the budget). */
const WEIRD = {
  name: 'Glimmer striders',
  thermo: 'warm',
  birth: 'live',
  litter_size: 3,
  founders: 40,
  traits: { body_size: 0.35, fur: 0.6, coloration: 0.9, wings: 0.0, large_ears: 0.8, tail: 0.7 },
  morph: { eye_count: 3, leg_count: 6, ear_count: 4, ear_shape: 'long', tail_present: true, tail_shape: 'bushy' },
};

const scenes = {
  evolution: {
    title: 'A species evolving over the years',
    caption: 'Forty founders, a few game years: watch a player-designed species spread, adapt and branch. Every animal is simulated.',
    tags: ['#ArtificialLife', '#procedural'],
    server: ['--fast', '--tick-ms', '4', '--state-ms', '500', '--size', '64', '--seed', '11', '--days-per-year', '24', '--no-catastrophes'],
    async run({ base, page, dir, frames, cards }) {
      const p = await player(base, 'Glimmer', WEIRD);
      await page.goto(`${base}/?watch=main&lineage=${p.lineage}&stream=1`);
      await spectating(page);
      const shots = await record(page, dir, frames - Math.round(opts.fps * 2.5), () => sleep(120));
      // The family tree at the end, from the game itself (logged in as the player).
      await page.addInitScript((s) => localStorage.setItem('evo.session', JSON.stringify(s)), p.auth);
      await page.goto(`${base}/?tour=off`);
      await page.waitForSelector('#tab-family', { timeout: 30000 });
      await page.click('#tab-family');
      await page.waitForFunction(() => window.evoCards?.tree(), null, { timeout: 30000 });
      const tree = await page.evaluate(() => window.evoCards.tree());
      const portrait = await page.evaluate(() => window.evoCards.portrait());
      cards.push(['family-tree', tree], ['portrait', portrait]);
      shots.push(...(await holdImage(page, dir, shots.length, frames - shots.length, tree)));
      return shots;
    },
  },
  'boom-bust': {
    title: 'Predators and prey: boom and bust',
    caption: 'Foxes thrive on voles until the voles run out. The population chart tells the rest. No scripts, just the simulation.',
    tags: ['#ecology', '#predator'],
    server: ['--fast', '--tick-ms', '3', '--state-ms', '500', '--size', '64', '--seed', '7', '--days-per-year', '24', '--no-catastrophes'],
    async run({ base, page, dir, frames, cards }) {
      const s = await api(base, 'GET', '/api/spectate');
      const fox = s.lineages.find((l) => /fox/i.test(l.name)) ?? s.lineages.find((l) => l.look?.diet === 'carnivore') ?? s.lineages[0];
      await page.goto(`${base}/?watch=main&lineage=${fox.lineage}&stream=1`);
      await spectating(page);
      const shots = await record(page, dir, frames, () => sleep(150));
      cards.push(['portrait', await page.evaluate(() => window.evoSpectator.portraitCard())]);
      return shots;
    },
  },
  builder: {
    title: 'Design a weird creature',
    caption: 'Six legs, three eyes, fan ears and a club tail: the body builder shows what every choice does to sight and speed.',
    tags: ['#creaturedesign', '#builder'],
    server: ['--fast', '--size', '32', '--no-wild'],
    async run({ base, page, dir, frames }) {
      await page.goto(`${base}/?builder=strider&tour=off`);
      await page.waitForFunction(() => window.evoBuilder?.builder, null, { timeout: 30000 });
      // Keyframes the builder moves through, one value at a time.
      const keys = [
        { eye_count: 3 }, { eye_azimuth: 110 }, { eye_size: 0.9 }, { leg_count: 6 }, { leg_length: 0.8 },
        { ear_count: 4 }, { ear_shape: 'fan' }, { ear_size: 0.9 }, { tail_present: true }, { tail_shape: 'club' },
        { tail_length: 0.9 }, { horn_size: 0.7 }, { pattern: 'stripes' }, { snout_length: 0.8 }, { fang_size: 0.6 },
      ];
      const per = Math.max(1, Math.floor(frames / keys.length));
      return record(page, dir, frames, async (i) => {
        const k = Math.floor(i / per);
        if (i % per === 0 && keys[k]) await page.evaluate((v) => window.evoBuilder.builder.set(v, { emit: true }), keys[k]);
        await sleep(40);
      });
    },
  },
  catastrophe: {
    title: 'A catastrophe sweeps the map',
    caption: 'A severe winter rolls over the world. Which lineages ride it out? Catastrophes reshape the rankings every era.',
    tags: ['#survival', '#catastrophe'],
    server: ['--fast', '--tick-ms', '12', '--state-ms', '500', '--size', '64', '--seed', '5', '--days-per-year', '48'],
    async run({ base, page, dir, frames }) {
      await player(base, 'Tundra', { ...WEIRD, name: 'Tundra trotters', traits: { body_size: 0.5, fur: 0.9, fat_stores: 0.8 } });
      await page.goto(`${base}/?watch=main&stream=1`);
      await spectating(page);
      const before = Math.round(frames * 0.2);
      return record(page, dir, frames, async (i) => {
        if (i === before) await api(base, 'POST', '/api/e2e/catastrophe', { kind: 'severe_winter', days: 400 });
        await sleep(150);
      });
    },
  },
};

// --- main --------------------------------------------------------------------

async function main() {
  if (!existsSync(bin)) throw new Error(`no server binary at ${bin}: cargo build --release -p evo-server`);
  if (!existsSync(path.join(dist, 'index.html'))) throw new Error('no client/dist: cd client && npm run build');
  mkdirSync(opts.out, { recursive: true });
  console.log(`capture: ${opts.scenes.join(', ')} · ${opts.formats.join(', ')} · ${opts.seconds} s at ${opts.fps} fps · ${hasFfmpeg ? 'ffmpeg' : 'no ffmpeg: frames + GIF'} → ${opts.out}`);
  const manifest = { generated: new Date().toISOString(), ffmpeg: hasFfmpeg, fps: opts.fps, items: [] };
  for (const id of opts.scenes) {
    const sc = scenes[id];
    const item = {
      id, title: sc.title, caption: sc.caption, hashtags: [...TAGS, ...sc.tags],
      duration_s: opts.seconds, fps: opts.fps, files: {}, cards: {},
    };
    for (const fname of opts.formats) {
      const fmt = FORMATS[fname];
      const frames = Math.round(opts.seconds * opts.fps);
      const started = Date.now();
      await withServer(sc.server, async (base) => {
        // A browser of its own per recording (with one shared browser the
        // second recording's pages stopped answering).
        const browser = await chromium.launch();
        const context = await browser.newContext({ viewport: fmt.viewport, deviceScaleFactor: fmt.scale });
        const page = await context.newPage();
        const errors = [];
        page.on('pageerror', (e) => errors.push(e.message));
        const dir = path.join(opts.out, 'frames', `${id}-${fname}`);
        rmSync(dir, { recursive: true, force: true });
        const cards = [];
        try {
          const shots = await sc.run({ base, page, dir, frames, cards }).catch(async (e) => {
            await page.screenshot({ path: path.join(opts.out, `failure-${id}-${fname}.png`) }).catch(() => {});
            e.message = `${id} ${fname}: ${e.message}${errors.length ? `; page errors: ${errors.join('; ')}` : ''}`;
            throw e;
          });
          item.files[fname] = await encode(browser, dir, `${id}-${fname}`, shots, fmt);
          for (const [kind, url] of cards) {
            if (!url || item.cards[kind]) continue;
            const file = `${id}-${kind}.png`;
            writeFileSync(path.join(opts.out, file), Buffer.from(url.split(',')[1], 'base64'));
            item.cards[kind] = file;
          }
        } finally {
          await browser.close();
        }
        if (errors.length) throw new Error(`${id} ${fname}: page errors: ${errors.join('; ')}`);
      });
      log(`${id} ${fname}: ${Object.values(item.files[fname]).join(', ')} (${((Date.now() - started) / 1000).toFixed(0)} s)`);
    }
    manifest.items.push(item);
    writeFileSync(path.join(opts.out, 'manifest.json'), JSON.stringify(manifest, null, 2));
  }
  console.log(`capture: done; manifest ${path.join(opts.out, 'manifest.json')}`);
}

main().catch((e) => {
  console.error(e);
  process.exit(1);
});
