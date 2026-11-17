import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { DEFAULTS, deepMerge } from '../src/config.mjs';
import { normalizeStory, normalizeMediaItem } from '../src/stories.mjs';

export function tmpdir() {
  return fs.mkdtempSync(path.join(os.tmpdir(), 'evo-mkt-'));
}

export function testConfig(over = {}, env = {}) {
  const dir = tmpdir();
  const cfg = deepMerge(DEFAULTS, { dataDir: path.join(dir, 'data'), outDir: path.join(dir, 'out'), site: { url: 'https://game.example', campaign: 'launch' }, ...over });
  cfg.env = env;
  return cfg;
}

export const STORY = normalizeStory({
  id: 'y3-winter',
  kind: 'catastrophe',
  title: 'A severe winter hit the northern steppe in year 3',
  summary:
    'Snow lay for weeks. The hibernating grazers that had fattened in autumn came through, while the rest of the herd halved from 1,240 to 610 animals. ' +
    'Predators followed the weakened herd south, and by spring the steppe belonged to the burrowers. '.repeat(6),
  facts: { year: 3, herd_before: 1240, herd_after: 610 },
});

export const VIDEO = normalizeMediaItem({ id: 'v1', path: '/nonexistent/winter.mp4', type: 'video', duration_s: 40, width: 1080, height: 1920, story_id: 'y3-winter', alt: 'Snow spreads over the steppe' });
export const IMAGE = normalizeMediaItem({ id: 'c1', path: '/nonexistent/card.png', type: 'card', width: 1200, height: 675, story_id: 'y3-winter', alt: 'Share card' });

/** A fetch mock: routes is [ [matcher(url, init) => bool, responder(url, init) => {status,json,headers}] ]. */
export function mockFetch(routes) {
  const calls = [];
  const fn = async (url, init = {}) => {
    calls.push({ url, init });
    for (const [match, respond] of routes) {
      if (match(url, init)) {
        const r = respond(url, init);
        const h = new Headers(r.headers || {});
        if (r.json !== undefined) h.set('content-type', 'application/json');
        return new Response(r.json === undefined ? null : JSON.stringify(r.json), { status: r.status || 200, headers: h });
      }
    }
    throw new Error(`unmocked ${init.method || 'GET'} ${url}`);
  };
  fn.calls = calls;
  return fn;
}

export const has = (s) => (url) => url.includes(s);
