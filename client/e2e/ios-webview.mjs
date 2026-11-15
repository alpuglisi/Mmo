// Drive the installed Capacitor app in an iOS simulator through its
// WKWebView: the app smoke scenario in e2e/android/scenario.mjs, over Appium
// (XCUITest driver) and e2e/ios/webdriver.mjs. Usually run by
// e2e/ios-simulator.sh, which boots the simulator, starts the server and
// Appium, and installs the app first.
//
//   node e2e/ios-webview.mjs
//
// Env:
//   SIM_UDID         the booted simulator (required)
//   APPIUM_URL       Appium server (default http://127.0.0.1:4723)
//   EVO_APP_SERVER   server origin as the app sees it (default http://127.0.0.1:8080:
//                    the simulator shares the Mac's network, loopback included)
//   EVO_APP_ID       bundle id (default org.evolutionmmo.client)
//   EVO_APP_PAGE     regex for the app page's URL (default ^capacitor://localhost)
//   EVO_IOS_NATIVE_TAP=1  tap buttons through XCUITest at their coordinates
//                    instead of dispatching the click in the page
//   WDA_DERIVED_DATA WebDriverAgent's derived data (cached in CI: a rebuild is
//                    then incremental)
//   BG_SECONDS       how long to stay in the background (default 25; > the 20 s watchdog)
//   SCREENSHOT_DIR   where screenshots and report.json go (default e2e/out/ios)
//   XCRUN            xcrun binary (default xcrun)

import { execFile } from 'node:child_process';
import { copyFileSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { promisify } from 'node:util';
import { runScenario } from './android/scenario.mjs';
import { WebDriver, WebViewSession } from './ios/webdriver.mjs';

const run = promisify(execFile);
const here = path.dirname(fileURLToPath(import.meta.url));
const XCRUN = process.env.XCRUN ?? 'xcrun';
const UDID = process.env.SIM_UDID;
const APP = process.env.EVO_APP_ID ?? 'org.evolutionmmo.client';
const SERVER = process.env.EVO_APP_SERVER ?? 'http://127.0.0.1:8080';
const PAGE_RE = new RegExp(process.env.EVO_APP_PAGE ?? '^capacitor://localhost');
const APPIUM = process.env.APPIUM_URL ?? 'http://127.0.0.1:4723';
const BG_SECONDS = Number(process.env.BG_SECONDS ?? 25);
const NATIVE_TAP = process.env.EVO_IOS_NATIVE_TAP === '1';
const outDir = path.resolve(process.env.SCREENSHOT_DIR ?? path.join(here, 'out', 'ios'));
mkdirSync(outDir, { recursive: true });
if (!UDID) throw new Error('SIM_UDID is not set (the booted simulator)');

const simctl = (...args) => run(XCRUN, ['simctl', ...args], { maxBuffer: 64 * 1024 * 1024 });
const tmp = mkdtempSync(path.join(os.tmpdir(), 'evo-ios-shot-'));

// While the game screen renders (WebGL, no GPU in CI's simulator) one
// simulator screenshot took 3 to 6 minutes (CI: the smoke passed but ran past
// the job's limit). Screenshots are for people reading the artifacts, so a
// slow one is given up, and the page screenshot that follows a screen
// screenshot (the same image on iOS) reuses it instead of taking another.
const SHOT_MS = 45000;
let lastShot = { file: null, at: 0 };

/** The simulator's screen as PNG (whole screen, device pixels). */
async function screenPng(file) {
  try {
    await run(XCRUN, ['simctl', 'io', UDID, 'screenshot', '--type=png', file], { timeout: SHOT_MS, maxBuffer: 64 * 1024 * 1024 });
    lastShot = { file, at: Date.now() };
  } catch (e) {
    lastShot = { file: null, at: Date.now() };
    throw e.killed ? new Error(`simulator screenshot took over ${SHOT_MS / 1000} s`) : e;
  }
}

/** A screen PNG as base64 for the scenario's page screenshot, scaled down when `sips` is there (it is on macOS). */
let shotSeq = 0;
async function pageShot() {
  const file = path.join(tmp, `page-${++shotSeq}.png`);
  if (Date.now() - lastShot.at < 10000) {
    // A screen screenshot was just taken (or just given up): use it.
    if (!lastShot.file) throw new Error('the simulator screenshot just before failed');
    copyFileSync(lastShot.file, file);
  } else {
    await screenPng(file);
  }
  try {
    await run('sips', ['--resampleWidth', '600', file, '--out', file]);
  } catch { /* full size then */ }
  return readFileSync(file).toString('base64');
}

const driver = new WebDriver(APPIUM);
const caps = {
  platformName: 'iOS',
  'appium:automationName': 'XCUITest',
  'appium:udid': UDID,
  'appium:bundleId': APP,
  // The script installed a fresh copy; keep it as it is and launch it.
  'appium:noReset': true,
  'appium:newCommandTimeout': 600,
  'appium:wdaLaunchTimeout': 600000,
  'appium:wdaConnectionTimeout': 600000,
  'appium:simulatorStartupTimeout': 300000,
  'appium:fullContextList': true,
  'appium:includeSafariInWebviews': false,
  // The Web Inspector lists the app's page under its process name
  // ("process-App", from the Xcode target App), not the bundle id, and
  // Appium matches only bundle ids it knows (seen in CI: the page
  // capacitor://localhost was listed, but only NATIVE_APP returned).
  'appium:additionalWebviewBundleIds': ['process-App', APP],
  'appium:webviewConnectTimeout': 60000,
  ...(process.env.WDA_DERIVED_DATA ? { 'appium:derivedDataPath': process.env.WDA_DERIVED_DATA } : {}),
};

/** Run `fn` in the native context and switch back to `web` (if given) afterwards. */
async function native(fn, web) {
  await driver.setContext('NATIVE_APP');
  try {
    return await fn();
  } finally {
    if (web) await driver.setContext(web).catch((e) => console.log(`    (back to ${web} failed: ${e.message})`));
  }
}

let current = null;
const device = {
  label: `ios simulator ${UDID}`,
  serverOrigin: SERVER,
  async connect() {
    current = await WebViewSession.attach(driver, { pageRe: PAGE_RE, timeoutMs: 90000, screenshot: pageShot, nativeTap: NATIVE_TAP });
    console.log(`    web view ${current.context}${current.url ? ` (${current.url})` : ''}`);
    return current;
  },
  async screenshot(file) {
    await screenPng(file);
  },
  async background() {
    // The home button (XCUITest), as a person would; simctl can only launch
    // another app, which is the fallback.
    try {
      await native(() => driver.execute('mobile: pressButton', [{ name: 'home' }]));
    } catch (e) {
      console.log(`    (home button failed: ${e.message}; launching Settings instead)`);
      await simctl('launch', UDID, 'com.apple.Preferences');
    }
  },
  async resume() {
    try {
      await native(() => driver.execute('mobile: activateApp', [{ bundleId: APP }]), current?.context);
    } catch (e) {
      console.log(`    (activateApp failed: ${e.message}; simctl launch instead)`);
      await simctl('launch', UDID, APP);
    }
  },
};

console.log(`iOS WebView smoke: ${APP} on ${UDID} against ${SERVER}, background ${BG_SECONDS} s, output in ${outDir}`);
const started = Date.now();
try {
  await driver.status().catch((e) => { throw new Error(`Appium is not answering at ${APPIUM}: ${e.message}`); });
  console.log('    starting the Appium session (builds WebDriverAgent the first time)');
  await driver.createSession(caps);
  console.log(`    session ${driver.sessionId} in ${Math.round((Date.now() - started) / 1000)} s`);
  const report = await runScenario(device, { outDir, backgroundSeconds: BG_SECONDS, name: `ios_${Date.now().toString(36)}` });
  console.log(`iOS WebView smoke passed (${report.steps.length} steps; report in ${path.join(outDir, 'report.json')})`);
} catch (e) {
  console.error(`iOS WebView smoke FAILED: ${e.message}`);
  try {
    writeFileSync(path.join(outDir, 'contexts.json'), JSON.stringify(await driver.contexts(), null, 2));
  } catch { /* no session */ }
  process.exitCode = 1;
} finally {
  await driver.deleteSession();
  rmSync(tmp, { recursive: true, force: true });
}
