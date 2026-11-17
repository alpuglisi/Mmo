// Text measurement helpers. Platforms count length differently:
// - Bluesky: graphemes (300).
// - X: "weighted" characters: most Latin = 1, CJK/emoji = 2, every URL = 23.
// - Mastodon: characters, every URL = 23 regardless of length.
// - Most others: UTF-16 code units or plain characters (we use code points).

const segmenter = new Intl.Segmenter('en', { granularity: 'grapheme' });

export const URL_RE = /https?:\/\/[^\s<>()]+[^\s<>().,!?;:'"]/g;
export const HASHTAG_RE = /(^|[\s(])#([\p{L}\p{N}_]*[\p{L}_][\p{L}\p{N}_]*)/gu;

export function graphemes(text) {
  let n = 0;
  for (const _ of segmenter.segment(text)) n++;
  return n;
}

export function codePoints(text) {
  return [...text].length;
}

export function utf8Bytes(text) {
  return Buffer.byteLength(text, 'utf8');
}

/** Replace every URL with a placeholder of `len` characters. */
function withUrlsAs(text, len) {
  return text.replace(URL_RE, 'x'.repeat(len));
}

/** X (twitter-text v3) weighted length: URLs count 23, chars outside the
 * light ranges (Latin, general punctuation...) count 2. */
export function xWeightedLength(text) {
  const t = withUrlsAs(text, 1);
  let urls = (text.match(URL_RE) || []).length;
  let n = 0;
  for (const seg of segmenter.segment(t)) {
    const cp = seg.segment.codePointAt(0);
    const light =
      (cp >= 0x0000 && cp <= 0x10ff) ||
      (cp >= 0x2000 && cp <= 0x200d) ||
      (cp >= 0x2010 && cp <= 0x201f) ||
      (cp >= 0x2032 && cp <= 0x2037);
    n += light && seg.segment.length <= 2 ? 1 : 2;
  }
  // each URL placeholder counted 1 above; make it 23
  return n + urls * 22;
}

export function mastodonLength(text) {
  return codePoints(withUrlsAs(text, 23));
}

export const counters = {
  graphemes,
  codePoints,
  utf8Bytes,
  x: xWeightedLength,
  mastodon: mastodonLength,
};

/** Truncate `text` so that `measure(text) <= max`, cutting at a word boundary
 * and appending an ellipsis. Never cuts inside a URL. */
export function truncateTo(text, max, measure = codePoints) {
  if (measure(text) <= max) return text;
  const words = text.split(/(\s+)/);
  let out = '';
  for (const w of words) {
    const next = out + w;
    if (measure(next.trimEnd() + '…') > max) break;
    out = next;
  }
  out = out.trimEnd().replace(/[,;:\-–—]+$/, '');
  // Prefer ending on a whole sentence when that keeps most of the room.
  const sentenceEnd = Math.max(out.lastIndexOf('. '), out.lastIndexOf('! '), out.lastIndexOf('? '));
  if (sentenceEnd > 0 && measure(out.slice(0, sentenceEnd + 1)) >= max * 0.4) return out.slice(0, sentenceEnd + 1);
  if (!out) {
    // single giant word: hard cut by graphemes
    const segs = [...segmenter.segment(text)].map((s) => s.segment);
    while (segs.length && measure(segs.join('') + '…') > max) segs.pop();
    return segs.join('') + '…';
  }
  return out + '…';
}

export function extractHashtags(text) {
  const tags = [];
  for (const m of text.matchAll(HASHTAG_RE)) tags.push(m[2]);
  return tags;
}

export function slug(s) {
  return String(s)
    .toLowerCase()
    .normalize('NFKD')
    .replace(/[^\p{L}\p{N}]+/gu, '-')
    .replace(/^-+|-+$/g, '')
    .slice(0, 60);
}
