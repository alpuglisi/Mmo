import fs from 'node:fs';
import path from 'node:path';
import { blobOf } from './media.mjs';

// X API v2 with an OAuth 2.0 user-context token for the owner's account
// (scopes: tweet.read tweet.write users.read media.write offline.access).
// Access tokens last about two hours. With X_CLIENT_ID and X_REFRESH_TOKEN set,
// the adapter refreshes, and since X rotates refresh tokens it stores the new
// pair in <dataDir>/secrets/x-token.json (mode 0600, gitignored with data/).

const API = 'https://api.x.com';
const CHUNK = 4 * 1024 * 1024;

function tokenFile(ctx) {
  return path.join(ctx.config.dataDir, 'secrets', 'x-token.json');
}

export default {
  name: 'x',
  env: [],
  minIntervalMs: 3000,

  fake(url, init) {
    if (url.endsWith('/2/oauth2/token')) return { json: { access_token: 'dry-access', refresh_token: 'dry-refresh', expires_in: 7200 } };
    if (url.endsWith('/2/media/upload/initialize')) return { json: { data: { id: '1880000000000000000' } } };
    if (url.includes('/append')) return { json: {} };
    if (url.includes('/finalize')) return { json: { data: { id: '1880000000000000000', processing_info: { state: 'succeeded' } } } };
    if (url.includes('/2/media/upload?')) return { json: { data: { processing_info: { state: 'succeeded' } } } };
    if (url.endsWith('/2/tweets') && init?.method === 'POST') return { status: 201, json: { data: { id: '1890000000000000000', text: '' } } };
    if (url.includes('/2/tweets/')) return { json: { data: { public_metrics: { like_count: 0, retweet_count: 0, reply_count: 0, quote_count: 0, impression_count: 0 } } } };
    return { json: {} };
  },

  async token(ctx) {
    const env = ctx.env;
    let stored = null;
    const f = tokenFile(ctx);
    if (fs.existsSync(f)) stored = JSON.parse(fs.readFileSync(f, 'utf8'));
    if (stored?.access_token && stored.expires_at > Date.now() + 60000) return stored.access_token;
    const refresh = stored?.refresh_token || env.X_REFRESH_TOKEN;
    if (env.X_CLIENT_ID && refresh) {
      const headers = { 'content-type': 'application/x-www-form-urlencoded' };
      if (env.X_CLIENT_SECRET) headers.authorization = `Basic ${Buffer.from(`${env.X_CLIENT_ID}:${env.X_CLIENT_SECRET}`).toString('base64')}`;
      const r = await ctx.http.request(`${API}/2/oauth2/token`, {
        method: 'POST',
        headers,
        body: new URLSearchParams({ grant_type: 'refresh_token', refresh_token: refresh, client_id: env.X_CLIENT_ID }).toString(),
      });
      if (ctx.live) {
        fs.mkdirSync(path.dirname(f), { recursive: true });
        fs.writeFileSync(f, JSON.stringify({ access_token: r.json.access_token, refresh_token: r.json.refresh_token, expires_at: Date.now() + r.json.expires_in * 1000 }), { mode: 0o600 });
      }
      return r.json.access_token;
    }
    if (env.X_USER_ACCESS_TOKEN) return env.X_USER_ACCESS_TOKEN;
    const e = new Error('x: set X_USER_ACCESS_TOKEN, or X_CLIENT_ID and X_REFRESH_TOKEN (see PUBLISHING.md)');
    e.code = 'MISSING_SECRET';
    throw e;
  },

  async uploadMedia(m, auth, ctx) {
    const f = ctx.readMedia(m.path);
    const isVideo = m.type === 'video';
    const init = await ctx.http.request(`${API}/2/media/upload/initialize`, {
      method: 'POST',
      headers: { ...auth, 'content-type': 'application/json' },
      body: JSON.stringify({ media_type: f.mime, total_bytes: f.size, media_category: isVideo ? 'tweet_video' : 'tweet_image' }),
    });
    const id = init.json.data.id;
    for (let i = 0, seg = 0; i < f.size; i += CHUNK, seg++) {
      const form = new FormData();
      form.append('segment_index', String(seg));
      form.append('media', blobOf({ bytes: f.bytes.subarray(i, i + CHUNK), mime: f.mime }), f.name);
      await ctx.http.request(`${API}/2/media/upload/${id}/append`, { method: 'POST', headers: auth, body: form });
    }
    const fin = await ctx.http.request(`${API}/2/media/upload/${id}/finalize`, { method: 'POST', headers: auth });
    let info = fin.json.data?.processing_info;
    for (let i = 0; info && !['succeeded', 'failed'].includes(info.state) && i < 60; i++) {
      await ctx.sleep(ctx.live ? (info.check_after_secs || 5) * 1000 : 0);
      info = (await ctx.http.request(`${API}/2/media/upload?command=STATUS&media_id=${id}`, { headers: auth })).json.data?.processing_info;
    }
    if (info?.state === 'failed') throw new Error(`x: media processing failed: ${JSON.stringify(info.error || info)}`);
    if (m.alt && !isVideo) {
      await ctx.http.request(`${API}/2/media/metadata`, {
        method: 'POST',
        headers: { ...auth, 'content-type': 'application/json' },
        body: JSON.stringify({ id, metadata: { alt_text: { text: m.alt.slice(0, 1000) } } }),
      });
    }
    return id;
  },

  async publish(item, ctx) {
    const auth = { authorization: `Bearer ${await this.token(ctx)}` };
    const body = { text: item.fields.text };
    const media = (item.media || []).slice(0, 1);
    if (media.length) body.media = { media_ids: [] };
    for (const m of media) body.media.media_ids.push(await this.uploadMedia(m, auth, ctx));
    const r = await ctx.http.request(`${API}/2/tweets`, {
      method: 'POST',
      headers: { ...auth, 'content-type': 'application/json' },
      body: JSON.stringify(body),
    });
    return { remoteId: r.json.data.id, url: `https://x.com/i/web/status/${r.json.data.id}` };
  },

  async metrics(item, ctx) {
    if (!item.result?.remoteId) return null;
    const auth = { authorization: `Bearer ${await this.token(ctx)}` };
    const r = await ctx.http.request(`${API}/2/tweets/${item.result.remoteId}?tweet.fields=public_metrics`, { headers: auth });
    const m = r.json.data.public_metrics;
    return { likes: m.like_count, reposts: m.retweet_count + m.quote_count, replies: m.reply_count, views: m.impression_count };
  },
};
