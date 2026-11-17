import { requireSecrets } from '../config.mjs';
import { blobOf } from './media.mjs';
import { PLATFORMS } from '../platforms.mjs';

// Discord: an incoming webhook on a channel of the owner's server
// (Server settings → Integrations → Webhooks). Mentions are disabled
// unless config.discord.allowEveryone is true.

export default {
  name: 'discord',
  env: ['DISCORD_WEBHOOK_URL'],
  minIntervalMs: 1500,

  fake() {
    return { json: { id: '1300000000000000000', channel_id: '1200000000000000000' } };
  },

  async publish(item, ctx) {
    const { DISCORD_WEBHOOK_URL: hook } = requireSecrets('discord', ctx.env, this.env);
    const payload = {
      content: item.fields.content,
      allowed_mentions: { parse: ctx.config.discord.allowEveryone ? ['everyone'] : [] },
    };
    if (ctx.config.discord.username) payload.username = ctx.config.discord.username;
    const url = `${hook}${hook.includes('?') ? '&' : '?'}wait=true`;
    const files = [];
    for (const m of item.media || []) {
      const f = ctx.readMedia(m.path);
      if (f.size <= PLATFORMS.discord.media.maxFileBytes) files.push(f);
      else ctx.log(`discord: ${f.name} is over the upload limit; posting without it`);
    }
    let init;
    if (files.length) {
      const form = new FormData();
      payload.attachments = files.map((f, i) => ({ id: i, filename: f.name }));
      form.append('payload_json', JSON.stringify(payload));
      files.forEach((f, i) => form.append(`files[${i}]`, blobOf(f), f.name));
      init = { method: 'POST', body: form };
    } else {
      init = { method: 'POST', headers: { 'content-type': 'application/json' }, body: JSON.stringify(payload) };
    }
    const r = await ctx.http.request(url, init);
    return { remoteId: r.json?.id ?? null, url: null };
  },

  async metrics() {
    return null; // webhooks expose no metrics
  },
};
