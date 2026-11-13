// Screenshot helper (not part of the smoke test): a short-year world on a
// 96×96 map, played until winter, to show snow and ice on the tile map.
//
//   node e2e/winter_shot.mjs   (SCREENSHOT_DIR, EVO_SERVER_BIN as for smoke.mjs)

import { spawn } from 'node:child_process';
import { createRequire } from 'node:module';
import { mkdirSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const require = createRequire(import.meta.url);
const { chromium } = require('playwright');
const here = path.dirname(fileURLToPath(import.meta.url));
const clientDir = path.resolve(here, '..');
const bin = process.env.EVO_SERVER_BIN ?? path.resolve(clientDir, '../target/release/evo-server');
const shots = path.resolve(process.env.SCREENSHOT_DIR ?? path.join(here, 'out'));
mkdirSync(shots, { recursive: true });
const port = 18000 + Math.floor(Math.random() * 2000);
const base = `http://127.0.0.1:${port}`;
const server = spawn(bin, ['--fast', '--memory', '--size', '96', '--seed', '3', '--days-per-year', '16', '--bind', `127.0.0.1:${port}`,
  '--static', path.join(clientDir, 'dist')], { stdio: 'ignore' });
const browser = await chromium.launch();
try {
  for (let k = 0; k < 100; k++) {
    try { if ((await fetch(`${base}/api/world`)).ok) break; } catch { /* starting */ }
    await new Promise((r) => setTimeout(r, 100));
  }
  const page = await browser.newPage({ viewport: { width: 1280, height: 800 }, deviceScaleFactor: 1 });
  await page.goto(base);
  await page.fill('#login-name', 'winter_demo');
  await page.fill('#login-password', 'a long password');
  await page.click('#register-submit');
  await page.waitForSelector('#design-name');
  await page.fill('#design-name', 'Snow hares');
  await page.click('#design-submit');
  await page.waitForSelector('#world-canvas');
  await page.waitForFunction(() => window.evo?.screen()?.view.terrain?.season === 'winter', null, { timeout: 60000, polling: 500 });
  await page.waitForTimeout(9000);
  await page.evaluate(() => {
    const v = window.evo.screen().view;
    v.setFollow(false);
    v.cam.zoom = 8;
    v.centerOn(48, 48);
  });
  await page.click('#tab-genetics');
  await page.waitForTimeout(6000);
  await page.screenshot({ path: path.join(shots, 'world-winter.png') });
  console.log(`winter screenshot in ${shots}`);
} finally {
  await browser.close();
  server.kill('SIGINT');
}
