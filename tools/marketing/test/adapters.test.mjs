import { test } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { makeCtx } from '../src/publish.mjs';
import { ADAPTERS } from '../src/adapters/index.mjs';
import { facetsFor, richText } from '../src/adapters/bluesky.mjs';
import { chunkPlan } from '../src/adapters/tiktok.mjs';
import { createHttp, RateLimitError, retryAfterSeconds, describeRequest } from '../src/http.mjs';
import { testConfig, tmpdir, mockFetch, has } from './helpers.mjs';

const dir = tmpdir();
const VIDEO_PATH = path.join(dir, 'clip.mp4');
const IMG_PATH = path.join(dir, 'card.png');
fs.writeFileSync(VIDEO_PATH, Buffer.alloc(1000, 1));
fs.writeFileSync(IMG_PATH, Buffer.alloc(200, 2));

const ENV = {
  BLUESKY_HANDLE: 'evo.bsky.social', BLUESKY_APP_PASSWORD: 'app-pass',
  MASTODON_INSTANCE: 'mastodon.example', MASTODON_ACCESS_TOKEN: 'masto-tok',
  X_USER_ACCESS_TOKEN: 'x-tok',
  YOUTUBE_CLIENT_ID: 'yid', YOUTUBE_CLIENT_SECRET: 'ysecret', YOUTUBE_REFRESH_TOKEN: 'yrefresh',
  REDDIT_CLIENT_ID: 'rid', REDDIT_CLIENT_SECRET: 'rsecret', REDDIT_USERNAME: 'owner', REDDIT_PASSWORD: 'rpass',
  DISCORD_WEBHOOK_URL: 'https://discord.com/api/webhooks/123/secret-token',
  BUTTONDOWN_API_KEY: 'bd-key',
  TIKTOK_ACCESS_TOKEN: 'tt-tok',
  INSTAGRAM_USER_ID: '1789', INSTAGRAM_ACCESS_TOKEN: 'ig-tok',
};

function ctxFor(platform, fetch, over = {}) {
  const config = testConfig({ reddit: { username: 'owner', ownSubreddits: ['EvolutionMMO'], defaultTarget: 'EvolutionMMO' }, ...over }, ENV);
  return makeCtx(platform, { config, live: true, fetch, sleep: async () => {}, now: new Date('2026-11-17T10:00:00Z') });
}

const item = (platform, fields, media = [], extra = {}) => ({ id: 'abc123', platform, fields, media, contentHash: 'h1', hashtags: ['EvolutionMMO'], ...extra });
const body = (call) => JSON.parse(call.init.body);

test('bluesky: session, blob upload, post with facets and video embed', async () => {
  const f = mockFetch([
    [has('createSession'), () => ({ json: { accessJwt: 'jwt1', did: 'did:plc:me' } })],
    [has('uploadBlob'), () => ({ json: { blob: { $type: 'blob', ref: { $link: 'cid' }, mimeType: 'video/mp4', size: 1000 } } })],
    [has('createRecord'), () => ({ json: { uri: 'at://did:plc:me/app.bsky.feed.post/3abc', cid: 'c' } })],
  ]);
  const text = 'Winter came. https://game.example/?utm_source=bluesky&utm_content=y3 #EvolutionMMO';
  const res = await ADAPTERS.bluesky.publish(item('bluesky', { text }, [{ type: 'video', path: VIDEO_PATH, alt: 'snow' }]), ctxFor('bluesky', f));
  assert.equal(res.url, 'https://bsky.app/profile/evo.bsky.social/post/3abc');
  assert.deepEqual(body(f.calls[0]), { identifier: 'evo.bsky.social', password: 'app-pass' });
  assert.equal(f.calls[1].init.headers.authorization, 'Bearer jwt1');
  assert.equal(f.calls[1].init.headers['content-type'], 'video/mp4');
  const rec = body(f.calls[2]);
  assert.equal(rec.repo, 'did:plc:me');
  assert.equal(rec.collection, 'app.bsky.feed.post');
  assert.equal(rec.record.embed.$type, 'app.bsky.embed.video');
  assert.equal(rec.record.embed.alt, 'snow');
  assert.match(rec.record.text, /game\.example\/\?utm_source=bl\.\.\./, 'long link shortened for display');
  const link = rec.record.facets.find((x) => x.features[0].$type.endsWith('#link'));
  assert.equal(link.features[0].uri, 'https://game.example/?utm_source=bluesky&utm_content=y3');
  const tag = rec.record.facets.find((x) => x.features[0].$type.endsWith('#tag'));
  assert.equal(Buffer.from(rec.record.text).subarray(tag.index.byteStart, tag.index.byteEnd).toString(), '#EvolutionMMO');
});

test('bluesky facets use UTF-8 byte offsets', () => {
  const t = 'Ünïcödé 🦊 https://a.example #fox';
  const [link, tag] = facetsFor(t);
  const b = Buffer.from(t);
  assert.equal(b.subarray(link.index.byteStart, link.index.byteEnd).toString(), 'https://a.example');
  assert.equal(b.subarray(tag.index.byteStart, tag.index.byteEnd).toString(), '#fox');
  assert.equal(richText('short https://a.example').text, 'short a.example');
});

test('mastodon: media upload with processing poll, status with idempotency key', async () => {
  const f = mockFetch([
    [has('/api/v2/media'), () => ({ status: 202, json: { id: 'm1', url: null } })],
    [has('/api/v1/media/m1'), () => ({ json: { id: 'm1', url: 'https://cdn/x.mp4' } })],
    [has('/api/v1/statuses'), () => ({ json: { id: '99', url: 'https://mastodon.example/@evo/99' } })],
  ]);
  const res = await ADAPTERS.mastodon.publish(item('mastodon', { text: 'hello' }, [{ type: 'video', path: VIDEO_PATH, alt: 'snow' }]), ctxFor('mastodon', f));
  assert.equal(res.remoteId, '99');
  assert.equal(f.calls[0].url, 'https://mastodon.example/api/v2/media');
  assert.ok(f.calls[0].init.body instanceof FormData);
  assert.equal(f.calls[0].init.body.get('description'), 'snow');
  const st = f.calls[2];
  assert.equal(st.init.headers.authorization, 'Bearer masto-tok');
  assert.equal(st.init.headers['idempotency-key'], 'evo-abc123-h1');
  assert.deepEqual(body(st), { status: 'hello', visibility: 'public', language: 'en', media_ids: ['m1'] });
});

test('x: chunked media upload then tweet with media id', async () => {
  const f = mockFetch([
    [has('/2/media/upload/initialize'), () => ({ json: { data: { id: 'M1' } } })],
    [has('/append'), () => ({ json: {} })],
    [has('/finalize'), () => ({ json: { data: { id: 'M1', processing_info: { state: 'pending', check_after_secs: 1 } } } })],
    [has('command=STATUS'), () => ({ json: { data: { processing_info: { state: 'succeeded' } } } })],
    [(u) => u.endsWith('/2/tweets'), () => ({ status: 201, json: { data: { id: 'T1' } } })],
  ]);
  const res = await ADAPTERS.x.publish(item('x', { text: 'hi' }, [{ type: 'video', path: VIDEO_PATH }]), ctxFor('x', f));
  assert.equal(res.url, 'https://x.com/i/web/status/T1');
  assert.deepEqual(body(f.calls[0]), { media_type: 'video/mp4', total_bytes: 1000, media_category: 'tweet_video' });
  assert.equal(f.calls[1].init.body.get('segment_index'), '0');
  const tw = f.calls.at(-1);
  assert.equal(tw.init.headers.authorization, 'Bearer x-tok');
  assert.deepEqual(body(tw), { text: 'hi', media: { media_ids: ['M1'] } });
});

test('x: refreshes an OAuth 2.0 token when client id and refresh token are set', async () => {
  const f = mockFetch([
    [has('/2/oauth2/token'), () => ({ json: { access_token: 'fresh', refresh_token: 'r2', expires_in: 7200 } })],
    [(u) => u.endsWith('/2/tweets'), () => ({ status: 201, json: { data: { id: 'T2' } } })],
  ]);
  const ctx = ctxFor('x', f);
  ctx.env = { X_CLIENT_ID: 'cid', X_REFRESH_TOKEN: 'r1' };
  await ADAPTERS.x.publish(item('x', { text: 'hi' }), ctx);
  assert.equal(new URLSearchParams(f.calls[0].init.body).get('grant_type'), 'refresh_token');
  assert.equal(f.calls[1].init.headers.authorization, 'Bearer fresh');
  const saved = JSON.parse(fs.readFileSync(path.join(ctx.config.dataDir, 'secrets', 'x-token.json'), 'utf8'));
  assert.equal(saved.refresh_token, 'r2');
  assert.equal((fs.statSync(path.join(ctx.config.dataDir, 'secrets', 'x-token.json')).mode & 0o777).toString(8), '600');
});

test('youtube: token refresh, resumable session, upload; #Shorts appended', async () => {
  const f = mockFetch([
    [has('oauth2.googleapis.com/token'), () => ({ json: { access_token: 'ya29' } })],
    [has('uploadType=resumable'), () => ({ headers: { location: 'https://upload.example/session1' }, json: {} })],
    [has('upload.example/session1'), () => ({ json: { id: 'VID1' } })],
  ]);
  const res = await ADAPTERS.youtube.publish(item('youtube', { title: 'A severe winter', description: 'desc' }, [{ type: 'video', path: VIDEO_PATH }]), ctxFor('youtube', f));
  assert.equal(res.url, 'https://www.youtube.com/shorts/VID1');
  const tok = new URLSearchParams(f.calls[0].init.body);
  assert.equal(tok.get('refresh_token'), 'yrefresh');
  const meta = body(f.calls[1]);
  assert.equal(meta.snippet.title, 'A severe winter #Shorts');
  assert.equal(meta.status.privacyStatus, 'public');
  assert.equal(meta.status.selfDeclaredMadeForKids, false);
  assert.equal(f.calls[1].init.headers['x-upload-content-length'], '1000');
  assert.equal(f.calls[2].init.method, 'PUT');
});

test('reddit: script-app token, self post to own subreddit, descriptive user agent', async () => {
  const f = mockFetch([
    [has('/api/v1/access_token'), () => ({ json: { access_token: 'rt' } })],
    [has('/api/submit'), () => ({ json: { json: { errors: [], data: { name: 't3_x', url: 'https://reddit.com/r/EvolutionMMO/x' } } } })],
  ]);
  const it = item('reddit', { title: 'T', body: 'B' }, [], { target: 'EvolutionMMO', approval: {} });
  const res = await ADAPTERS.reddit.publish(it, ctxFor('reddit', f));
  assert.equal(res.remoteId, 't3_x');
  assert.equal(f.calls[0].init.headers.authorization, `Basic ${Buffer.from('rid:rsecret').toString('base64')}`);
  assert.equal(f.calls[1].init.headers['user-agent'], 'script:evo-marketing:0.1.0 (by /u/owner)');
  const p = new URLSearchParams(f.calls[1].init.body);
  assert.equal(p.get('sr'), 'EvolutionMMO');
  assert.equal(p.get('kind'), 'self');
});

test('reddit: refuses a third-party subreddit that was not approved for it', async () => {
  const f = mockFetch([]);
  const it = item('reddit', { title: 'T', body: 'B' }, [], { target: 'IndieGaming', approval: {} });
  await assert.rejects(ADAPTERS.reddit.publish(it, ctxFor('reddit', f)), /not an own subreddit/);
  assert.equal(f.calls.length, 0);
});

test('discord: webhook with attachment, mentions disabled, wait=true', async () => {
  const f = mockFetch([[has('discord.com/api/webhooks'), () => ({ json: { id: 'D1' } })]]);
  await ADAPTERS.discord.publish(item('discord', { content: '**Hi** @everyone' }, [{ type: 'image', path: IMG_PATH }]), ctxFor('discord', f));
  assert.equal(f.calls[0].url, 'https://discord.com/api/webhooks/123/secret-token?wait=true');
  const payload = JSON.parse(f.calls[0].init.body.get('payload_json'));
  assert.deepEqual(payload.allowed_mentions, { parse: [] });
  assert.ok(f.calls[0].init.body.get('files[0]'));
  assert.match(describeRequest(f.calls[0].url).url, /webhooks\/123\/\*\*\*/);
});

test('email: Buttondown draft by default', async () => {
  const f = mockFetch([[has('api.buttondown.com'), () => ({ status: 201, json: { id: 'E1' } })]]);
  await ADAPTERS.email.publish(item('email', { subject: 'S', body: 'B' }), ctxFor('email', f, { email: { provider: 'buttondown' } }));
  assert.equal(f.calls[0].init.headers.authorization, 'Token bd-key');
  assert.equal(body(f.calls[0]).status, 'draft');
});

test('email: SMTP only reaches opted-in subscribers (dry-run count)', async () => {
  const list = path.join(dir, 'list.json');
  fs.writeFileSync(list, JSON.stringify([{ email: 'a@x.example', optedIn: true }, { email: 'b@x.example' }, { email: 'c@x.example', optedIn: false }]));
  const config = testConfig({ email: { provider: 'smtp', from: 'me@x.example', listFile: list, unsubscribeUrl: 'https://x.example/unsub' } }, ENV);
  const logs = [];
  const ctx = makeCtx('email', { config, live: false, log: (m) => logs.push(m), now: new Date() });
  await ADAPTERS.email.publish(item('email', { subject: 'S', body: 'B' }), ctx);
  assert.match(logs.join('\n'), /1 opted-in subscriber\(s\) \(2 skipped\)/);
});

test('tiktok: inbox upload by default with content-range chunks', async () => {
  const f = mockFetch([
    [has('/inbox/video/init/'), () => ({ json: { data: { publish_id: 'P1', upload_url: 'https://upload.tiktok.example/u1' } } })],
    [has('upload.tiktok.example'), () => ({ status: 201, json: {} })],
    [has('status/fetch'), () => ({ json: { data: { status: 'SEND_TO_USER_INBOX' } } })],
  ]);
  const res = await ADAPTERS.tiktok.publish(item('tiktok', { caption: 'c' }, [{ type: 'video', path: VIDEO_PATH }]), ctxFor('tiktok', f));
  assert.equal(res.remoteId, 'P1');
  assert.deepEqual(body(f.calls[0]).source_info, { source: 'FILE_UPLOAD', video_size: 1000, chunk_size: 1000, total_chunk_count: 1 });
  assert.equal(f.calls[1].init.headers['content-range'], 'bytes 0-999/1000');
});

test('tiktok: direct mode checks privacy options from creator_info', async () => {
  const f = mockFetch([[has('creator_info'), () => ({ json: { data: { privacy_level_options: ['SELF_ONLY'] } } })]]);
  const ctx = ctxFor('tiktok', f, { tiktok: { mode: 'direct', privacyLevel: 'PUBLIC_TO_EVERYONE' } });
  await assert.rejects(ADAPTERS.tiktok.publish(item('tiktok', { caption: 'c' }, [{ type: 'video', path: VIDEO_PATH }]), ctx), /not offered/);
});

test('tiktok chunk plan', () => {
  assert.deepEqual(chunkPlan(1000), { chunkSize: 1000, count: 1 });
  assert.deepEqual(chunkPlan(100 * 1024 * 1024), { chunkSize: 10 * 1024 * 1024, count: 10 });
  assert.deepEqual(chunkPlan(105 * 1024 * 1024), { chunkSize: 10 * 1024 * 1024, count: 10 });
});

test('instagram: reels container from a public URL, poll, publish', async () => {
  const f = mockFetch([
    [(u, i) => u.endsWith('/1789/media') && i.method === 'POST', () => ({ json: { id: 'C1' } })],
    [has('C1?fields=status_code'), () => ({ json: { status_code: 'FINISHED' } })],
    [has('/media_publish'), () => ({ json: { id: 'IG1' } })],
    [has('IG1?fields=permalink'), () => ({ json: { permalink: 'https://instagram.com/reel/IG1' } })],
  ]);
  const ctx = ctxFor('instagram', f, { instagram: { publicMediaBaseUrl: 'https://cdn.example/m/' } });
  const res = await ADAPTERS.instagram.publish(item('instagram', { caption: 'cap' }, [{ type: 'video', path: VIDEO_PATH }]), ctx);
  assert.equal(res.url, 'https://instagram.com/reel/IG1');
  assert.equal(f.calls[0].url, 'https://graph.facebook.com/v23.0/1789/media');
  const p = new URLSearchParams(f.calls[0].init.body);
  assert.equal(p.get('media_type'), 'REELS');
  assert.equal(p.get('video_url'), 'https://cdn.example/m/clip.mp4');
  assert.equal(new URLSearchParams(f.calls[2].init.body).get('creation_id'), 'C1');
});

test('http: waits once on a short 429, stops on a long one, never retries more', async () => {
  let n = 0;
  const slept = [];
  const f1 = async () => (++n === 1 ? new Response('{}', { status: 429, headers: { 'retry-after': '2' } }) : new Response('{"ok":1}', { status: 200 }));
  const h1 = createHttp({ platform: 't', live: true, fetch: f1, minIntervalMs: 0, sleep: async (ms) => slept.push(ms) });
  assert.deepEqual((await h1.request('https://a.example')).json, { ok: 1 });
  assert.deepEqual(slept, [2000]);
  const f2 = async () => new Response('{}', { status: 429, headers: { 'retry-after': '900' } });
  const h2 = createHttp({ platform: 't', live: true, fetch: f2, minIntervalMs: 0, sleep: async () => {} });
  await assert.rejects(h2.request('https://a.example'), RateLimitError);
  let calls = 0;
  const f3 = async () => (calls++, new Response('{}', { status: 429, headers: { 'retry-after': '1' } }));
  const h3 = createHttp({ platform: 't', live: true, fetch: f3, minIntervalMs: 0, sleep: async () => {} });
  await assert.rejects(h3.request('https://a.example'), RateLimitError);
  assert.equal(calls, 2);
});

test('http: rate-limit header formats', () => {
  const now = Date.parse('2026-11-17T10:00:00Z');
  const r = (h) => new Response(null, { status: 429, headers: h });
  assert.equal(retryAfterSeconds(r({ 'x-rate-limit-reset': String(now / 1000 + 30) }), now), 30);
  assert.equal(retryAfterSeconds(r({ 'x-ratelimit-reset': '45' }), now), 45);
  assert.equal(retryAfterSeconds(r({ 'x-ratelimit-reset': '2026-11-17T10:01:00Z' }), now), 60);
});

test('http: dry-run sends nothing and redacts secrets in the record', async () => {
  const h = createHttp({ platform: 't', live: false, fetch: () => assert.fail('network used in dry-run'), fake: () => ({ json: { id: 1 } }) });
  const r = await h.request('https://a.example/?access_token=abc', { method: 'POST', headers: { authorization: 'Bearer s3cret' }, body: JSON.stringify({ password: 'pw', text: 'hi' }) });
  assert.deepEqual(r.json, { id: 1 });
  const rec = JSON.stringify(h.records);
  assert.doesNotMatch(rec, /s3cret|abc|"pw"/);
  assert.match(rec, /"text":"hi"/);
});
