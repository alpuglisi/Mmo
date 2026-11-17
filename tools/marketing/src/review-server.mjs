import http from 'node:http';
import fs from 'node:fs';
import crypto from 'node:crypto';
import { PLATFORMS } from './platforms.mjs';
import { mimeOf } from './adapters/media.mjs';

// Local review page: http://127.0.0.1:<port>/ . Lists drafts with their
// checks, lets a human edit, approve or reject. Binds to loopback only and
// requires a per-run token on every write.

const esc = (s) => String(s ?? '').replace(/[&<>"']/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' })[c]);

function page(queue, token, filter) {
  const items = queue.items.filter((i) => (filter ? i.status === filter : ['pending', 'approved', 'scheduled', 'failed'].includes(i.status)));
  const cards = items
    .map((i) => {
      const v = i.validation || { errors: [], warnings: [] };
      const fields = Object.entries(i.fields)
        .map(([f, val]) => `<label>${esc(f)} <small>(max ${PLATFORMS[i.platform].fields[f]?.max})</small><textarea name="${esc(f)}" rows="${Math.min(14, 2 + Math.ceil(val.length / 70))}">${esc(val)}</textarea></label>`)
        .join('');
      const media = (i.media || [])
        .map((m, n) => (m.type === 'video' ? `<video src="/media/${i.id}/${n}" controls muted width="180"></video>` : `<img src="/media/${i.id}/${n}" width="180" alt="${esc(m.alt)}">`))
        .join('');
      const reddit = i.platform === 'reddit' ? `<label>Subreddit <input name="__target" value="${esc(i.target || '')}" placeholder="default target"></label><label class="inline"><input type="checkbox" name="__third"> I read this community's rules and approve this one post there</label>` : '';
      return `<form class="card ${esc(i.status)}" data-id="${esc(i.id)}">
  <header><b>${esc(PLATFORMS[i.platform].label)}</b> · ${esc(i.status)} · ${esc(i.id)} · story ${esc(i.storyId)} · ${esc(i.source)}${i.scheduledAt ? ` · ${esc(i.scheduledAt)}` : ''}</header>
  ${v.errors.length ? `<ul class="err">${v.errors.map((e) => `<li>${esc(e)}</li>`).join('')}</ul>` : ''}
  ${v.warnings.length ? `<ul class="warn">${v.warnings.map((e) => `<li>${esc(e)}</li>`).join('')}</ul>` : ''}
  <div class="media">${media}</div>${fields}${reddit}
  <footer><button name="edit">Save edit</button> <button name="approve">Approve</button> <button name="reject">Reject</button> <span class="msg"></span></footer>
</form>`;
    })
    .join('\n');
  return `<!doctype html><html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<title>Draft review</title><style>
:root{color-scheme:light dark;--bg:#fafaf7;--fg:#1d1d1b;--card:#fff;--line:#d9d6cc;--err:#a3271c;--warn:#8a5a00}
@media (prefers-color-scheme:dark){:root{--bg:#161614;--fg:#ecebe6;--card:#21211e;--line:#3a3a35;--err:#ff8a7a;--warn:#e3b45a}}
body{margin:0;padding:16px;background:var(--bg);color:var(--fg);font:15px/1.45 system-ui,sans-serif;max-width:900px;margin-inline:auto}
.card{background:var(--card);border:1px solid var(--line);border-radius:8px;padding:12px;margin:12px 0}
textarea,input{width:100%;box-sizing:border-box;font:inherit;background:transparent;color:inherit;border:1px solid var(--line);border-radius:4px;padding:6px}
label{display:block;margin:8px 0}label.inline input{width:auto}.err{color:var(--err)}.warn{color:var(--warn)}
.media{display:flex;gap:8px;flex-wrap:wrap}button{font:inherit;padding:6px 12px}
</style></head><body><h1>Draft review</h1>
<p>${items.length} draft(s). Nothing posts from this page: approved drafts are scheduled by <code>schedule</code> and posted only by <code>publish --live</code>.</p>
<p><a href="/">Open</a> · <a href="/?status=pending">Pending</a> · <a href="/?status=approved">Approved</a> · <a href="/?status=scheduled">Scheduled</a> · <a href="/?status=rejected">Rejected</a></p>
${cards || '<p>Nothing to review.</p>'}
<script>
const TOKEN=${JSON.stringify(token)};
document.querySelectorAll('form.card').forEach(f=>f.addEventListener('submit',async e=>{
  e.preventDefault();const action=e.submitter.name;const fd=new FormData(f);const fields={};let target,third=false;
  for(const [k,v] of fd){if(k==='__target')target=v||undefined;else if(k==='__third')third=true;else fields[k]=v}
  const body=action==='edit'?{fields}:action==='reject'?{reason:prompt('Reason (optional)')||''}:{target,confirmThirdParty:third};
  const r=await fetch('/api/'+f.dataset.id+'/'+action,{method:'POST',headers:{'content-type':'application/json','x-review-token':TOKEN},body:JSON.stringify(body)});
  const j=await r.json();f.querySelector('.msg').textContent=r.ok?'done: '+j.status:'error: '+j.error;if(r.ok)setTimeout(()=>location.reload(),600);
}));
</script></body></html>`;
}

export function createReviewServer({ queue, config, save }) {
  const token = crypto.randomBytes(16).toString('hex');
  const server = http.createServer(async (req, res) => {
    const url = new URL(req.url, 'http://127.0.0.1');
    const send = (code, body, type = 'application/json') => {
      res.writeHead(code, { 'content-type': type, 'cache-control': 'no-store', 'x-frame-options': 'DENY' });
      res.end(type === 'application/json' ? JSON.stringify(body) : body);
    };
    try {
      if (req.method === 'GET' && url.pathname === '/') return send(200, page(queue, token, url.searchParams.get('status')), 'text/html; charset=utf-8');
      const mm = url.pathname.match(/^\/media\/([0-9a-f]+)\/(\d+)$/);
      if (req.method === 'GET' && mm) {
        const m = queue.get(mm[1]).media?.[+mm[2]];
        if (!m || !fs.existsSync(m.path)) return send(404, { error: 'no media' });
        res.writeHead(200, { 'content-type': mimeOf(m.path) });
        return fs.createReadStream(m.path).pipe(res);
      }
      const am = url.pathname.match(/^\/api\/([0-9a-f]+)\/(approve|reject|edit)$/);
      if (req.method === 'POST' && am) {
        if (req.headers['x-review-token'] !== token) return send(403, { error: 'bad token' });
        let raw = '';
        for await (const c of req) raw += c;
        const body = raw ? JSON.parse(raw) : {};
        const [, id, action] = am;
        let item;
        if (action === 'approve') item = queue.approve(id, { actor: 'web', target: body.target, confirmThirdParty: !!body.confirmThirdParty, config });
        if (action === 'reject') item = queue.reject(id, { actor: 'web', reason: body.reason });
        if (action === 'edit') item = queue.edit(id, body.fields || {}, { actor: 'web', config });
        save();
        return send(200, { status: item.status, validation: item.validation });
      }
      send(404, { error: 'not found' });
    } catch (e) {
      send(400, { error: e.message });
    }
  });
  return { server, token };
}
