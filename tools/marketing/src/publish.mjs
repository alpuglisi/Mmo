import { ADAPTERS, manualReason } from './adapters/index.mjs';
import { createHttp, RateLimitError } from './http.mjs';
import { makeReadMedia } from './adapters/media.mjs';
import { exportDraft } from './adapters/manual.mjs';
import { SECRET_ENV } from './config.mjs';

const PLACEHOLDER = {
  MASTODON_INSTANCE: 'mastodon.example',
  DISCORD_WEBHOOK_URL: 'https://discord.com/api/webhooks/0/placeholder',
  BLUESKY_HANDLE: 'you.bsky.social',
};

/** Dry runs work without credentials: missing secrets get placeholders. */
export function dryRunEnv(env, platform) {
  const out = { ...env };
  for (const n of SECRET_ENV[platform] || []) if (!out[n]) out[n] = PLACEHOLDER[n] || 'dry-run-placeholder';
  if (platform === 'x' && !out.X_USER_ACCESS_TOKEN && !out.X_REFRESH_TOKEN) out.X_USER_ACCESS_TOKEN = 'dry-run-placeholder';
  return out;
}

const DAY = 86400000;

export function makeCtx(platform, { config, live, fetch, sleep, log, now }) {
  const a = ADAPTERS[platform];
  const http = createHttp({ platform, live, fetch, fake: a?.fake?.bind(a), minIntervalMs: a?.minIntervalMs, sleep, log });
  return {
    http,
    env: live ? config.env || process.env : dryRunEnv(config.env || process.env, platform),
    config,
    live,
    now,
    log: log || (() => {}),
    sleep: sleep || ((ms) => new Promise((r) => setTimeout(r, ms))),
    readMedia: makeReadMedia({ live, log }),
  };
}

/** Pre-flight checks that hold in dry-run and live alike. Returns a reason to skip, or null. */
export function preflight(item, queue, config, now) {
  if (!['approved', 'scheduled', 'failed'].includes(item.status)) return `status is ${item.status}`;
  if (!item.approval) return 'not approved';
  if (item.approval.contentHash !== item.contentHash) return 'edited after approval; approve again';
  if (!item.validation?.ok) return `fails checks: ${item.validation.errors.join('; ')}`;
  const dup = queue.items.find((o) => o !== item && o.status === 'posted' && o.platform === item.platform && o.contentHash === item.contentHash);
  if (dup) return `duplicate of ${dup.id}, already posted`;
  const cap = config.dailyCaps?.[item.platform];
  if (cap) {
    const recent = queue.items.filter((o) => o.platform === item.platform && o.status === 'posted' && now - new Date(o.result?.postedAt) < DAY).length;
    if (recent >= cap) return `daily cap of ${cap} reached for ${item.platform}`;
  }
  return null;
}

/**
 * Publish due items. Dry-run (default) records the requests each adapter
 * would make and changes nothing. Live posts and updates the queue.
 */
export async function publishDue(queue, config, { live = false, now = new Date(), ids = null, fetch, sleep, log = console.log, save = () => {} } = {}) {
  const due = ids
    ? ids.map((id) => queue.get(id))
    : queue.items.filter((i) => i.status === 'scheduled' && new Date(i.scheduledAt) <= now);
  const report = [];
  const stopped = new Set();
  for (const item of due) {
    const entry = { id: item.id, platform: item.platform, live };
    report.push(entry);
    if (stopped.has(item.platform)) {
      entry.skipped = 'platform rate-limited earlier in this run';
      continue;
    }
    const why = preflight(item, queue, config, now);
    if (why) {
      entry.skipped = why;
      continue;
    }
    const manual = manualReason(item.platform, config);
    if (manual) {
      const ex = exportDraft(item, config.outDir);
      entry.exported = ex.dir;
      entry.note = manual;
      if (live) queue.mark(item.id, 'exported', { result: { exportedTo: ex.dir, at: now.toISOString() } }, { now });
      save();
      continue;
    }
    const adapter = ADAPTERS[item.platform];
    const ctx = makeCtx(item.platform, { config, live, fetch, sleep, log, now });
    try {
      if (live) {
        queue.mark(item.id, 'posting', {}, { now });
        save();
      }
      const res = await adapter.publish(item, ctx);
      entry.result = res;
      entry.requests = ctx.http.records;
      if (live) queue.mark(item.id, 'posted', { result: { ...res, postedAt: now.toISOString() } }, { now });
    } catch (e) {
      entry.error = e.message;
      entry.requests = ctx.http.records;
      if (e instanceof RateLimitError) stopped.add(item.platform);
      if (live) queue.mark(item.id, 'failed', { result: { error: e.message, at: now.toISOString() } }, { now });
    }
    save();
  }
  return report;
}
