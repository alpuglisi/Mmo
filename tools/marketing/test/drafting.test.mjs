import { test } from 'node:test';
import assert from 'node:assert/strict';
import { draftStory } from '../src/drafting/index.mjs';
import { validateDraft, unsupportedNumbers } from '../src/drafting/validate.mjs';
import { ALL_PLATFORMS, PLATFORMS } from '../src/platforms.mjs';
import { counters } from '../src/text.mjs';
import { factCorpus } from '../src/stories.mjs';
import { buildSchema, FALLBACK_BETA } from '../src/drafting/claude.mjs';
import { STORY, VIDEO, IMAGE, testConfig } from './helpers.mjs';

test('template drafts fit every platform limit for a long story', async () => {
  const config = testConfig();
  const { drafts, skipped } = await draftStory(STORY, [VIDEO, IMAGE], { platforms: ALL_PLATFORMS, config });
  assert.deepEqual(skipped, []);
  assert.equal(drafts.length, ALL_PLATFORMS.length);
  for (const d of drafts) {
    assert.ok(d.validation.ok, `${d.platform}: ${d.validation.errors}`);
    for (const [f, spec] of Object.entries(PLATFORMS[d.platform].fields)) {
      assert.ok(counters[spec.measure](d.fields[f]) <= spec.max, `${d.platform}.${f} too long`);
    }
    assert.equal(d.source, 'template');
  }
});

test('short posts carry a UTM link and few hashtags; captions have no link', async () => {
  const { drafts } = await draftStory(STORY, [VIDEO], { platforms: ['x', 'tiktok', 'reddit'], config: testConfig() });
  const x = drafts.find((d) => d.platform === 'x');
  assert.match(x.fields.text, /https:\/\/game\.example\/\?utm_source=x&utm_medium=social&utm_campaign=launch&utm_content=y3-winter/);
  assert.match(x.fields.text, /#EvolutionMMO/);
  const tt = drafts.find((d) => d.platform === 'tiktok');
  assert.doesNotMatch(tt.fields.caption, /https?:/);
  const r = drafts.find((d) => d.platform === 'reddit');
  assert.doesNotMatch(r.fields.title, /#/);
});

test('video platforms are skipped without a video', async () => {
  const { drafts, skipped } = await draftStory(STORY, [IMAGE], { platforms: ['tiktok', 'youtube', 'instagram', 'bluesky'], config: testConfig() });
  assert.deepEqual(skipped, ['tiktok', 'youtube', 'instagram']);
  assert.equal(drafts[0].media[0].type, 'image');
});

test('fact check flags numbers not in the story', () => {
  const corpus = factCorpus(STORY);
  assert.deepEqual(unsupportedNumbers('The herd fell from 1,240 to 610 in year 3.', corpus), []);
  assert.deepEqual(unsupportedNumbers('Over 5000 players watched 90% die.', corpus), ['5000', '90']);
  assert.deepEqual(unsupportedNumbers('Visit https://x.example/?a=999 #gamedev2', corpus), []);
});

test('validation catches limits, hashtags, banned words, angle brackets', () => {
  const config = testConfig();
  const long = validateDraft({ platform: 'bluesky', fields: { text: 'a'.repeat(301) } }, { config });
  assert.match(long.errors[0], /301 > 300/);
  const tags = validateDraft({ platform: 'x', fields: { text: 'hi #a #b #c' } }, { config });
  assert.ok(tags.errors.some((e) => /hashtags/.test(e)));
  const hype = validateDraft({ platform: 'x', fields: { text: 'The most revolutionary sim ever' } }, { config });
  assert.ok(hype.errors.some((e) => /banned/.test(e)));
  const yt = validateDraft({ platform: 'youtube', fields: { title: 'a <b>', description: 'ok' }, media: [{ type: 'video' }] }, { config });
  assert.ok(yt.errors.some((e) => /< and >/.test(e)));
  const nov = validateDraft({ platform: 'tiktok', fields: { caption: 'ok' }, media: [] }, { config });
  assert.ok(nov.errors.some((e) => /needs a video/.test(e)));
});

test('mastodon limit follows config.mastodon.maxChars', () => {
  const config = testConfig({ mastodon: { maxChars: 1000 } });
  assert.ok(validateDraft({ platform: 'mastodon', fields: { text: 'a'.repeat(800) } }, { config }).ok);
});

function fakeClient(responses) {
  const calls = [];
  return {
    calls,
    beta: {
      messages: {
        create: async (req) => {
          calls.push(req);
          const r = responses[Math.min(calls.length - 1, responses.length - 1)];
          if (r instanceof Error) throw r;
          return { model: req.model, stop_reason: r.stop_reason || 'end_turn', content: [{ type: 'text', text: JSON.stringify(r.copy ?? {}) }] };
        },
      },
    },
  };
}

test('Claude copy is used when it passes checks; request uses structured output and fallbacks', async () => {
  const client = fakeClient([{ copy: { x: { text: 'Year 3 brought a severe winter to the steppe. The herd went from 1,240 to 610.' }, bluesky: { text: 'Snow for weeks on the steppe in year 3.' } } }]);
  const { drafts } = await draftStory(STORY, [VIDEO], { platforms: ['x', 'bluesky'], config: testConfig(), client });
  assert.deepEqual(drafts.map((d) => d.source), ['claude', 'claude']);
  const req = client.calls[0];
  assert.equal(req.model, 'claude-opus-5-5');
  assert.deepEqual(req.betas, [FALLBACK_BETA]);
  assert.equal(req.fallbacks, 'default');
  assert.equal(req.output_config.format.type, 'json_schema');
  assert.deepEqual(req.output_config.format.schema.required, ['x', 'bluesky']);
  assert.match(req.messages[0].content, /"herd_before": 1240/);
  assert.match(req.messages[0].content, /x \(X\): text <= \d+ chars/);
});

test('Claude copy with invented facts is retried with feedback, then falls back to the template', async () => {
  const bad = { copy: { x: { text: 'Over 9000 creatures died in the worst winter ever.' } } };
  const client = fakeClient([bad, bad]);
  const { drafts } = await draftStory(STORY, [VIDEO], { platforms: ['x'], config: testConfig(), client });
  assert.equal(client.calls.length, 2);
  assert.match(client.calls[1].messages[0].content, /not found in the story data/);
  assert.equal(drafts[0].source, 'template');
  assert.ok(drafts[0].validation.ok);
  assert.match(drafts[0].validation.warnings[0], /Claude copy failed checks/);
});

test('Claude refusal or API error falls back to templates', async () => {
  for (const r of [{ stop_reason: 'refusal' }, new Error('overloaded')]) {
    const client = fakeClient([r]);
    const out = await draftStory(STORY, [VIDEO], { platforms: ['discord'], config: testConfig(), client });
    assert.equal(out.drafts[0].source, 'template');
    assert.equal(out.notes.length, 1);
  }
});

test('over-long Claude copy is truncated to fit rather than rejected', async () => {
  const client = fakeClient([{ copy: { bluesky: { text: 'The herd halved in year 3. ' + 'Snow lay on the steppe for weeks. '.repeat(20) } } }]);
  const { drafts } = await draftStory(STORY, [VIDEO], { platforms: ['bluesky'], config: testConfig(), client });
  assert.equal(drafts[0].source, 'claude');
  assert.ok(counters.graphemes(drafts[0].fields.text) <= 300);
});

test('schema has one strict object per platform', () => {
  const s = buildSchema(['reddit', 'youtube']);
  assert.deepEqual(s.properties.reddit.required, ['title', 'body']);
  assert.equal(s.properties.youtube.additionalProperties, false);
});
