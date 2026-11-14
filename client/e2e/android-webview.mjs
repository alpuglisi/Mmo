// Drive the installed Capacitor app on an Android emulator or device through
// its WebView's DevTools socket (enabled in debug builds) and adb: the app
// smoke scenario in e2e/android/scenario.mjs. Usually run by
// e2e/android-emulator.sh, which starts the server, installs the APK and
// launches the app first.
//
//   node e2e/android-webview.mjs
//
// Env:
//   ADB              adb binary (default: adb; ANDROID_SERIAL picks the device)
//   EVO_APP_SERVER   server origin as the app sees it (default http://10.0.2.2:8080,
//                    the emulator's address for the host's loopback)
//   EVO_APP_ID       application id (default org.evolutionmmo.client)
//   EVO_APP_PAGE     regex for the app page's URL among the DevTools targets
//                    (default ^https?://localhost/, the bundled page; set it for
//                    an EVO_CAP_LIVE_URL build)
//   CDP_PORT         local port to forward the DevTools socket to (default 9333)
//   BG_SECONDS       how long to stay in the background (default 25; > the 20 s watchdog)
//   SCREENSHOT_DIR   where screenshots and report.json go (default e2e/out/android)

import { execFile } from 'node:child_process';
import { mkdirSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { promisify } from 'node:util';
import { runScenario } from './android/scenario.mjs';

const run = promisify(execFile);
const here = path.dirname(fileURLToPath(import.meta.url));
const ADB = process.env.ADB ?? 'adb';
const APP = process.env.EVO_APP_ID ?? 'org.evolutionmmo.client';
const ACTIVITY = `${APP}/.MainActivity`;
const SERVER = process.env.EVO_APP_SERVER ?? 'http://10.0.2.2:8080';
const PAGE_RE = new RegExp(process.env.EVO_APP_PAGE ?? '^https?://localhost/');
const PORT = Number(process.env.CDP_PORT ?? 9333);
const BG_SECONDS = Number(process.env.BG_SECONDS ?? 25);
const outDir = path.resolve(process.env.SCREENSHOT_DIR ?? path.join(here, 'out', 'android'));
mkdirSync(outDir, { recursive: true });

async function adb(...args) {
  const { stdout } = await run(ADB, args, { maxBuffer: 64 * 1024 * 1024, encoding: 'buffer' });
  return stdout;
}

async function adbRetry(args, tries = 3) {
  for (let i = 1; ; i++) {
    try {
      return await adb(...args);
    } catch (e) {
      if (i >= tries) throw e;
      console.log(`    (adb ${args.join(' ')} failed, retrying: ${e.message.split('\n')[0]})`);
      await new Promise((r) => setTimeout(r, 2000));
    }
  }
}

async function appPid() {
  for (let i = 0; i < 60; i++) {
    const out = (await adb('shell', 'pidof', APP).catch(() => Buffer.from(''))).toString().trim();
    if (out) return out.split(/\s+/)[0];
    await new Promise((r) => setTimeout(r, 1000));
  }
  throw new Error(`${APP} is not running`);
}

let forwardedPid = null;
const device = {
  label: `android (${process.env.ANDROID_SERIAL ?? 'default adb device'})`,
  serverOrigin: SERVER,
  // The Capacitor page: https://localhost/ on Android.
  match: (t) => PAGE_RE.test(t.url),
  async endpoint() {
    const pid = await appPid();
    if (pid !== forwardedPid) {
      // Each WebView process serves DevTools on @webview_devtools_remote_<pid>.
      await adbRetry(['forward', `tcp:${PORT}`, `localabstract:webview_devtools_remote_${pid}`]);
      console.log(`    DevTools of pid ${pid} on 127.0.0.1:${PORT}`);
      forwardedPid = pid;
    }
    return `http://127.0.0.1:${PORT}`;
  },
  async screenshot(file) {
    writeFileSync(file, await adbRetry(['exec-out', 'screencap', '-p']));
  },
  async background() {
    await adbRetry(['shell', 'input', 'keyevent', 'KEYCODE_HOME']);
  },
  async resume() {
    // The launcher intent brings the existing task back (as tapping the icon would).
    await adbRetry(['shell', 'am', 'start', '-W', '-a', 'android.intent.action.MAIN', '-c', 'android.intent.category.LAUNCHER', '-n', ACTIVITY]);
  },
};

console.log(`Android WebView smoke: ${APP} against ${SERVER}, background ${BG_SECONDS} s, output in ${outDir}`);
try {
  const report = await runScenario(device, { outDir, backgroundSeconds: BG_SECONDS });
  console.log(`Android WebView smoke passed (${report.steps.length} steps; report in ${path.join(outDir, 'report.json')})`);
} catch (e) {
  console.error(`Android WebView smoke FAILED: ${e.message}`);
  process.exitCode = 1;
} finally {
  await adb('forward', '--remove', `tcp:${PORT}`).catch(() => {});
}
