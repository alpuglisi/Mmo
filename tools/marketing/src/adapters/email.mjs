import fs from 'node:fs';

// Newsletter. Providers:
//   buttondown  Buttondown API (subscribers opted in through Buttondown).
//               Creates a draft by default; config.email.sendNow sends it.
//   smtp        Any SMTP server via nodemailer (optional dependency), to
//               config.email.listFile: a JSON array of
//               { email, optedIn: true, optedInAt } records. Addresses without
//               optedIn are skipped. Requires config.email.unsubscribeUrl.
//   manual      (default) Nothing is sent; use `export` to get the text.

export default {
  name: 'email',
  env: [],
  minIntervalMs: 1000,

  fake(url) {
    if (url.includes('buttondown')) return { status: 201, json: { id: 'dry-email-1', status: 'draft' } };
    return { json: {} };
  },

  async publish(item, ctx) {
    const cfg = ctx.config.email;
    if (cfg.provider === 'buttondown') {
      if (!ctx.env.BUTTONDOWN_API_KEY) throw Object.assign(new Error('email: BUTTONDOWN_API_KEY not set'), { code: 'MISSING_SECRET' });
      const r = await ctx.http.request('https://api.buttondown.com/v1/emails', {
        method: 'POST',
        headers: { authorization: `Token ${ctx.env.BUTTONDOWN_API_KEY}`, 'content-type': 'application/json' },
        body: JSON.stringify({ subject: item.fields.subject, body: item.fields.body, status: cfg.sendNow ? 'about_to_send' : 'draft' }),
      });
      return { remoteId: r.json.id, url: null, note: cfg.sendNow ? 'sent' : 'saved as a Buttondown draft; send it from Buttondown' };
    }
    if (cfg.provider === 'smtp') return this.smtp(item, ctx);
    throw Object.assign(new Error('email: provider is "manual"; use export'), { code: 'MANUAL_ONLY' });
  },

  async smtp(item, ctx) {
    const cfg = ctx.config.email;
    if (!cfg.from || !cfg.listFile || !cfg.unsubscribeUrl) throw new Error('email: smtp needs email.from, email.listFile and email.unsubscribeUrl');
    const list = JSON.parse(fs.readFileSync(cfg.listFile, 'utf8'));
    const recipients = list.filter((r) => r && r.optedIn === true && /@/.test(r.email));
    const skipped = list.length - recipients.length;
    if (!ctx.live) {
      ctx.log(`dry-run: would send "${item.fields.subject}" to ${recipients.length} opted-in subscriber(s) (${skipped} skipped)`);
      ctx.http.records.push({ method: 'SMTP', url: 'smtp://(SMTP_URL)', body: { to: `${recipients.length} recipients`, subject: item.fields.subject } });
      return { remoteId: null, url: null };
    }
    if (!ctx.env.SMTP_URL) throw Object.assign(new Error('email: SMTP_URL not set'), { code: 'MISSING_SECRET' });
    const { default: nodemailer } = await import('nodemailer');
    const t = nodemailer.createTransport(ctx.env.SMTP_URL, { pool: true, maxConnections: 1, rateDelta: 1000, rateLimit: 1 });
    for (const r of recipients) {
      const unsub = `${cfg.unsubscribeUrl}${cfg.unsubscribeUrl.includes('?') ? '&' : '?'}email=${encodeURIComponent(r.email)}`;
      await t.sendMail({
        from: cfg.from,
        to: r.email,
        subject: item.fields.subject,
        text: `${item.fields.body}\n\nUnsubscribe: ${unsub}`,
        headers: { 'List-Unsubscribe': `<${unsub}>`, 'List-Unsubscribe-Post': 'List-Unsubscribe=One-Click' },
      });
    }
    t.close();
    return { remoteId: null, url: null, note: `sent to ${recipients.length}` };
  },

  async metrics() {
    return null;
  },
};
