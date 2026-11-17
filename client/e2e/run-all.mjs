// Runs the browser end-to-end scripts (`npm run e2e`). Each script starts its
// own evo-server on a free port with its own browser, so they are
// independent: E2E_JOBS of them run at once (default half the cores, at most
// 4; E2E_JOBS=1 runs them one after another). The pages run at a capped
// frame rate and the phones at pixel ratio 1 (frames.mjs: E2E_FPS, E2E_DPR),
// which is what makes running several at once pay: uncapped, Chromium's
// software WebGL alone kept three cores busy. With more than one job a
// script's output is printed in one piece when it finishes. Every script runs
// even after one fails; the run then fails and lists the failures, with each
// script's time.
//
// Usage: node e2e/run-all.mjs [script ...]   (names without .mjs; default all)

import { spawn } from 'node:child_process';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const here = path.dirname(fileURLToPath(import.meta.url));
// Longest first, so a parallel run does not end waiting on a long one.
const ALL = ['smoke', 'mobile', 'android-desktop', 'onboarding', 'vision', 'family', 'refound', 'builder', 'context-loss', 'push', 'lobby'];
const scripts = process.argv.length > 2 ? process.argv.slice(2) : ALL;
const cores = os.availableParallelism?.() ?? os.cpus().length;
const jobs = Math.max(1, Number.parseInt(process.env.E2E_JOBS ?? '', 10) || Math.min(4, Math.floor(cores / 2)));

function run(name) {
  return new Promise((resolve) => {
    const started = Date.now();
    const child = spawn(process.execPath, [path.join(here, `${name}.mjs`)], { stdio: ['ignore', 'pipe', 'pipe'] });
    const out = [];
    child.stdout.on('data', (d) => (jobs === 1 ? process.stdout.write(d) : out.push(d)));
    child.stderr.on('data', (d) => (jobs === 1 ? process.stderr.write(d) : out.push(d)));
    child.on('close', (code, signal) => {
      const seconds = (Date.now() - started) / 1000;
      if (jobs > 1) {
        process.stdout.write(`\n── e2e/${name}.mjs ──\n`);
        process.stdout.write(Buffer.concat(out));
      }
      resolve({ name, ok: code === 0, code: code ?? signal, seconds });
    });
  });
}

const started = Date.now();
const queue = [...scripts];
const results = [];
await Promise.all(
  Array.from({ length: Math.min(jobs, queue.length) }, async () => {
    for (let name = queue.shift(); name !== undefined; name = queue.shift()) results.push(await run(name));
  }),
);
const wall = (Date.now() - started) / 1000;
console.log(`\ne2e: ${results.length} scripts, ${jobs} at a time, ${wall.toFixed(0)} s`);
for (const r of results) console.log(`  ${r.ok ? 'ok  ' : 'FAIL'} ${r.name.padEnd(16)} ${r.seconds.toFixed(0).padStart(4)} s${r.ok ? '' : ` (exit ${r.code})`}`);
const failed = results.filter((r) => !r.ok);
if (failed.length > 0) {
  console.error(`e2e failed: ${failed.map((r) => r.name).join(', ')}`);
  process.exit(1);
}
