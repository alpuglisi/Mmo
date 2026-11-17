import { test } from 'node:test';
import assert from 'node:assert/strict';
import path from 'node:path';
import { Queue } from '../src/queue.mjs';
import { draftStory } from '../src/drafting/index.mjs';
import { createReviewServer } from '../src/review-server.mjs';
import { STORY, testConfig, tmpdir } from './helpers.mjs';

test('review page lists drafts; writes need the per-run token', async () => {
  const config = testConfig();
  const q = new Queue(path.join(tmpdir(), 'q.json'));
  const { drafts } = await draftStory(STORY, [], { platforms: ['x', 'bluesky'], config });
  for (const d of drafts) q.add(d);
  let saves = 0;
  const { server, token } = createReviewServer({ queue: q, config, save: () => saves++ });
  await new Promise((r) => server.listen(0, '127.0.0.1', r));
  const base = `http://127.0.0.1:${server.address().port}`;
  try {
    const html = await (await fetch(base + '/')).text();
    assert.match(html, /Draft review/);
    assert.match(html, /<textarea name="text"/);
    assert.doesNotMatch(html, /<script>alert/);
    const id = q.items[0].id;
    const denied = await fetch(`${base}/api/${id}/approve`, { method: 'POST', body: '{}' });
    assert.equal(denied.status, 403);
    const ok = await fetch(`${base}/api/${id}/approve`, { method: 'POST', headers: { 'x-review-token': token }, body: '{}' });
    assert.equal((await ok.json()).status, 'approved');
    const ed = await fetch(`${base}/api/${q.items[1].id}/edit`, { method: 'POST', headers: { 'x-review-token': token }, body: JSON.stringify({ fields: { text: '<script>alert(1)</script> year 3' } }) });
    assert.equal((await ed.json()).status, 'pending');
    assert.doesNotMatch(await (await fetch(base + '/')).text(), /<script>alert/);
    assert.equal(saves, 2);
  } finally {
    server.close();
  }
});
