// The app smoke scenario, driven over the Chrome DevTools protocol. The same
// steps run against the Capacitor app's WebView in an Android emulator
// (e2e/android-webview.mjs, through adb) and against desktop Chromium
// (e2e/android-desktop.mjs, which checks this driver without a device):
//
//   1. the app loads and shows the login screen (the Server field open on a
//      native shell);
//   2. it is pointed at the test server through the Server field and
//      registers a new player;
//   3. the founder design (the onboarding for a new player) founds a lineage;
//   4. the map renders (the canvas shows more than a flat color) and state
//      frames arrive; the tour, if it opens, is skipped;
//   5. the app is backgrounded for longer than the socket's 20 s silence
//      watchdog, resumed, and must be connected again with fresh state frames
//      (the reconnects it took, the visibility changes the page saw and how
//      long recovery took go into the report).
//
// It also drives the iOS app's WKWebView in a simulator
// (e2e/ios-webview.mjs), through Appium and a DevTools-shaped session over
// WebDriver (e2e/ios/webdriver.mjs).
//
// `device` supplies what differs between them:
//   endpoint()      → the DevTools HTTP endpoint (re-forwarded if the app restarted)
//   match(target)   → whether a /json/list page target is the app
//   connect()       → instead of endpoint()/match: a ready session with the
//                     same send/evaluate/on/close/closed surface (iOS)
//   serverOrigin    → what to type into the Server field
//   screenshot(file)→ a whole-screen screenshot (optional)
//   background(), resume()
//   label           → for logs

import { writeFileSync } from 'node:fs';
import path from 'node:path';
import { connectPage } from './cdp.mjs';

const UI_MS = 45000;

/** Installed before the client opens a socket (and on every new document): counts sockets, frames and lifecycle events. */
const PROBE = `(() => {
  if (window.__e2e) return;
  const p = window.__e2e = { opens: 0, closes: 0, frames: 0, lastFrameAt: 0, events: [] };
  const log = (...e) => { p.events.push([Date.now(), ...e]); if (p.events.length > 200) p.events.shift(); };
  const Native = window.WebSocket;
  class ProbeSocket extends Native {
    constructor(...a) {
      super(...a);
      p.opens += 1;
      log('socket', String(a[0]).replace(/token=[^&]+/, 'token=…'));
      this.addEventListener('open', () => log('open'));
      this.addEventListener('message', () => { p.frames += 1; p.lastFrameAt = Date.now(); });
      this.addEventListener('close', (e) => { p.closes += 1; log('close', e.code); });
    }
  }
  window.WebSocket = ProbeSocket;
  document.addEventListener('visibilitychange', () => log('visibility', document.visibilityState));
  for (const t of ['freeze', 'resume', 'pause']) document.addEventListener(t, () => log(t));
  for (const t of ['online', 'offline', 'focus', 'blur']) window.addEventListener(t, () => log(t));
})();`;

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

async function waitFor(what, fn, timeoutMs = UI_MS) {
  const end = Date.now() + timeoutMs;
  let last;
  while (Date.now() < end) {
    try {
      last = await fn();
      if (last) return last;
    } catch (e) {
      last = e.message ?? e;
    }
    await sleep(250);
  }
  throw new Error(`timed out after ${timeoutMs} ms waiting for ${what} (last: ${typeof last === 'object' ? JSON.stringify(last) : last})`);
}

/** In-page snapshot of the connection, the model and the probe. */
const status = (cdp) => cdp.evaluate(() => {
  const p = window.__e2e;
  const s = window.evo?.model?.state;
  return {
    conn: document.querySelector('#conn-state')?.textContent ?? null,
    socketState: window.evo?.socket?.state ?? null,
    tick: s?.clock?.tick ?? null,
    day: s?.clock?.day ?? null,
    organisms: s?.organisms?.length ?? 0,
    opens: p?.opens ?? null,
    closes: p?.closes ?? null,
    frames: p?.frames ?? null,
    frameAgeMs: p?.lastFrameAt ? Date.now() - p.lastFrameAt : null,
    visibility: document.visibilityState,
    url: location.href,
  };
});

export async function runScenario(device, { outDir, backgroundSeconds = 25, name = `emu_${Date.now().toString(36)}` } = {}) {
  const report = { device: device.label, name, steps: [], errors: [], console: [] };
  const step = (msg, data) => {
    report.steps.push(data === undefined ? msg : { step: msg, ...data });
    console.log(`  ✓ ${msg}${data ? ` ${JSON.stringify(data)}` : ''}`);
  };
  let shotN = 0;
  let cdp = null;

  const attach = async () => {
    cdp?.close();
    cdp = device.connect
      ? await device.connect()
      : await connectPage(await device.endpoint(), { match: device.match, timeoutMs: 90000 });
    cdp.on('Runtime.consoleAPICalled', (e) => {
      const text = e.args.map((a) => a.value ?? a.description ?? a.type).join(' ');
      report.console.push(`[${e.type}] ${text}`);
    });
    cdp.on('Runtime.exceptionThrown', (e) => {
      const d = e.exceptionDetails;
      report.errors.push(d.exception?.description ?? d.text);
    });
    await cdp.send('Runtime.enable');
    await cdp.send('Page.enable');
    await cdp.send('Page.addScriptToEvaluateOnNewDocument', { source: PROBE });
    await cdp.evaluate(PROBE);
  };

  /** A screenshot of the page (CDP) and of the whole screen (the device's own, when it has one). */
  const shoot = async (label, { page = true } = {}) => {
    shotN += 1;
    const base = `${String(shotN).padStart(2, '0')}-${label}`;
    if (device.screenshot) {
      try {
        await device.screenshot(path.join(outDir, `${base}.png`));
      } catch (e) {
        console.log(`    (screen screenshot failed: ${e.message})`);
      }
    }
    if (!page) return null;
    for (let attempt = 0; attempt < 3; attempt++) {
      try {
        const { data } = await cdp.send('Page.captureScreenshot', { format: 'png' }, 30000);
        writeFileSync(path.join(outDir, `${base}-page.png`), Buffer.from(data, 'base64'));
        return data;
      } catch (e) {
        if (attempt === 2) console.log(`    (page screenshot failed: ${e.message})`);
        await sleep(1000);
      }
    }
    return null;
  };

  /** Where the element's center is now, and the viewport it was measured in. */
  const locate = (selector, scroll) => cdp.evaluate((sel, scrollIt) => {
    const el = document.querySelector(sel);
    if (!el) return null;
    let r = el.getBoundingClientRect();
    if (scrollIt || r.bottom < 0 || r.top > innerHeight) {
      el.scrollIntoView({ block: 'center', inline: 'center' });
      r = el.getBoundingClientRect();
    }
    const x = r.left + r.width / 2;
    const y = r.top + r.height / 2;
    const hit = document.elementFromPoint(x, y);
    const vv = window.visualViewport;
    return {
      x, y, onTop: !!hit && (hit === el || el.contains(hit)),
      view: [innerWidth, innerHeight, scrollX, scrollY, vv?.width ?? 0, vv?.height ?? 0, vv?.offsetTop ?? 0, vv?.scale ?? 1].map((v) => Math.round(v)),
    };
  }, selector, scroll);

  /**
   * Tap the element's center with real input events (falls back to el.click() when something else is on top).
   * Typing raises the soft keyboard and the blur here dismisses it: while it
   * animates the viewport resizes or scrolls (the login card is centered, the
   * page scrolls back), so a point measured then is somewhere else when the
   * tap lands (seen in CI: the tap missed Create account / Found lineage, or
   * hit the Name field). Measure until the element and the viewport have held
   * still for about a second, then tap.
   */
  const tap = async (selector) => {
    await cdp.evaluate(() => document.activeElement?.blur?.());
    let box = await locate(selector, true);
    if (!box) throw new Error(`no ${selector} to tap`);
    const same = (a, b) => !!a && !!b && Math.abs(a.x - b.x) < 1 && Math.abs(a.y - b.y) < 1 && a.view.join() === b.view.join();
    let still = 0;
    for (const end = Date.now() + 10000; still < 4 && Date.now() < end;) {
      await sleep(300);
      const next = await locate(selector, false);
      if (!next) throw new Error(`${selector} went away before the tap`);
      still = same(box, next) ? still + 1 : 0;
      box = next;
    }
    if (still < 4) console.log(`    (${selector} kept moving for 10 s; tapping where it is now)`);
    if (!box.onTop) {
      console.log(`    (${selector} is covered; clicking it directly)`);
      await cdp.evaluate((sel) => document.querySelector(sel).click(), selector);
      return;
    }
    // The input events the page sees during the tap, and whether the element got its click.
    const watch = () => cdp.evaluate((sel) => {
      const seen = (window.__e2eTap = { clicked: false, events: [] });
      const el = document.querySelector(sel);
      for (const t of ['pointerdown', 'mousedown', 'pointerup', 'mouseup', 'click']) {
        document.addEventListener(t, (e) => {
          if (window.__e2eTap !== seen) return;
          seen.events.push(`${t}:${e.target?.id || e.target?.tagName?.toLowerCase()}`);
          if (t === 'click' && (e.target === el || el?.contains(e.target))) seen.clicked = true;
        }, { capture: true });
      }
    }, selector);
    const record = { selector, ...box, tries: [] };
    (report.taps ??= []).push(record);
    // A synthetic press and release can come to nothing on the emulator's
    // WebView (CI: hover styling showed the pointer reached Create account,
    // but no click; the button never ran). Only input the page never turned
    // into a click is sent again: a click that arrived is never repeated.
    for (let attempt = 1; attempt <= 3; attempt++) {
      await watch();
      for (const type of ['mousePressed', 'mouseReleased']) {
        const r = await cdp.send('Input.dispatchMouseEvent', { type, x: box.x, y: box.y, button: 'left', buttons: type === 'mousePressed' ? 1 : 0, clickCount: 1 });
        if (r?.hit) record.hit = r.hit; // the iOS driver says what it clicked
      }
      let seen = null;
      for (const end = Date.now() + 2000; Date.now() < end;) {
        seen = await cdp.evaluate(() => window.__e2eTap ?? null).catch(() => null);
        if (!seen || seen.clicked) break; // clicked, or the page moved on (navigated)
        await sleep(200);
      }
      record.tries.push(seen?.events ?? 'page changed');
      if (!seen || seen.clicked) return;
      console.log(`    (tap on ${selector} produced no click, saw [${seen.events.join(' ')}]; tapping again)`);
      box = (await locate(selector, false)) ?? box;
    }
    throw new Error(`three taps on ${selector} produced no click`);
  };

  /** Focus a field, clear it and type `text` as keyboard input. */
  const type = async (selector, text) => {
    const ok = await cdp.evaluate((sel) => {
      const el = document.querySelector(sel);
      if (!el) return false;
      el.focus();
      el.select?.();
      return true;
    }, selector);
    if (!ok) throw new Error(`no ${selector} to type into`);
    await cdp.send('Input.insertText', { text });
    const value = await cdp.evaluate((sel) => document.querySelector(sel).value, selector);
    if (value !== text) {
      // An IME may have swallowed or mangled the text: set it the way a paste would.
      await cdp.evaluate((sel, t) => {
        const el = document.querySelector(sel);
        el.value = t;
        el.dispatchEvent(new Event('input', { bubbles: true }));
        el.dispatchEvent(new Event('change', { bubbles: true }));
      }, selector, text);
    }
  };

  /**
   * The map canvas shows more than a flat fill: count distinct colors on a
   * grid of the canvas's own pixels. Read from the canvas, not from a page
   * screenshot: the Android WebView's DevTools screenshot leaves the WebGL
   * canvas out (CI: the page screenshot was blank where the device's own
   * showed the map; a pass counted the toasts' text on top of it), and
   * overlays must not count as map. With preserveDrawingBuffer off, the
   * drawing buffer is readable only in the frame it was drawn: the copy is
   * made in an animation frame callback queued after the renderer's (Pixi
   * queues its next frame while drawing the current one), over a few frames.
   */
  const mapColors = () => cdp.evaluate(async () => {
    const c = document.querySelector('#world-canvas');
    const box = c?.getBoundingClientRect();
    if (!c || !box || box.width < 10 || box.height < 10 || c.width < 16 || c.height < 16) return { colors: 0, why: 'no canvas box' };
    const cv = document.createElement('canvas');
    cv.width = c.width;
    cv.height = c.height;
    const g = cv.getContext('2d', { willReadFrequently: true });
    let best = 0;
    for (let frame = 0; frame < 10 && best < 6; frame++) {
      await new Promise((r) => requestAnimationFrame(r));
      g.clearRect(0, 0, cv.width, cv.height);
      g.drawImage(c, 0, 0);
      const seen = new Set();
      for (let i = 1; i < 16; i++) {
        for (let j = 1; j < 16; j++) {
          const [r, gg, b] = g.getImageData(Math.floor((cv.width * i) / 16), Math.floor((cv.height * j) / 16), 1, 1).data;
          seen.add(`${r >> 3},${gg >> 3},${b >> 3}`);
        }
      }
      best = Math.max(best, seen.size);
    }
    return { colors: best, canvas: [Math.round(box.width), Math.round(box.height)] };
  });

  const save = () => writeFileSync(path.join(outDir, 'report.json'), JSON.stringify(report, null, 2));
  try {
    // 1. The app loads and reaches the login screen.
    await attach();
    await waitFor('the login screen', () => cdp.evaluate(() => !!document.querySelector('#login-name')), 90000);
    const login = await cdp.evaluate(() => ({
      url: location.href, native: !!window.Capacitor?.isNativePlatform?.(), serverBoxOpen: document.querySelector('#server-box')?.open ?? null,
      ua: navigator.userAgent, dpr: devicePixelRatio, viewport: [innerWidth, innerHeight],
    }));
    step('login screen', login);
    await shoot('login');

    // 2. Point it at the test server and register.
    await cdp.evaluate(() => { const b = document.querySelector('#server-box'); if (b) b.open = true; });
    await type('#server-origin', device.serverOrigin);
    await cdp.evaluate(() => document.querySelector('#server-origin').dispatchEvent(new Event('change', { bubbles: true })));
    await type('#login-name', name);
    await type('#login-password', 'a long password');
    await shoot('login-filled');
    await tap('#register-submit');

    // 3. The founder design: a new player has no lineage.
    await waitFor('the founder design screen', async () => {
      const r = await cdp.evaluate(() => ({ design: !!document.querySelector('#design-name'), error: document.querySelector('.error')?.textContent || '' }));
      if (r.error && !r.error.includes('busy')) throw new Error(`login screen error: ${r.error}`);
      return r.design;
    });
    step('registered, founder design shown', { server: device.serverOrigin, player: name });
    await shoot('design');
    await type('#design-name', `${name} line`);
    await tap('#design-submit');

    // 4. The map and the first state frames.
    await waitFor('the world map', async () => {
      const r = await cdp.evaluate(() => {
        const submit = document.querySelector('#design-submit');
        return {
          map: !!document.querySelector('#world-canvas'),
          design: !!submit,
          submitDisabled: submit?.disabled ?? null,
          problem: document.querySelector('#design-problem')?.textContent || '',
          serverError: document.querySelector('#design-server-error')?.textContent || '',
        };
      });
      if (r.map) return true;
      throw new Error(JSON.stringify(r)); // what the timeout reports
    });
    const first = await waitFor('a state frame with own organisms on a connected socket', async () => {
      const s = await status(cdp);
      return s.conn === 'connected' && s.organisms > 0 && s.tick > 0 && s.frames > 0 ? s : null;
    });
    step('map up, state frames arriving', first);
    if (await cdp.evaluate(() => !!document.querySelector('#tour'))) {
      await shoot('tour');
      await tap('#tour-skip');
      await waitFor('the tour to close', () => cdp.evaluate(() => !document.querySelector('#tour')), 15000);
      step('onboarding tour shown and skipped');
    }
    await sleep(2000); // a terrain fetch and a few frames
    await shoot('map');
    const m = await mapColors();
    if (m.colors < 6) throw new Error(`the map looks blank: ${JSON.stringify(m)}`);
    step('map renders', m);

    // 5. Background past the 20 s watchdog, resume, and recover.
    const before = await status(cdp);
    report.beforeBackground = before;
    await device.background();
    await sleep(1500);
    await shoot('background', { page: false });
    console.log(`    backgrounded; waiting ${backgroundSeconds} s`);
    await sleep(backgroundSeconds * 1000);
    await device.resume();
    const resumedAt = Date.now();
    // The app may have been restarted (process killed while away): attach again if the DevTools socket went away.
    try {
      await cdp.evaluate('1');
    } catch {
      console.log('    DevTools session lost while backgrounded; attaching again');
      await attach();
    }
    let lastSeen;
    const after = await waitFor('the socket to recover after resume', async () => {
      const s = (lastSeen = await status(cdp));
      if (s.opens === null) throw new Error('the page reloaded while backgrounded (probe gone)');
      return s.conn === 'connected' && s.tick > before.tick && s.frames > before.frames && s.frameAgeMs !== null && s.frameAgeMs < 3000 ? s : null;
    }, 30000).catch((e) => { report.lastStatus = lastSeen; throw e; });
    const recovery = {
      recoveredInMs: Date.now() - resumedAt,
      reconnects: after.opens - before.opens,
      ticks: after.tick - before.tick,
      events: (await cdp.evaluate(() => window.__e2e.events.slice(-30))).map(([t, ...e]) => [t, ...e].join(' ')),
    };
    report.afterResume = { ...after, ...recovery };
    step('socket recovered after resume', { recoveredInMs: recovery.recoveredInMs, reconnects: recovery.reconnects, ticks: recovery.ticks, conn: after.conn });
    await shoot('resumed');

    if (report.errors.length) throw new Error(`uncaught page errors:\n${report.errors.join('\n')}`);
    report.ok = true;
    return report;
  } catch (e) {
    report.ok = false;
    report.failure = e.message;
    if (cdp && !cdp.closed) {
      try {
        report.lastStatus ??= await status(cdp);
        report.probeEvents = await cdp.evaluate(() => window.__e2e?.events ?? null);
      } catch { /* page gone */ }
      await shoot('failure').catch(() => {});
    }
    throw e;
  } finally {
    save();
    cdp?.close();
  }
}
