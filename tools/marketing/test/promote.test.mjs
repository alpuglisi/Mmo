import { test } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { writeOutreach, writePressKit, suggestFor } from '../src/promote.mjs';
import { utmLink, parseUtm } from '../src/utm.mjs';
import { loadStories, loadManifest, normalizeStory, mediaForStory } from '../src/stories.mjs';
import { STORY, VIDEO, testConfig } from './helpers.mjs';

test('UTM scheme', () => {
  const u = utmLink('https://game.example/play?ref=1', { source: 'reddit', campaign: 'launch', content: 'Y3 Severe Winter!' });
  assert.deepEqual(parseUtm(u), { utm_source: 'reddit', utm_medium: 'community', utm_campaign: 'launch', utm_content: 'y3-severe-winter-' });
  assert.match(u, /ref=1/);
});

test('outreach drafts require a personal reason and are capped per run', () => {
  const config = testConfig();
  const creators = [
    { name: 'A Creator', channel: 'YouTube', why: 'Your series on artificial life sims explains the mechanics.' },
    { name: 'Blast', why: 'great channel' },
    ...Array.from({ length: 3 }, (_, i) => ({ name: `C${i}`, why: 'You covered evolution simulators in depth last month.' })),
  ];
  const r = writeOutreach({ config, creators, stories: [STORY], outDir: config.outDir, max: 2 });
  assert.equal(r.written.length, 2);
  assert.equal(r.refused.length, 3);
  assert.match(r.refused[0], /specific "why"/);
  const text = fs.readFileSync(r.written[0], 'utf8');
  assert.match(text, /Hi A Creator/);
  assert.match(text, /utm_source=outreach/);
  assert.match(text, /won't follow up more than once/);
});

test('press kit marks unknown facts as TBD rather than inventing them', () => {
  const config = testConfig();
  const dir = writePressKit({ config, stories: [STORY], manifest: [VIDEO], outDir: config.outDir });
  const md = fs.readFileSync(path.join(dir, 'README.md'), 'utf8');
  assert.match(md, /Release date:\*\* _TBD/);
  assert.match(md, /A severe winter/);
});

test('community and hashtag suggestions fit the story kind', () => {
  const s = suggestFor(normalizeStory({ id: 's', kind: 'speciation', title: 'A new species split off' }));
  assert.ok(s.communities.some((c) => c.name === 'r/alife'));
  assert.equal(s.hashtags.x[0], 'EvolutionMMO');
});

test('example inputs load; media matched to stories', () => {
  const here = path.dirname(new URL(import.meta.url).pathname);
  const stories = loadStories(path.join(here, '../examples/stories.example.json'));
  const manifest = loadManifest(path.join(here, '../examples/manifest.example.json'));
  assert.equal(stories.length, 2);
  assert.equal(mediaForStory(stories[0], manifest)[0].type, 'video');
  assert.equal(mediaForStory(stories[0], manifest)[0].vertical, true);
  assert.equal(mediaForStory(stories[1], manifest)[0].type, 'image');
});

test('only opted-in players can be named', () => {
  const s = normalizeStory({ id: 'p', title: 't', players: [{ name: 'Ann', optedIn: true }, { name: 'Bob' }] });
  assert.deepEqual(s.players, ['Ann']);
});
