import { test } from 'node:test';
import assert from 'node:assert/strict';
import { graphemes, xWeightedLength, mastodonLength, truncateTo, extractHashtags, codePoints } from '../src/text.mjs';

test('graphemes count emoji clusters as one', () => {
  assert.equal(graphemes('a👩‍👩‍👧b'), 3);
  assert.equal(graphemes('é'), 1);
});

test('X weighted length: URLs are 23, CJK doubles', () => {
  assert.equal(xWeightedLength('hello'), 5);
  assert.equal(xWeightedLength('see https://example.com/a/very/long/path?with=query&and=more'), 4 + 23);
  assert.equal(xWeightedLength('進化'), 4);
});

test('Mastodon counts every URL as 23', () => {
  assert.equal(mastodonLength('x https://example.com/' + 'a'.repeat(100)), 2 + 23);
});

test('truncateTo respects the limit, cuts on words, prefers sentence ends', () => {
  const s = 'The herd crossed the river. Then the winter came and the snow lay for many weeks across the plain';
  const t = truncateTo(s, 60);
  assert.ok(codePoints(t) <= 60);
  assert.equal(t, 'The herd crossed the river.');
  const u = truncateTo('one two three four five six seven', 20);
  assert.ok(codePoints(u) <= 20);
  assert.ok(u.endsWith('…'));
  assert.equal(truncateTo('short', 20), 'short');
  assert.ok(graphemes(truncateTo('x'.repeat(50), 10, graphemes)) <= 10);
});

test('extractHashtags ignores URL fragments and numbers', () => {
  assert.deepEqual(extractHashtags('hi #gamedev and #EvolutionMMO, not https://a.b/#frag or #123'), ['gamedev', 'EvolutionMMO']);
});
