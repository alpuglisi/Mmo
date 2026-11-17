import { test } from 'node:test';
import assert from 'node:assert/strict';
import path from 'node:path';
import { Queue, resolveRedditTarget } from '../src/queue.mjs';
import { draftStory } from '../src/drafting/index.mjs';
import { preflight } from '../src/publish.mjs';
import { STORY, VIDEO, testConfig, tmpdir } from './helpers.mjs';

async function setup(platforms = ['x', 'reddit'], over = {}) {
  const config = testConfig({ reddit: { username: 'owner', ownSubreddits: ['EvolutionMMO'], defaultTarget: 'EvolutionMMO' }, ...over });
  const q = new Queue(path.join(tmpdir(), 'queue.json'));
  const { drafts } = await draftStory(STORY, [VIDEO], { platforms, config });
  const items = drafts.map((d) => q.add(d).item);
  return { q, config, items };
}

test('add, save and reload round-trip', async () => {
  const { q, items } = await setup();
  q.save();
  const q2 = new Queue(q.file);
  assert.equal(q2.items.length, 2);
  assert.equal(q2.get(items[0].id.slice(0, 4)).status, 'pending');
});

test('duplicates are refused: same story and platform, or same content', async () => {
  const { q, config } = await setup(['x']);
  const { drafts } = await draftStory(STORY, [VIDEO], { platforms: ['x'], config });
  assert.equal(q.add(drafts[0]).added, false);
  assert.equal(q.add({ ...drafts[0], storyId: 'other' }).added, false, 'same content hash');
  assert.equal(q.add(drafts[0], { force: true }).added, true);
});

test('rejected items do not block a new draft of the same story', async () => {
  const { q, config, items } = await setup(['x']);
  q.reject(items[0].id, { reason: 'tone' });
  const { drafts } = await draftStory(STORY, [VIDEO], { platforms: ['x'], config });
  assert.equal(q.add(drafts[0]).added, true);
});

test('approve requires passing checks; edit sends it back to pending', async () => {
  const { q, config, items } = await setup(['x']);
  const id = items[0].id;
  q.approve(id, { config });
  assert.equal(q.get(id).status, 'approved');
  q.edit(id, { text: 'A severe winter, year 3: the herd halved. https://game.example' }, { config });
  assert.equal(q.get(id).status, 'pending');
  assert.equal(q.get(id).approval, null);
  q.edit(id, { text: 'Over 9000 died. '.repeat(30) }, { config });
  assert.equal(q.get(id).validation.ok, false);
  assert.throws(() => q.approve(id, { config }), /fails checks/);
  assert.throws(() => q.edit(id, { nope: 'x' }, { config }), /no field nope/);
  assert.deepEqual(q.get(id).history.map((h) => h.action), ['created', 'approved', 'edited', 'edited']);
});

test('reddit: own subreddit by default; third-party needs explicit confirmation', async () => {
  const { q, config, items } = await setup(['reddit']);
  const id = items[0].id;
  assert.throws(() => q.approve(id, { config, target: 'r/IndieGaming' }), /not one of your own subreddits/);
  assert.equal(q.get(id).status, 'pending');
  q.approve(id, { config, target: 'r/IndieGaming', confirmThirdParty: true });
  assert.equal(q.get(id).target, 'IndieGaming');
  assert.equal(q.get(id).approval.thirdParty, true);
  assert.deepEqual(resolveRedditTarget(undefined, config), { target: 'EvolutionMMO', own: true });
  assert.deepEqual(resolveRedditTarget('u/owner', config), { target: 'u_owner', own: true });
});

test('publish preflight: unapproved, edited-after-approval and duplicates are refused', async () => {
  const { q, config, items } = await setup(['x']);
  const it = items[0];
  const now = new Date();
  assert.match(preflight(it, q, config, now), /status is pending/);
  q.approve(it.id, { config });
  assert.equal(preflight(it, q, config, now), null);
  it.fields.text += ' sneaky'; // edit without going through queue.edit
  it.contentHash = 'changed';
  assert.match(preflight(it, q, config, now), /edited after approval/);
  it.contentHash = it.approval.contentHash;
  q.add({ ...it, storyId: 'x2' }, { force: true });
  const twin = q.items[1];
  q.mark(twin.id, 'posted', { result: { postedAt: now.toISOString() } });
  assert.match(preflight(it, q, config, now), /duplicate of/);
});

test('daily cap blocks more posts on a platform', async () => {
  const { q, config, items } = await setup(['x'], { dailyCaps: { x: 1 } });
  const now = new Date();
  q.approve(items[0].id, { config });
  const other = q.add({ ...items[0], storyId: 'other', contentHash: 'other', status: undefined }, { force: true }).item;
  q.mark(other.id, 'posted', { result: { postedAt: now.toISOString() } });
  assert.match(preflight(items[0], q, config, now), /daily cap of 1/);
});
