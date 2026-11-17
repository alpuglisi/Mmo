// Instagram Reels via the Instagram Graph API content publishing endpoints.
// Needs a Meta app with instagram_content_publish (or
// instagram_business_content_publish) approved, a professional (Business or
// Creator) Instagram account, and a long-lived token. The API fetches the
// video from a public URL, so set config.instagram.publicMediaBaseUrl to a
// place where the owner has uploaded the export (e.g. the game's CDN).
// Without enabled + appApproved in config the publisher uses manual export.

import path from 'node:path';

export default {
  name: 'instagram',
  env: ['INSTAGRAM_USER_ID', 'INSTAGRAM_ACCESS_TOKEN'],
  minIntervalMs: 5000,
  gated: true,

  fake(url, init) {
    if (url.endsWith('/media') && init?.method === 'POST') return { json: { id: '17900000000000000' } };
    if (url.includes('fields=status_code')) return { json: { status_code: 'FINISHED' } };
    if (url.endsWith('/media_publish')) return { json: { id: '17910000000000000' } };
    if (url.includes('fields=permalink')) return { json: { permalink: 'https://www.instagram.com/reel/dry/' } };
    if (url.includes('/insights')) return { json: { data: [{ name: 'views', values: [{ value: 0 }] }, { name: 'likes', values: [{ value: 0 }] }] } };
    return { json: {} };
  },

  base(ctx) {
    const c = ctx.config.instagram;
    return `https://${c.graphHost || 'graph.facebook.com'}/${c.graphVersion}`;
  },

  async publish(item, ctx) {
    const { INSTAGRAM_USER_ID: uid, INSTAGRAM_ACCESS_TOKEN: token } = ctx.env;
    if (!uid || !token) throw Object.assign(new Error('instagram: INSTAGRAM_USER_ID / INSTAGRAM_ACCESS_TOKEN not set'), { code: 'MISSING_SECRET' });
    const baseUrl = ctx.config.instagram.publicMediaBaseUrl;
    if (!baseUrl) throw new Error('instagram: set instagram.publicMediaBaseUrl (the API pulls the video from a public URL)');
    const video = (item.media || []).find((m) => m.type === 'video');
    if (!video) throw new Error('instagram: no video');
    const base = this.base(ctx);
    const form = (o) => ({ method: 'POST', headers: { 'content-type': 'application/x-www-form-urlencoded' }, body: new URLSearchParams({ ...o, access_token: token }).toString() });
    const c = await ctx.http.request(
      `${base}/${uid}/media`,
      form({ media_type: 'REELS', video_url: `${baseUrl.replace(/\/$/, '')}/${path.basename(video.path)}`, caption: item.fields.caption, share_to_feed: 'true' }),
    );
    const container = c.json.id;
    let status = 'IN_PROGRESS';
    for (let i = 0; status === 'IN_PROGRESS' && i < 40; i++) {
      await ctx.sleep(ctx.live ? 15000 : 0);
      status = (await ctx.http.request(`${base}/${container}?fields=status_code`, { headers: { authorization: `Bearer ${token}` } })).json.status_code;
    }
    if (status !== 'FINISHED') throw new Error(`instagram: container ${container} ended as ${status}`);
    const p = await ctx.http.request(`${base}/${uid}/media_publish`, form({ creation_id: container }));
    const link = await ctx.http.request(`${base}/${p.json.id}?fields=permalink`, { headers: { authorization: `Bearer ${token}` } });
    return { remoteId: p.json.id, url: link.json.permalink };
  },

  async metrics(item, ctx) {
    if (!item.result?.remoteId) return null;
    const r = await ctx.http.request(`${this.base(ctx)}/${item.result.remoteId}/insights?metric=views,likes,comments,shares`, {
      headers: { authorization: `Bearer ${ctx.env.INSTAGRAM_ACCESS_TOKEN}` },
    });
    const v = Object.fromEntries((r.json.data || []).map((d) => [d.name, d.values?.[0]?.value ?? 0]));
    return { views: v.views, likes: v.likes, replies: v.comments, reposts: v.shares };
  },
};
