// The Android app scenario (e2e/android/scenario.mjs) against desktop
// Chromium, to check the DevTools driver without an emulator. The setup
// mirrors the app's: the page is served from one origin (a static server for
// dist/, as the WebView's https://localhost) and the game server is another,
// typed into the Server field; the scenario talks to the page over a raw
// DevTools socket (--remote-debugging-port) exactly as it does to the
// WebView. "Backgrounding" freezes the page (Page Lifecycle `frozen`; the
// `resume` event wakes the socket when it thaws) and silently drops every
// open connection through a proxy in front of the server, without closing
// it, as a phone's sleeping radio or a network switch does: only the
// client's watchdog or wake() can notice.
//
//   npm run build && node e2e/android-desktop.mjs     (env as for smoke.mjs; BG_SECONDS, default 25)

import { spawn } from 'node:child_process';
import { createRequire } from 'node:module';
import { createReadStream, existsSync, mkdirSync, mkdtempSync, rmSync, statSync } from 'node:fs';
import { createServer as createHttpServer } from 'node:http';
import { createConnection, createServer } from 'node:net';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { runScenario } from './android/scenario.mjs';

const require = createRequire(import.meta.url);
const { chromium } = require('playwright');

const here = path.dirname(fileURLToPath(import.meta.url));
const clientDir = path.resolve(here, '..');
const bin = process.env.EVO_SERVER_BIN ?? path.resolve(clientDir, '../target/release/evo-server');
const dist = path.join(clientDir, 'dist');
const outDir = path.resolve(process.env.SCREENSHOT_DIR ?? path.join(here, 'out'), 'android-desktop');
const BG_SECONDS = Number(process.env.BG_SECONDS ?? 25);

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

const TYPES = { '.html': 'text/html', '.js': 'text/javascript', '.css': 'text/css', '.json': 'application/json', '.png': 'image/png', '.svg': 'image/svg+xml', '.webmanifest': 'application/manifest+json', '.ico': 'image/x-icon' };

/** dist/ on its own origin (index.html for unknown paths), like the app's bundled page. */
function staticServer(port) {
  const server = createHttpServer((req, res) => {
    const rel = decodeURIComponent(new URL(req.url, 'http://x').pathname);
    let file = path.join(dist, path.normalize(rel).replace(/^([/\\])+/, ''));
    if (!file.startsWith(dist) || !existsSync(file) || statSync(file).isDirectory()) file = path.join(dist, 'index.html');
    res.writeHead(200, { 'content-type': TYPES[path.extname(file)] ?? 'application/octet-stream' });
    createReadStream(file).pipe(res);
  });
  return new Promise((resolve) => server.listen(port, '127.0.0.1', () => resolve(server)));
}

/** A TCP proxy whose blackhole() turns every open connection into a dead one: bytes vanish, nothing closes. */
function blackholeProxy(port, target) {
  const live = new Set();
  const server = createServer((client) => {
    const upstream = createConnection(target, '127.0.0.1');
    const conn = { client, upstream, dead: false };
    live.add(conn);
    client.on('data', (d) => { if (!conn.dead) upstream.write(d); });
    upstream.on('data', (d) => { if (!conn.dead) client.write(d); });
    const end = () => { live.delete(conn); client.destroy(); upstream.destroy(); };
    client.on('close', end);
    upstream.on('close', end);
    client.on('error', end);
    upstream.on('error', end);
  });
  return new Promise((resolve) => server.listen(port, '127.0.0.1', () => resolve({
    server,
    blackhole() {
      for (const c of live) c.dead = true;
      return live.size;
    },
    closeAll() {
      for (const c of live) { c.client.destroy(); c.upstream.destroy(); }
    },
  })));
}

async function main() {
  if (!existsSync(bin)) throw new Error(`no server binary at ${bin} (cargo build --release -p evo-server)`);
  if (!existsSync(path.join(dist, 'index.html'))) throw new Error('no client build (npm run build)');
  mkdirSync(outDir, { recursive: true });
  const [apiPort, proxyPort, pagePort, cdpPort] = [await freePort(), await freePort(), await freePort(), await freePort()];
  const server = spawn(bin, ['--fast', '--memory', '--size', '64', '--no-rate-limits', '--bind', `127.0.0.1:${apiPort}`], { stdio: ['ignore', 'inherit', 'inherit'] });
  const userData = mkdtempSync(path.join(os.tmpdir(), 'evo-android-desktop-'));
  let context, proxy, pages;
  try {
    for (let i = 0; ; i++) {
      try {
        if ((await fetch(`http://127.0.0.1:${apiPort}/api/world`)).ok) break;
      } catch { /* not up yet */ }
      if (i > 100) throw new Error('server did not come up');
      await new Promise((r) => setTimeout(r, 200));
    }
    proxy = await blackholeProxy(proxyPort, apiPort);
    pages = await staticServer(pagePort);
    // A phone-sized page; --remote-debugging-port is the WebView's DevTools socket stand-in.
    context = await chromium.launchPersistentContext(userData, {
      headless: true,
      viewport: { width: 412, height: 915 }, deviceScaleFactor: 2.625, isMobile: true, hasTouch: true,
      args: [`--remote-debugging-port=${cdpPort}`, '--remote-allow-origins=*'],
    });
    const page = context.pages()[0] ?? await context.newPage();
    await page.goto(`http://127.0.0.1:${pagePort}/`);
    const pwCdp = await context.newCDPSession(page);
    const device = {
      label: 'desktop Chromium (phone viewport)',
      serverOrigin: `http://127.0.0.1:${proxyPort}`,
      match: (t) => t.url.startsWith(`http://127.0.0.1:${pagePort}/`),
      endpoint: async () => `http://127.0.0.1:${cdpPort}`,
      screenshot: (file) => page.screenshot({ path: file }),
      async background() {
        console.log(`    dropped ${proxy.blackhole()} open connection(s) silently`);
        await pwCdp.send('Page.setWebLifecycleState', { state: 'frozen' });
      },
      async resume() {
        await pwCdp.send('Page.setWebLifecycleState', { state: 'active' });
      },
    };
    const report = await runScenario(device, { outDir, backgroundSeconds: BG_SECONDS, name: `emu_desk_${Date.now().toString(36)}` });
    // The dropped socket can only come back through a new one.
    if (report.afterResume.reconnects < 1) throw new Error(`expected a reconnect after the dead socket, got ${report.afterResume.reconnects}`);
    console.log('Android scenario on desktop Chromium passed');
  } finally {
    await context?.close().catch(() => {});
    proxy?.closeAll();
    proxy?.server.close();
    pages?.close();
    server.kill('SIGTERM');
    rmSync(userData, { recursive: true, force: true });
  }
}

main().catch((e) => {
  console.error(`Android scenario on desktop Chromium FAILED: ${e.stack ?? e.message}`);
  process.exit(1);
});
