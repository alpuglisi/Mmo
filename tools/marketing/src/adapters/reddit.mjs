import { requireSecrets } from '../config.mjs';
import { resolveRedditTarget } from '../queue.mjs';

// Reddit official API, "script" app on the owner's account (password grant).
// Posts only to the owner's own subreddits or profile (u_<name>) unless a
// human approved that specific post for a third-party subreddit
// (queue approve --confirm-third-party). Reddit requires a descriptive
// User-Agent: <platform>:<app id>:<version> (by /u/<username>).

const VERSION = '0.1.0';

export default {
  name: 'reddit',
  env: ['REDDIT_CLIENT_ID', 'REDDIT_CLIENT_SECRET', 'REDDIT_USERNAME', 'REDDIT_PASSWORD'],
  minIntervalMs: 2000,

  fake(url) {
    if (url.endsWith('/api/v1/access_token')) return { json: { access_token: 'dry-access', token_type: 'bearer', expires_in: 86400 } };
    if (url.includes('/api/submit')) return { json: { json: { errors: [], data: { id: 'dry01', name: 't3_dry01', url: 'https://www.reddit.com/r/test/comments/dry01/' } } } };
    if (url.includes('/api/info')) return { json: { data: { children: [{ data: { score: 1, num_comments: 0, upvote_ratio: 1 } }] } } };
    return { json: {} };
  },

  userAgent(ctx) {
    const u = ctx.env.REDDIT_USERNAME || ctx.config.reddit.username || 'unknown';
    return `script:${ctx.config.reddit.userAgentApp}:${VERSION} (by /u/${u})`;
  },

  async token(ctx) {
    const s = requireSecrets('reddit', ctx.env, this.env);
    const r = await ctx.http.request('https://www.reddit.com/api/v1/access_token', {
      method: 'POST',
      headers: {
        authorization: `Basic ${Buffer.from(`${s.REDDIT_CLIENT_ID}:${s.REDDIT_CLIENT_SECRET}`).toString('base64')}`,
        'content-type': 'application/x-www-form-urlencoded',
        'user-agent': this.userAgent(ctx),
      },
      body: new URLSearchParams({ grant_type: 'password', username: s.REDDIT_USERNAME, password: s.REDDIT_PASSWORD }).toString(),
    });
    return r.json.access_token;
  },

  /** Throws unless the post's target is allowed. */
  checkTarget(item, config) {
    const t = resolveRedditTarget(item.target, config);
    if (t.target !== item.target) throw new Error(`reddit: target changed since approval (${item.target} → ${t.target}); re-approve`);
    if (!t.own && item.approval?.thirdParty !== true)
      throw new Error(`reddit: ${t.target} is not an own subreddit and this post was not approved for it`);
    return t;
  },

  async publish(item, ctx) {
    this.checkTarget(item, ctx.config);
    const token = await this.token(ctx);
    const form = new URLSearchParams({
      api_type: 'json',
      sr: item.target,
      kind: 'self',
      title: item.fields.title,
      text: item.fields.body,
      resubmit: 'false',
      sendreplies: 'true',
    });
    const r = await ctx.http.request('https://oauth.reddit.com/api/submit', {
      method: 'POST',
      headers: { authorization: `Bearer ${token}`, 'content-type': 'application/x-www-form-urlencoded', 'user-agent': this.userAgent(ctx) },
      body: form.toString(),
    });
    const errs = r.json?.json?.errors || [];
    if (errs.length) throw new Error(`reddit: ${JSON.stringify(errs)}`);
    const d = r.json.json.data;
    return { remoteId: d.name, url: d.url };
  },

  async metrics(item, ctx) {
    if (!item.result?.remoteId) return null;
    const token = await this.token(ctx);
    const r = await ctx.http.request(`https://oauth.reddit.com/api/info?id=${item.result.remoteId}`, {
      headers: { authorization: `Bearer ${token}`, 'user-agent': this.userAgent(ctx) },
    });
    const d = r.json.data.children?.[0]?.data;
    return d ? { score: d.score, replies: d.num_comments } : null;
  },
};
