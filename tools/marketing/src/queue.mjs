import fs from 'node:fs';
import path from 'node:path';
import crypto from 'node:crypto';
import { validateDraft } from './drafting/validate.mjs';
import { contentHash } from './drafting/index.mjs';

// A local JSON approval queue. Life cycle:
//
//   pending ──approve──▶ approved ──schedule──▶ scheduled ──publish──▶ posted
//      │                    │                      │           └────▶ failed ─▶ (retry: approved)
//      └──reject──▶ rejected ◀──────────────reject─┘
//                     exported (manual-upload platforms)
//
// Nothing publishes unless a human approved it. Editing an approved or
// scheduled draft sends it back to pending. Every change is appended to the
// item's history.

export const STATUSES = ['pending', 'approved', 'scheduled', 'posting', 'posted', 'failed', 'rejected', 'exported'];
const ACTIVE = new Set(['pending', 'approved', 'scheduled', 'posting', 'posted', 'exported']);

export class Queue {
  constructor(file) {
    this.file = file;
    this.data = { version: 1, items: [] };
    if (fs.existsSync(file)) this.data = JSON.parse(fs.readFileSync(file, 'utf8'));
  }

  static open(dataDir) {
    return new Queue(path.join(dataDir, 'queue.json'));
  }

  save() {
    fs.mkdirSync(path.dirname(this.file), { recursive: true });
    const tmp = `${this.file}.${process.pid}.tmp`;
    fs.writeFileSync(tmp, JSON.stringify(this.data, null, 2) + '\n');
    fs.renameSync(tmp, this.file);
  }

  get items() {
    return this.data.items;
  }

  get(id) {
    const it = this.items.find((i) => i.id === id || i.id.startsWith(id));
    if (!it) throw new Error(`no queue item ${id}`);
    return it;
  }

  list({ status, platform } = {}) {
    return this.items.filter((i) => (!status || i.status === status) && (!platform || i.platform === platform));
  }

  /** Duplicate check: same content on the same platform, or the same story
   * already queued for that platform (rejected items don't count). */
  findDuplicate(draft) {
    return this.items.find(
      (i) => ACTIVE.has(i.status) && i.platform === draft.platform && (i.contentHash === draft.contentHash || i.storyId === draft.storyId),
    );
  }

  add(draft, { force = false, actor = 'drafter', now = new Date() } = {}) {
    const dup = this.findDuplicate(draft);
    if (dup && !force) return { added: false, duplicateOf: dup.id };
    // Only draft content is copied; queue state always starts fresh.
    const { id: _i, status: _s, history: _h, approval: _a, result: _r, scheduledAt: _t, ...content } = draft;
    const item = {
      id: crypto.randomBytes(4).toString('hex'),
      status: 'pending',
      ...content,
      scheduledAt: null,
      target: draft.target ?? null,
      approval: null,
      result: null,
      history: [{ at: now.toISOString(), actor, action: 'created', source: draft.source }],
    };
    this.items.push(item);
    return { added: true, item };
  }

  #log(item, actor, action, extra = {}, now = new Date()) {
    item.history.push({ at: now.toISOString(), actor, action, ...extra });
  }

  approve(id, { actor = 'reviewer', target, confirmThirdParty = false, config, now = new Date() } = {}) {
    const item = this.get(id);
    if (!['pending', 'failed'].includes(item.status)) throw new Error(`${item.id} is ${item.status}; only pending or failed drafts can be approved`);
    if (!item.validation?.ok) throw new Error(`${item.id} fails checks: ${item.validation.errors.join('; ')}. Edit it first.`);
    if (target !== undefined) item.target = target;
    if (item.platform === 'reddit') {
      const t = resolveRedditTarget(item.target, config);
      item.target = t.target;
      if (!t.own && !confirmThirdParty)
        throw new Error(
          `${t.target} is not one of your own subreddits. Read its rules, then approve with --confirm-third-party to post there once.`,
        );
      item.approval = { thirdParty: !t.own };
    }
    item.status = 'approved';
    item.approval = { ...(item.approval || {}), by: actor, at: now.toISOString(), contentHash: item.contentHash };
    this.#log(item, actor, 'approved', { target: item.target }, now);
    return item;
  }

  reject(id, { actor = 'reviewer', reason = '', now = new Date() } = {}) {
    const item = this.get(id);
    if (['posted', 'posting'].includes(item.status)) throw new Error(`${item.id} is already ${item.status}`);
    item.status = 'rejected';
    item.scheduledAt = null;
    this.#log(item, actor, 'rejected', { reason }, now);
    return item;
  }

  edit(id, fields, { actor = 'reviewer', corpus = '', config, now = new Date() } = {}) {
    const item = this.get(id);
    if (['posted', 'posting'].includes(item.status)) throw new Error(`${item.id} is already ${item.status}`);
    const before = { ...item.fields };
    for (const [k, v] of Object.entries(fields)) {
      if (!(k in item.fields)) throw new Error(`${item.platform} has no field ${k} (fields: ${Object.keys(item.fields).join(', ')})`);
      item.fields[k] = v;
    }
    // Human edits get the same checks as generated copy, facts included.
    item.validation = validateDraft(item, { corpus: corpus || item.corpus || '', config });
    item.contentHash = contentHash(item.platform, item.fields);
    item.status = 'pending';
    item.scheduledAt = null;
    item.approval = null;
    this.#log(item, actor, 'edited', { before }, now);
    return item;
  }

  mark(id, status, extra = {}, { actor = 'publisher', now = new Date() } = {}) {
    const item = this.get(id);
    item.status = status;
    Object.assign(item, extra);
    this.#log(item, actor, status, {}, now);
    return item;
  }
}

export function resolveRedditTarget(target, config = {}) {
  const r = config.reddit || {};
  const own = new Set((r.ownSubreddits || []).map((s) => s.toLowerCase().replace(/^r\//, '')));
  if (r.username) own.add(`u_${r.username.toLowerCase()}`);
  let t = target || r.defaultTarget || (r.username ? `u_${r.username}` : null);
  if (!t) throw new Error('reddit: no target; set reddit.defaultTarget or reddit.username in the config');
  t = t.replace(/^\/?r\//i, '').replace(/^\/?u\//i, 'u_');
  return { target: t, own: own.has(t.toLowerCase()) };
}
