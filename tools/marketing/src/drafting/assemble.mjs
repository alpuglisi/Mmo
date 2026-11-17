import { PLATFORMS } from '../platforms.mjs';
import { truncateTo } from '../text.mjs';
import { limitFor } from './validate.mjs';

// Turns the "copy" (what a writer or Claude produced) into final post fields
// by appending the tracked link and hashtags, fitting everything to limits.
// The writer is told the budget left after the suffix (see budgets()).

const tagStr = (tags) => tags.map((t) => `#${t}`).join(' ');

function suffixFor(platform, field, { link, hashtags }) {
  switch (platform) {
    case 'x':
    case 'bluesky':
    case 'mastodon':
      return `\n\n${link}${hashtags.length ? ' ' + tagStr(hashtags) : ''}`;
    case 'tiktok':
    case 'instagram':
      // Links in captions are not clickable; tags only.
      return field === 'caption' && hashtags.length ? `\n\n${tagStr(hashtags)}` : '';
    case 'youtube':
      if (field === 'description') return `\n\n${link}${hashtags.length ? '\n\n' + tagStr(hashtags) : ''}`;
      return '';
    case 'reddit':
      return field === 'body' ? `\n\n---\n\nMore about the game: ${link}` : '';
    case 'discord':
      return `\n${link}`;
    case 'devlog':
    case 'email':
      return field === 'body' ? `\n\n${link}` : '';
    default:
      return '';
  }
}

/** Characters the writer may use per field, after the suffix. */
export function budgets(platform, ctx, config) {
  const out = {};
  for (const field of Object.keys(PLATFORMS[platform].fields)) {
    const lim = limitFor(platform, field, config);
    out[field] = Math.max(0, lim.max - lim.count(suffixFor(platform, field, ctx)));
  }
  return out;
}

export function assemble(platform, copy, ctx, config) {
  const fields = {};
  for (const field of Object.keys(PLATFORMS[platform].fields)) {
    const lim = limitFor(platform, field, config);
    const suffix = suffixFor(platform, field, ctx);
    let body = String(copy[field] ?? '').trim();
    const room = lim.max - lim.count(suffix);
    body = truncateTo(body, room, lim.count);
    if (lim.noAngleBrackets) body = body.replace(/[<>]/g, '');
    fields[field] = body + suffix;
  }
  return fields;
}
