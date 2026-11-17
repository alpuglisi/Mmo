import { test } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { Queue } from '../src/queue.mjs';
import { draftStory } from '../src/drafting/index.mjs';
import { publishDue } from '../src/publish.mjs';
import { scheduleApproved } from '../src/calendar.mjs';
import { collectMetrics, summarize, renderSummary } from '../src/analytics.mjs';
import { STORY, VIDEO, testConfig, tmpdir, mockFetch, has } from './helpers.mjs';

async function scheduled(platforms, configOver = {}, env = {}, media = [VIDEO]) {
  const config = testConfig(configOver, env);
  const q = new Queue(path.join(tmpdir(), 'q.json'));
  const { drafts } = await draftStory(STORY, media, { platforms, config });
  for (const d of drafts) q.approve(q.add(d).item.id, { config });
  scheduleApproved(q, config, new Date('2026-11-16T00:00:00Z'));
  return { q, config };
}

const later = new Date('2026-12-31T00:00:00Z');

test('dry-run publishes nothing and leaves the queue unchanged', async () => {
  const { q, config } = await scheduled(['discord', 'mastodon']);
  const before = JSON.stringify(q.items);
  const report = await publishDue(q, config, { live: false, now: later, fetch: () => assert.fail('network'), log: () => {} });
  assert.equal(report.length, 2);
  assert.ok(report.every((r) => r.result && !r.error), JSON.stringify(report));
  assert.equal(JSON.stringify(q.items), before);
});

test('live: posts due items and records the result; second run finds nothing due', async () => {
  const env = { DISCORD_WEBHOOK_URL: 'https://discord.com/api/webhooks/1/t' };
  const { q, config } = await scheduled(['discord'], {}, env, []);
  const f = mockFetch([[has('discord.com'), () => ({ json: { id: 'D9' } })]]);
  const r1 = await publishDue(q, config, { live: true, now: later, fetch: f, sleep: async () => {}, log: () => {} });
  assert.equal(r1[0].result.remoteId, 'D9');
  assert.equal(q.items[0].status, 'posted');
  assert.equal(q.items[0].result.postedAt, later.toISOString());
  const r2 = await publishDue(q, config, { live: true, now: later, fetch: f, log: () => {} });
  assert.equal(r2.length, 0);
  assert.equal(f.calls.length, 1);
});

test('live: missing credentials mark the item failed, nothing sent', async () => {
  const { q, config } = await scheduled(['discord']);
  const f = mockFetch([]);
  const r = await publishDue(q, config, { live: true, now: later, fetch: f, log: () => {} });
  assert.match(r[0].error, /DISCORD_WEBHOOK_URL/);
  assert.equal(q.items[0].status, 'failed');
  assert.equal(f.calls.length, 0);
});

test('items not yet due are not published', async () => {
  const { q, config } = await scheduled(['discord']);
  const r = await publishDue(q, config, { live: false, now: new Date('2026-11-01T00:00:00Z'), log: () => {} });
  assert.equal(r.length, 0);
});

test('TikTok and Instagram without an approved app go to manual export', async () => {
  const { q, config } = await scheduled(['tiktok', 'instagram', 'devlog']);
  const r = await publishDue(q, config, { live: true, now: later, fetch: () => assert.fail('network'), log: () => {} });
  assert.equal(r.length, 3);
  for (const e of r) {
    assert.ok(e.exported, JSON.stringify(e));
    assert.ok(fs.existsSync(path.join(e.exported, 'README.md')));
  }
  assert.ok(fs.readFileSync(path.join(r[0].exported, 'caption.txt'), 'utf8').length > 10);
  assert.ok(fs.existsSync(path.join(r[2].exported, 'post.md')));
  assert.ok(q.items.every((i) => i.status === 'exported'));
});

test('a rate limit stops further posts to that platform in the run', async () => {
  const env = { DISCORD_WEBHOOK_URL: 'https://discord.com/api/webhooks/1/t' };
  const config = testConfig({ dailyCaps: { discord: 10 } }, env);
  const q = new Queue(path.join(tmpdir(), 'q.json'));
  for (const id of ['a', 'b']) {
    const s = { ...STORY, id, title: `${STORY.title} ${id}` };
    const { drafts } = await draftStory(s, [], { platforms: ['discord'], config });
    const it = q.add(drafts[0]).item;
    q.approve(it.id, { config });
  }
  const f = mockFetch([[has('discord.com'), () => ({ status: 429, headers: { 'retry-after': '3600' }, json: {} })]]);
  const r = await publishDue(q, config, { live: true, ids: q.items.map((i) => i.id), now: later, fetch: f, sleep: async () => {}, log: () => {} });
  assert.match(r[0].error, /rate limited/);
  assert.match(r[1].skipped, /rate-limited earlier/);
  assert.equal(f.calls.length, 1);
});

test('metrics and summary', async () => {
  const env = { MASTODON_INSTANCE: 'm.example', MASTODON_ACCESS_TOKEN: 't' };
  const { q, config } = await scheduled(['mastodon'], {}, env);
  q.mark(q.items[0].id, 'posted', { result: { remoteId: '42', url: 'https://m.example/@me/42', postedAt: later.toISOString() } });
  const f = mockFetch([[has('/api/v1/statuses/42'), () => ({ json: { favourites_count: 7, reblogs_count: 2, replies_count: 1 } })]]);
  const snaps = await collectMetrics(q, config, { live: true, fetch: f, now: later });
  assert.deepEqual(snaps[0].metrics, { likes: 7, reposts: 2, replies: 1 });
  const s = summarize(q, snaps);
  assert.equal(s.byPlatform.mastodon.likes, 7);
  assert.equal(s.top[0].engagement, 7 + 4 + 2);
  assert.match(renderSummary(s), /\| mastodon \| 1 \|/);
  assert.ok(fs.existsSync(path.join(config.dataDir, 'metrics.json')));
});
