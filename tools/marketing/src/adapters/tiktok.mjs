// TikTok Content Posting API. Needs an approved TikTok developer app with the
// video.upload (inbox/draft) and, for direct posting, video.publish scopes,
// and a user access token for the owner's account. Until TikTok audits the
// app, direct posts are forced to private (SELF_ONLY).
//
// Modes (config.tiktok.mode):
//   inbox  (default) upload as a draft to the owner's TikTok inbox; the owner
//          finishes and posts it in the app (sound, cover, privacy).
//   direct publish directly with the privacy level in config.tiktok.privacyLevel,
//          which must be one the creator_info query offers.
// Without enabled + appApproved in config the publisher uses manual export.

const API = 'https://open.tiktokapis.com';
const MAX_SINGLE = 64 * 1024 * 1024;
const CHUNK = 10 * 1024 * 1024;

export function chunkPlan(size) {
  if (size <= MAX_SINGLE) return { chunkSize: size, count: 1 };
  const count = Math.floor(size / CHUNK); // last chunk absorbs the remainder
  return { chunkSize: CHUNK, count };
}

export default {
  name: 'tiktok',
  env: ['TIKTOK_ACCESS_TOKEN'],
  minIntervalMs: 3000,
  gated: true,

  fake(url) {
    if (url.includes('creator_info')) return { json: { data: { privacy_level_options: ['PUBLIC_TO_EVERYONE', 'SELF_ONLY'], max_video_post_duration_sec: 600 }, error: { code: 'ok' } } };
    if (url.includes('/init/')) return { json: { data: { publish_id: 'v_pub_dry', upload_url: 'https://open-upload.tiktokapis.com/upload/?upload_id=dry' }, error: { code: 'ok' } } };
    if (url.includes('open-upload')) return { status: 201, json: {} };
    if (url.includes('status/fetch')) return { json: { data: { status: 'PUBLISH_COMPLETE' }, error: { code: 'ok' } } };
    return { json: { error: { code: 'ok' } } };
  },

  async publish(item, ctx) {
    const token = ctx.env.TIKTOK_ACCESS_TOKEN;
    if (!token) throw Object.assign(new Error('tiktok: TIKTOK_ACCESS_TOKEN not set'), { code: 'MISSING_SECRET' });
    const auth = { authorization: `Bearer ${token}`, 'content-type': 'application/json; charset=UTF-8' };
    const video = (item.media || []).find((m) => m.type === 'video');
    if (!video) throw new Error('tiktok: no video');
    const f = ctx.readMedia(video.path);
    const plan = chunkPlan(f.size);
    const source_info = { source: 'FILE_UPLOAD', video_size: f.size, chunk_size: plan.chunkSize, total_chunk_count: plan.count };
    const cfg = ctx.config.tiktok;
    let init;
    if (cfg.mode === 'direct') {
      const info = await ctx.http.request(`${API}/v2/post/publish/creator_info/query/`, { method: 'POST', headers: auth });
      const options = info.json.data.privacy_level_options || [];
      const privacy = cfg.privacyLevel || 'SELF_ONLY';
      if (!options.includes(privacy)) throw new Error(`tiktok: privacy ${privacy} not offered (${options.join(', ')})`);
      init = await ctx.http.request(`${API}/v2/post/publish/video/init/`, {
        method: 'POST',
        headers: auth,
        body: JSON.stringify({
          post_info: { title: item.fields.caption, privacy_level: privacy, disable_duet: false, disable_comment: false, disable_stitch: false, is_aigc: false },
          source_info,
        }),
      });
    } else {
      init = await ctx.http.request(`${API}/v2/post/publish/inbox/video/init/`, { method: 'POST', headers: auth, body: JSON.stringify({ source_info }) });
    }
    const { publish_id, upload_url } = init.json.data;
    for (let i = 0; i < plan.count; i++) {
      const start = i * plan.chunkSize;
      const end = i === plan.count - 1 ? f.size : start + plan.chunkSize;
      await ctx.http.request(upload_url, {
        method: 'PUT',
        headers: { 'content-type': f.mime, 'content-range': `bytes ${start}-${end - 1}/${f.size}`, 'content-length': String(end - start) },
        body: f.bytes.subarray(start, end),
      });
    }
    const st = await ctx.http.request(`${API}/v2/post/publish/status/fetch/`, { method: 'POST', headers: auth, body: JSON.stringify({ publish_id }) });
    return {
      remoteId: publish_id,
      url: null,
      note: cfg.mode === 'direct' ? `status ${st.json.data?.status}` : 'uploaded to the TikTok inbox; finish the post in the app (caption is in the export folder)',
    };
  },

  async metrics() {
    return null; // needs video.list scope and the post's video id; see PUBLISHING.md
  },
};
