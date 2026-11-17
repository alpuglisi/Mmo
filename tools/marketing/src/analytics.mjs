import fs from 'node:fs';
import path from 'node:path';
import { ADAPTERS } from './adapters/index.mjs';
import { makeCtx } from './publish.mjs';
import { summaryOf } from './calendar.mjs';

// Pulls public metrics for posted items through the same official APIs and
// keeps snapshots in <dataDir>/metrics.json. Link clicks come from the site's
// own analytics, grouped by the UTM parameters (see utm.mjs).

export async function collectMetrics(queue, config, { live = false, fetch, now = new Date(), log } = {}) {
  const file = path.join(config.dataDir, 'metrics.json');
  const store = fs.existsSync(file) ? JSON.parse(fs.readFileSync(file, 'utf8')) : { snapshots: [] };
  const out = [];
  for (const item of queue.items.filter((i) => i.status === 'posted' && i.result?.remoteId)) {
    const a = ADAPTERS[item.platform];
    if (!a?.metrics) continue;
    const ctx = makeCtx(item.platform, { config, live, fetch, log, now });
    try {
      const m = await a.metrics(item, ctx);
      if (!m) continue;
      const snap = { itemId: item.id, platform: item.platform, storyId: item.storyId, at: now.toISOString(), metrics: m };
      out.push(snap);
      if (live) store.snapshots.push(snap);
    } catch (e) {
      out.push({ itemId: item.id, platform: item.platform, error: e.message });
    }
  }
  if (live) {
    fs.mkdirSync(config.dataDir, { recursive: true });
    fs.writeFileSync(file, JSON.stringify(store, null, 2) + '\n');
  }
  return out;
}

export function engagement(m) {
  return (m.likes || 0) + (m.reposts || 0) * 2 + (m.replies || 0) * 2 + (m.score || 0);
}

/** Latest snapshot per item, summarised per platform and per story. */
export function summarize(queue, snapshots) {
  const latest = new Map();
  for (const s of snapshots) if (!s.error) latest.set(s.itemId, s);
  const byPlatform = {};
  for (const item of queue.items) {
    if (item.status !== 'posted') continue;
    const p = (byPlatform[item.platform] ||= { posts: 0, views: 0, likes: 0, reposts: 0, replies: 0 });
    p.posts++;
    const m = latest.get(item.id)?.metrics;
    if (m) for (const k of ['views', 'likes', 'reposts', 'replies']) p[k] += m[k] || 0;
  }
  const top = [...latest.values()]
    .map((s) => ({ ...s, engagement: engagement(s.metrics), item: queue.items.find((i) => i.id === s.itemId) }))
    .sort((a, b) => b.engagement - a.engagement)
    .slice(0, 5);
  return { byPlatform, top };
}

export function renderSummary({ byPlatform, top }) {
  const rows = Object.entries(byPlatform).map(
    ([p, s]) => `| ${p} | ${s.posts} | ${s.views || '–'} | ${s.likes} | ${s.reposts} | ${s.replies} |`,
  );
  const lines = [
    '# Marketing summary',
    '',
    '| Channel | Posts | Views | Likes | Reposts | Replies |',
    '| --- | ---: | ---: | ---: | ---: | ---: |',
    ...(rows.length ? rows : ['| (nothing posted yet) | | | | | |']),
    '',
    '## Top posts',
    '',
    ...(top.length ? top.map((t, i) => `${i + 1}. ${t.platform} ${t.itemId} (engagement ${t.engagement}): ${t.item ? summaryOf(t.item) : ''}${t.item?.result?.url ? ` ${t.item.result.url}` : ''}`) : ['No metrics collected yet (run `metrics --live`).']),
    '',
    'Link clicks: filter your site analytics by utm_campaign, then group by utm_source and utm_content (story id).',
  ];
  return lines.join('\n');
}
