// capture.mjs's manifest.json and stories.json read back through the
// publisher's own loaders (PUBLISHING.md, "Inputs").
import { test } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { boomBust, catastropheStory, frameSize, outputs, peakAndLow, sceneItems, sceneStory } from '../capture-manifest.mjs';
import { factCorpus, loadManifest, loadStories, mediaForStory } from '../src/stories.mjs';

const VERTICAL = { viewport: { width: 540, height: 960 }, scale: 2, gifWidth: 360 };
const SQUARE = { viewport: { width: 600, height: 600 }, scale: 1.8, gifWidth: 400 };

test('frame sizes are 1080 wide in both formats', () => {
  assert.deepEqual(frameSize(VERTICAL), { width: 1080, height: 1920 });
  assert.deepEqual(frameSize(SQUARE), { width: 1080, height: 1080 });
});

test('the publisher reads the manifest and the stories, each story with its media', () => {
  const scene = { id: 'evolution', kind: 'lineage', alt: 'A species spreading', subject: 'Glimmer striders', hashtags: ['#EvolutionMMO'] };
  const recordings = [
    { format: VERTICAL, formatName: 'vertical', files: { mp4: 'evolution-vertical.mp4', gif: 'evolution-vertical.gif' } },
    { format: SQUARE, formatName: 'square', files: { gif: 'evolution-square.gif', frames: 'frames/evolution-square' } },
  ];
  const items = sceneItems(scene, recordings, { portrait: 'evolution-portrait.png', 'family-tree': 'evolution-family-tree.png' }, 12);
  const measured = {
    title: 'Glimmer striders: from 40 founders to 312 animals in 9 game years',
    summary: 'A three-eyed design founded with 40 animals; 9 game years later 312 of them live.',
    facts: { lineage: 'Glimmer striders', founders: 40, alive: 312, years: 9 },
  };
  const story = sceneStory(scene, measured, items);
  const out = outputs({ generatedAt: '2026-11-17T00:00:00.000Z', fps: 12, ffmpeg: true, stories: [story], items });

  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'capture-'));
  fs.writeFileSync(path.join(dir, 'manifest.json'), JSON.stringify(out.manifest));
  fs.writeFileSync(path.join(dir, 'stories.json'), JSON.stringify(out.stories));
  const manifest = loadManifest(path.join(dir, 'manifest.json'));
  const stories = loadStories(path.join(dir, 'stories.json'));

  assert.deepEqual(manifest.map((m) => [m.id, m.type, m.relPath]), [
    ['evolution-vertical', 'video', 'evolution-vertical.mp4'],
    ['evolution-vertical-gif', 'image', 'evolution-vertical.gif'],
    ['evolution-square-gif', 'image', 'evolution-square.gif'],
    ['evolution-portrait', 'image', 'evolution-portrait.png'],
    ['evolution-family-tree', 'image', 'evolution-family-tree.png'],
  ]);
  const video = manifest[0];
  assert.equal(video.vertical, true);
  assert.equal(video.durationS, 12);
  assert.equal(video.storyId, 'evolution');
  assert.ok(video.alt.includes('vertical 9:16'));
  assert.deepEqual([manifest[2].width, manifest[2].height], [400, 400]);
  assert.deepEqual([manifest[4].width, manifest[4].height], [1080, 1920]);

  assert.equal(stories.length, 1);
  const s = stories[0];
  assert.equal(s.kind, 'lineage');
  assert.equal(s.title, measured.title);
  assert.deepEqual(s.players, []);
  assert.equal(mediaForStory(s, manifest).length, 5);
  // Every number in the title and summary is a fact the copy may cite.
  const corpus = factCorpus(s);
  for (const n of `${measured.title} ${measured.summary}`.match(/\d+/g)) assert.ok(corpus.includes(n), n);
});

test('boom and bust is told only when the population really fell', () => {
  assert.deepEqual(peakAndLow([3, 9, 4, 6]), { peak: 9, peakAt: 1, low: 4, lowAt: 2, last: 6 });
  const days = [0, 24, 48, 72, 96];
  const bust = boomBust('Red fox', days, [6, 30, 41, 12, 15], 24);
  assert.equal(bust.title, 'Boom and bust: the Red fox');
  assert.deepEqual([bust.facts.peak, bust.facts.peak_year, bust.facts.low_after_peak, bust.facts.low_year], [41, 3, 12, 4]);
  const steady = boomBust('Red fox', days, [6, 10, 12, 11, 12], 24);
  assert.equal(steady.title, 'The Red fox over 5 game years');
  for (const st of [bust, steady]) {
    const facts = Object.values(st.facts).join(' ');
    for (const n of `${st.title} ${st.summary}`.match(/\d+/g)) assert.ok(facts.split(/\D+/).includes(n), `${n} in ${st.summary}`);
  }
});

test('the staged catastrophe story says it was triggered and follows the numbers', () => {
  const grew = catastropheStory({ animals: 232, lineages: 5, year: 1, day: 8 }, { animals: 416, lineages: 5 }, 24);
  assert.equal(grew.title, 'A severe winter, and the world keeps growing');
  assert.match(grew.summary, /triggered for this capture/);
  assert.match(grew.summary, /still grew from 232 animals to 416/);
  const fell = catastropheStory({ animals: 400, lineages: 5, year: 2, day: 3 }, { animals: 150, lineages: 3 }, 30);
  assert.equal(fell.title, 'A severe winter sweeps the map');
  assert.match(fell.summary, /from 400 animals to 150, and 2 of 5 lineages died out/);
});
