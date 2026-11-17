import { requireSecrets } from '../config.mjs';
import { blobOf } from './media.mjs';

// Mastodon REST API with an access token from the owner's account
// (Preferences → Development → New application, scopes write:statuses write:media read:statuses).

export default {
  name: 'mastodon',
  env: ['MASTODON_INSTANCE', 'MASTODON_ACCESS_TOKEN'],
  minIntervalMs: 2000,

  fake(url, init) {
    if (url.includes('/api/v2/media')) return { status: 202, json: { id: 'dry-media-1', url: null } };
    if (/\/api\/v1\/media\//.test(url)) return { json: { id: 'dry-media-1', url: 'https://example.invalid/m.mp4' } };
    if (url.endsWith('/api/v1/statuses') && init?.method === 'POST') return { json: { id: '1100', url: 'https://example.invalid/@me/1100' } };
    if (/\/api\/v1\/statuses\//.test(url)) return { json: { favourites_count: 0, reblogs_count: 0, replies_count: 0 } };
    return { json: {} };
  },

  base(ctx) {
    const s = requireSecrets('mastodon', ctx.env, this.env);
    const inst = s.MASTODON_INSTANCE.replace(/\/$/, '');
    return { inst: inst.startsWith('http') ? inst : `https://${inst}`, auth: { authorization: `Bearer ${s.MASTODON_ACCESS_TOKEN}` } };
  },

  async publish(item, ctx) {
    const { inst, auth } = this.base(ctx);
    const mediaIds = [];
    for (const m of (item.media || []).slice(0, 4)) {
      const f = ctx.readMedia(m.path);
      const form = new FormData();
      form.append('file', blobOf(f), f.name);
      if (m.alt) form.append('description', m.alt.slice(0, 1500));
      const r = await ctx.http.request(`${inst}/api/v2/media`, { method: 'POST', headers: auth, body: form });
      let media = r.json;
      // 202 = still processing; poll politely.
      for (let i = 0; r.status === 202 && !media.url && i < 30; i++) {
        await ctx.sleep(ctx.live ? 2000 : 0);
        media = (await ctx.http.request(`${inst}/api/v1/media/${media.id}`, { headers: auth })).json;
      }
      mediaIds.push(media.id);
    }
    const body = { status: item.fields.text, visibility: ctx.config.mastodon.visibility || 'public', language: 'en' };
    if (mediaIds.length) body.media_ids = mediaIds;
    const r = await ctx.http.request(`${inst}/api/v1/statuses`, {
      method: 'POST',
      // Idempotency-Key makes a retried request create at most one status.
      headers: { ...auth, 'content-type': 'application/json', 'idempotency-key': `evo-${item.id}-${item.contentHash}` },
      body: JSON.stringify(body),
    });
    return { remoteId: r.json.id, url: r.json.url };
  },

  async metrics(item, ctx) {
    if (!item.result?.remoteId) return null;
    const { inst, auth } = this.base(ctx);
    const r = await ctx.http.request(`${inst}/api/v1/statuses/${item.result.remoteId}`, { headers: auth });
    return { likes: r.json.favourites_count, reposts: r.json.reblogs_count, replies: r.json.replies_count };
  },
};
