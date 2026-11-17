// Frame-rate cap for the browser end-to-end scripts.
//
// Headless Chromium draws WebGL in software (SwiftShader): its GPU process
// takes about three cores while the client's frame loop runs at 60 fps, and
// that, not the server or the test steps, is what an e2e run spends its time
// on. The scripts assert states, layouts and drawings, never a frame rate,
// so pages get `requestAnimationFrame` capped at E2E_FPS frames a second
// (default 5; E2E_FPS=0 leaves it alone). Frames still come and every
// callback still runs, just less often. A step that checks what is drawn
// frame by frame lifts the cap with `window.__e2eFrameCap(0)` (and restores
// it with `__e2eFrameCap(FPS)`); the browser's own `requestAnimationFrame`
// stays reachable as `window.__e2eNativeRaf` for a measurement that needs
// real frame intervals.

export const FPS = Number.parseFloat(process.env.E2E_FPS ?? '5');

/** The init script: runs in every page (and frame) before its own scripts. */
function capFrames(fps) {
  if (!(fps > 0) || window.__e2eNativeRaf) return;
  const raf = window.requestAnimationFrame.bind(window);
  const caf = window.cancelAnimationFrame.bind(window);
  window.__e2eNativeRaf = raf;
  let period = 1000 / fps;
  // A check of what is drawn frame by frame (motion against the server) runs
  // at the browser's own rate: `__e2eFrameCap(0)` lifts the cap, `(fps)` sets it.
  window.__e2eFrameCap = (f) => {
    period = f > 0 ? 1000 / f : 0;
  };
  let nextFrame = 0;
  let ids = 0;
  // Our id → the browser's pending handle (a timeout, then an animation frame).
  const pending = new Map();
  window.requestAnimationFrame = (cb) => {
    const id = ++ids;
    if (period === 0) {
      pending.set(id, { frame: raf((t) => { pending.delete(id); cb(t); }) });
      return id;
    }
    // Every callback asked for before the next capped frame runs in that
    // frame, together, as it would in one browser frame.
    const now = performance.now();
    if (nextFrame <= now) nextFrame = now + period;
    const fire = () => {
      pending.set(id, { frame: raf((t) => { pending.delete(id); cb(t); }) });
    };
    pending.set(id, { timeout: setTimeout(fire, Math.max(0, nextFrame - now - 8)) });
    return id;
  };
  window.cancelAnimationFrame = (id) => {
    const p = pending.get(id);
    if (!p) return;
    pending.delete(id);
    if (p.timeout !== undefined) clearTimeout(p.timeout);
    if (p.frame !== undefined) caf(p.frame);
  };
}

/** Cap the frame rate of every page of `target` (a BrowserContext or Page). */
export async function capFrameRate(target) {
  if (FPS > 0) await target.addInitScript(capFrames, FPS);
  return target;
}

/** A browser whose new contexts and pages are frame-capped. */
export function cappedBrowser(browser) {
  const newContext = browser.newContext.bind(browser);
  browser.newContext = async (...args) => capFrameRate(await newContext(...args));
  // `browser.newPage` makes its own context: cap that one.
  browser.newPage = async (...args) => {
    const context = await newContext(...args);
    await capFrameRate(context);
    const page = await context.newPage();
    // As Playwright's own newPage: closing the page closes its context.
    const close = page.close.bind(page);
    page.close = async (...a) => {
      await close(...a);
      await context.close();
    };
    return page;
  };
  return browser;
}

// Device pixel ratio of the emulated phones. At a phone's own 2.6–3 a frame
// has 7–9 times the pixels of the same page at 1, all drawn in software; the
// phone checks measure layout in CSS pixels, taps and gestures, so they run
// at E2E_DPR (default 1). E2E_DPR=device keeps each device's own ratio (for
// full-resolution screenshots).
const DPR = process.env.E2E_DPR ?? '1';

/** `deviceScaleFactor` for an emulated device whose own ratio is `own`. */
export function pixelRatio(own) {
  if (DPR === 'device') return own;
  const r = Number.parseFloat(DPR);
  return r > 0 ? r : own;
}

/** A Playwright device descriptor for Chromium (the WebKit default of iPhones dropped), at E2E_DPR. */
export function chromiumDevice(devices, name) {
  const { defaultBrowserType: _ignored, ...d } = devices[name];
  return { ...d, deviceScaleFactor: pixelRatio(d.deviceScaleFactor) };
}
