import { PLATFORMS } from '../platforms.mjs';
import { counters, URL_RE, HASHTAG_RE, extractHashtags } from '../text.mjs';
import { BRAND, bannedWordsIn, unsupportedClaimsIn } from '../style.mjs';

const NUM_RE = /(?<![\p{L}\d])\d[\d,]*(?:\.\d+)?%?/gu;

function normNum(s) {
  return s.replace(/,/g, '').replace(/%$/, '').replace(/\.0+$/, '');
}

// Where copy points at things rather than says things: HTML src/href
// attributes and Markdown link and image targets. Their digits (file names,
// output folders) are not claims, so the fact check skips them.
const ATTR_RE = /\b(?:src|href|poster)\s*=\s*(?:"[^"]*"|'[^']*'|[^\s>]+)/gi;
const MD_TARGET_RE = /\]\(\s*<?[^)\s>]*>?(?:\s+(?:"[^"]*"|'[^']*'))?\s*\)/g;

export function numbersIn(text) {
  const cleaned = text.replace(ATTR_RE, ' ').replace(MD_TARGET_RE, '] ').replace(URL_RE, ' ').replace(HASHTAG_RE, ' ');
  return [...cleaned.matchAll(NUM_RE)].map((m) => normNum(m[0]));
}

/** Numbers in `text` that do not appear in the fact corpus. */
export function unsupportedNumbers(text, corpus) {
  const allowed = new Set([...numbersIn(corpus), ...BRAND.allowedNumbers]);
  return [...new Set(numbersIn(text).filter((n) => !allowed.has(n)))];
}

export function limitFor(platform, field, config) {
  const spec = PLATFORMS[platform].fields[field];
  let max = spec.max;
  if (platform === 'mastodon' && config?.mastodon?.maxChars) max = config.mastodon.maxChars;
  return { ...spec, max, count: counters[spec.measure] };
}

/**
 * Validate one draft. Errors block approval; warnings are shown to the reviewer.
 * draft = { platform, fields, media: [{type, vertical, durationS, alt}] }
 */
export function validateDraft(draft, { corpus = '', config } = {}) {
  const p = PLATFORMS[draft.platform];
  const errors = [];
  const warnings = [];
  if (!p) return { ok: false, errors: [`unknown platform ${draft.platform}`], warnings };
  const allText = [];
  for (const field of Object.keys(p.fields)) {
    const v = draft.fields?.[field];
    if (typeof v !== 'string' || !v.trim()) {
      errors.push(`${field}: empty`);
      continue;
    }
    allText.push(v);
    const lim = limitFor(draft.platform, field, config);
    const n = lim.count(v);
    if (n > lim.max) errors.push(`${field}: ${n} > ${lim.max} (${lim.measure})`);
    if (lim.noAngleBrackets && /[<>]/.test(v)) errors.push(`${field}: YouTube rejects < and >`);
  }
  const text = allText.join('\n');
  const tags = extractHashtags(text);
  if (tags.length > p.maxHashtags) errors.push(`${tags.length} hashtags > ${p.maxHashtags}`);
  const banned = bannedWordsIn(text);
  if (banned.length) errors.push(`style: banned phrase(s) ${banned.join(', ')}`);
  for (const why of unsupportedClaimsIn(text)) errors.push(`claims: ${why}`);
  const emoji = (text.match(/\p{Extended_Pictographic}/gu) || []).length;
  if (emoji > 1) warnings.push(`style: ${emoji} emoji (guide says at most one)`);
  if (corpus) {
    const bad = unsupportedNumbers(text, corpus);
    if (bad.length) errors.push(`facts: number(s) ${bad.join(', ')} not found in the story data`);
  }
  const media = draft.media || [];
  if (p.media.required && !media.some((m) => m.type === 'video')) errors.push('media: this platform needs a video');
  for (const m of media) {
    if (m.type === 'video' && !p.media.video) warnings.push('media: video not attached on this platform (link only)');
    if (p.media.vertical && m.type === 'video' && m.vertical === false) warnings.push('media: video is landscape; vertical 9:16 is expected');
    if (p.media.maxVideoSeconds && m.durationS && m.durationS > p.media.maxVideoSeconds)
      errors.push(`media: ${m.durationS}s > ${p.media.maxVideoSeconds}s`);
    if (p.media.altRequired && !m.alt) warnings.push('media: no alt text');
  }
  return { ok: errors.length === 0, errors, warnings };
}
