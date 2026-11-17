import { requireSecrets } from '../config.mjs';

// YouTube Data API v3 resumable upload to the owner's channel.
// A vertical video of up to three minutes is treated as a Short.
// OAuth scope: https://www.googleapis.com/auth/youtube.upload (plus
// youtube.readonly for metrics). Note: uploads from an API project that has
// not passed Google's audit are locked to private until it does.

export default {
  name: 'youtube',
  env: ['YOUTUBE_CLIENT_ID', 'YOUTUBE_CLIENT_SECRET', 'YOUTUBE_REFRESH_TOKEN'],
  minIntervalMs: 2000,

  fake(url, init) {
    if (url.startsWith('https://oauth2.googleapis.com/token')) return { json: { access_token: 'dry-access', expires_in: 3599 } };
    if (url.includes('uploadType=resumable')) return { headers: { location: 'https://www.googleapis.com/upload/youtube/v3/videos?upload_id=dry' }, json: {} };
    if (url.includes('upload_id=')) return { json: { id: 'dryRunVid01', status: { uploadStatus: 'uploaded' } } };
    if (url.includes('/youtube/v3/videos?part=statistics')) return { json: { items: [{ statistics: { viewCount: '0', likeCount: '0', commentCount: '0' } }] } };
    return { json: {} };
  },

  async token(ctx) {
    const s = requireSecrets('youtube', ctx.env, this.env);
    const r = await ctx.http.request('https://oauth2.googleapis.com/token', {
      method: 'POST',
      headers: { 'content-type': 'application/x-www-form-urlencoded' },
      body: new URLSearchParams({
        client_id: s.YOUTUBE_CLIENT_ID,
        client_secret: s.YOUTUBE_CLIENT_SECRET,
        refresh_token: s.YOUTUBE_REFRESH_TOKEN,
        grant_type: 'refresh_token',
      }).toString(),
    });
    return r.json.access_token;
  },

  async publish(item, ctx) {
    const video = (item.media || []).find((m) => m.type === 'video');
    if (!video) throw new Error('youtube: no video attached');
    const f = ctx.readMedia(video.path);
    const auth = { authorization: `Bearer ${await this.token(ctx)}` };
    let title = item.fields.title;
    if (!/#shorts/i.test(title + item.fields.description) && title.length + 8 <= 100) title += ' #Shorts';
    const meta = {
      snippet: {
        title,
        description: item.fields.description,
        tags: (item.hashtags || []).filter((t) => t.toLowerCase() !== 'shorts'),
        categoryId: ctx.config.youtube.categoryId,
        defaultLanguage: 'en',
      },
      status: { privacyStatus: ctx.config.youtube.privacyStatus, selfDeclaredMadeForKids: false, embeddable: true },
    };
    const start = await ctx.http.request(
      'https://www.googleapis.com/upload/youtube/v3/videos?uploadType=resumable&part=snippet,status',
      {
        method: 'POST',
        headers: { ...auth, 'content-type': 'application/json; charset=UTF-8', 'x-upload-content-length': String(f.size), 'x-upload-content-type': f.mime },
        body: JSON.stringify(meta),
      },
    );
    const location = start.headers.get('location');
    if (!location) throw new Error('youtube: no resumable upload URL returned');
    const up = await ctx.http.request(location, { method: 'PUT', headers: { ...auth, 'content-type': f.mime, 'content-length': String(f.size) }, body: f.bytes });
    return { remoteId: up.json.id, url: `https://www.youtube.com/shorts/${up.json.id}` };
  },

  async metrics(item, ctx) {
    if (!item.result?.remoteId) return null;
    const auth = { authorization: `Bearer ${await this.token(ctx)}` };
    const r = await ctx.http.request(`https://www.googleapis.com/youtube/v3/videos?part=statistics&id=${item.result.remoteId}`, { headers: auth });
    const s = r.json.items?.[0]?.statistics;
    if (!s) return null;
    return { views: +s.viewCount, likes: +(s.likeCount || 0), replies: +(s.commentCount || 0) };
  },
};
