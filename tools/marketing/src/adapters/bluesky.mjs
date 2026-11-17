import { requireSecrets } from '../config.mjs';
import { URL_RE, HASHTAG_RE } from '../text.mjs';

// Bluesky via the AT Protocol XRPC API with an app password
// (Settings → Privacy and security → App passwords). Posts go to the owner's
// own repo only.

const enc = new TextEncoder();
const byteIndex = (text, i) => enc.encode(text.slice(0, i)).length;

/** Rich-text facets (byte offsets) for links and hashtags. */
export function facetsFor(text) {
  const facets = [];
  for (const m of text.matchAll(URL_RE)) {
    facets.push({
      index: { byteStart: byteIndex(text, m.index), byteEnd: byteIndex(text, m.index + m[0].length) },
      features: [{ $type: 'app.bsky.richtext.facet#link', uri: m[0] }],
    });
  }
  for (const m of text.matchAll(HASHTAG_RE)) {
    const start = m.index + m[1].length;
    facets.push({
      index: { byteStart: byteIndex(text, start), byteEnd: byteIndex(text, start + 1 + m[2].length) },
      features: [{ $type: 'app.bsky.richtext.facet#tag', tag: m[2] }],
    });
  }
  return facets;
}

/** Shorten long URLs for display (Bluesky clients do the same); the facet
 * keeps the full URL. Returns { text, facets }. */
export function richText(full) {
  let text = '';
  let last = 0;
  const links = [];
  for (const m of full.matchAll(URL_RE)) {
    text += full.slice(last, m.index);
    const u = m[0];
    const bare = u.replace(/^https?:\/\//, '');
    const display = bare.length > 30 ? bare.slice(0, 27) + '...' : bare;
    links.push({ start: text.length, display, uri: u });
    text += display;
    last = m.index + u.length;
  }
  text += full.slice(last);
  const facets = links.map((l) => ({
    index: { byteStart: byteIndex(text, l.start), byteEnd: byteIndex(text, l.start + l.display.length) },
    features: [{ $type: 'app.bsky.richtext.facet#link', uri: l.uri }],
  }));
  for (const f of facetsFor(text)) if (f.features[0].$type.endsWith('#tag')) facets.push(f);
  return { text, facets };
}

export default {
  name: 'bluesky',
  env: ['BLUESKY_HANDLE', 'BLUESKY_APP_PASSWORD'],
  minIntervalMs: 2000,

  fake(url) {
    if (url.endsWith('createSession')) return { json: { accessJwt: 'dry-jwt', did: 'did:plc:dryrun', handle: 'dry.bsky.social' } };
    if (url.endsWith('uploadBlob')) return { json: { blob: { $type: 'blob', ref: { $link: 'bafydryrun' }, mimeType: 'video/mp4', size: 16 } } };
    if (url.endsWith('createRecord')) return { json: { uri: 'at://did:plc:dryrun/app.bsky.feed.post/3dryrun', cid: 'bafydry' } };
    if (url.includes('getPosts')) return { json: { posts: [{ likeCount: 0, repostCount: 0, replyCount: 0, quoteCount: 0 }] } };
    return { json: {} };
  },

  async session(ctx) {
    const s = requireSecrets('bluesky', ctx.env, this.env);
    const svc = ctx.config.bluesky.service;
    const r = await ctx.http.request(`${svc}/xrpc/com.atproto.server.createSession`, {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify({ identifier: s.BLUESKY_HANDLE, password: s.BLUESKY_APP_PASSWORD }),
    });
    return { svc, jwt: r.json.accessJwt, did: r.json.did };
  },

  async publish(item, ctx) {
    const { svc, jwt, did } = await this.session(ctx);
    const auth = { authorization: `Bearer ${jwt}` };
    const { text, facets } = richText(item.fields.text);
    const record = { $type: 'app.bsky.feed.post', text, createdAt: ctx.now.toISOString(), langs: ['en'], facets };
    const media = item.media || [];
    const video = media.find((m) => m.type === 'video');
    const images = media.filter((m) => m.type === 'image').slice(0, 4);
    if (video || images.length) {
      const uploaded = [];
      for (const m of video ? [video] : images) {
        const f = ctx.readMedia(m.path);
        const r = await ctx.http.request(`${svc}/xrpc/com.atproto.repo.uploadBlob`, {
          method: 'POST',
          headers: { ...auth, 'content-type': f.mime },
          body: f.bytes,
        });
        uploaded.push({ blob: r.json.blob, alt: m.alt || '' });
      }
      record.embed = video
        ? { $type: 'app.bsky.embed.video', video: uploaded[0].blob, alt: uploaded[0].alt }
        : { $type: 'app.bsky.embed.images', images: uploaded.map((u) => ({ image: u.blob, alt: u.alt })) };
    }
    const r = await ctx.http.request(`${svc}/xrpc/com.atproto.repo.createRecord`, {
      method: 'POST',
      headers: { ...auth, 'content-type': 'application/json' },
      body: JSON.stringify({ repo: did, collection: 'app.bsky.feed.post', record }),
    });
    const rkey = r.json.uri.split('/').pop();
    const handle = ctx.env.BLUESKY_HANDLE || 'unknown';
    return { remoteId: r.json.uri, url: `https://bsky.app/profile/${handle}/post/${rkey}` };
  },

  async metrics(item, ctx) {
    const uri = item.result?.remoteId;
    if (!uri) return null;
    const r = await ctx.http.request(`https://public.api.bsky.app/xrpc/app.bsky.feed.getPosts?uris=${encodeURIComponent(uri)}`);
    const p = r.json.posts?.[0];
    if (!p) return null;
    return { likes: p.likeCount, reposts: p.repostCount + (p.quoteCount || 0), replies: p.replyCount };
  },
};
