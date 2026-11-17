import { test } from 'node:test';
import assert from 'node:assert/strict';
import path from 'node:path';
import { zonedToUtc, scheduleApproved, localWeekKey, fits, channelRules, renderCalendar } from '../src/calendar.mjs';
import { Queue } from '../src/queue.mjs';
import { testConfig, tmpdir } from './helpers.mjs';

test('zonedToUtc handles offsets and DST', () => {
  assert.equal(zonedToUtc(2026, 1, 15, 9, 0, 'Europe/Rome').toISOString(), '2026-01-15T08:00:00.000Z');
  assert.equal(zonedToUtc(2026, 7, 15, 9, 0, 'Europe/Rome').toISOString(), '2026-07-15T07:00:00.000Z');
  assert.equal(zonedToUtc(2026, 3, 8, 12, 0, 'America/New_York').toISOString(), '2026-03-08T16:00:00.000Z');
  assert.equal(zonedToUtc(2026, 11, 17, 18, 30, 'UTC').toISOString(), '2026-11-17T18:30:00.000Z');
});

test('ISO week keys', () => {
  assert.equal(localWeekKey(new Date('2026-11-16T12:00:00Z'), 'UTC'), '2026-W47');
  assert.equal(localWeekKey(new Date('2026-11-22T12:00:00Z'), 'UTC'), '2026-W47');
  assert.equal(localWeekKey(new Date('2026-11-23T12:00:00Z'), 'UTC'), '2026-W48');
  assert.equal(localWeekKey(new Date('2027-01-01T12:00:00Z'), 'UTC'), '2026-W53');
});

function queueWith(n, platform) {
  const q = new Queue(path.join(tmpdir(), 'q.json'));
  for (let i = 0; i < n; i++) {
    const { item } = q.add({ storyId: `s${i}`, platform, fields: { text: `post ${i}` }, contentHash: `h${i}`, validation: { ok: true, errors: [], warnings: [] } });
    item.status = 'approved';
    item.approval = { at: new Date(2026, 0, 1, 0, i).toISOString(), contentHash: `h${i}` };
  }
  return q;
}

test('schedule respects windows, posts per week, one per day and the minimum gap', () => {
  const config = testConfig({
    timezone: 'Europe/Rome',
    calendar: { channels: { bluesky: { postsPerWeek: 3, maxPerDay: 1, minGapHours: 20, windows: [{ days: [1, 2, 3, 4, 5], start: '09:00', end: '12:00' }] } } },
  });
  const q = queueWith(5, 'bluesky');
  const now = new Date('2026-11-16T06:00:00Z'); // Monday 07:00 in Rome
  const done = scheduleApproved(q, config, now);
  assert.equal(done.length, 5);
  const times = q.items.map((i) => i.scheduledAt);
  assert.deepEqual(times, [
    '2026-11-16T08:00:00.000Z', // Mon 09:00 Rome
    '2026-11-17T08:00:00.000Z',
    '2026-11-18T08:00:00.000Z',
    '2026-11-23T08:00:00.000Z', // week cap of 3 → next Monday
    '2026-11-24T08:00:00.000Z',
  ]);
  assert.ok(q.items.every((i) => i.status === 'scheduled'));
  assert.match(renderCalendar(q, config, now, 14), /Mon 16 Nov, 09:00\s+bluesky/);
});

test('a slot inside an open window starts now; already-booked slots count', () => {
  const config = testConfig({ calendar: { channels: { x: { postsPerWeek: 5, maxPerDay: 1, minGapHours: 1, windows: [{ days: [0, 1, 2, 3, 4, 5, 6], start: '09:00', end: '11:00' }] } } } });
  const q = queueWith(2, 'x');
  q.items[0].status = 'scheduled';
  q.items[0].scheduledAt = '2026-11-17T09:00:00.000Z';
  const done = scheduleApproved(q, config, new Date('2026-11-17T09:30:10Z'));
  assert.equal(done.length, 1);
  assert.equal(q.items[1].scheduledAt, '2026-11-18T09:00:00.000Z', 'today already has its one post');
});

test('fits() rejects a slot too close to another', () => {
  const rules = { postsPerWeek: 10, maxPerDay: 5, minGapHours: 20 };
  assert.equal(fits(new Date('2026-11-17T10:00Z'), [new Date('2026-11-17T09:00Z')], rules, 'UTC'), false);
  assert.equal(fits(new Date('2026-11-18T10:00Z'), [new Date('2026-11-17T09:00Z')], rules, 'UTC'), true);
  assert.equal(channelRules(testConfig(), 'reddit').minGapHours, 96);
});
