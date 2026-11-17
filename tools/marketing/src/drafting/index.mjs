import crypto from 'node:crypto';
import { PLATFORMS } from '../platforms.mjs';
import { BRAND } from '../style.mjs';
import { mediaForStory, factCorpus } from '../stories.mjs';
import { utmLink } from '../utm.mjs';
import { suggestHashtags } from '../hashtags.mjs';
import { templateCopy } from './templates.mjs';
import { assemble, budgets } from './assemble.mjs';
import { validateDraft } from './validate.mjs';
import { claudeCopy } from './claude.mjs';

export function contentHash(platform, fields) {
  const norm = Object.keys(fields)
    .sort()
    .map((k) => `${k}=${String(fields[k]).replace(/\?utm_[^\s]*/g, '').replace(/\s+/g, ' ').trim().toLowerCase()}`)
    .join('\n');
  return crypto.createHash('sha256').update(`${platform}\n${norm}`).digest('hex').slice(0, 16);
}

function pickMedia(platform, media) {
  const p = PLATFORMS[platform];
  const videos = media.filter((m) => m.type === 'video');
  const images = media.filter((m) => m.type === 'image');
  if (p.media.required || (p.media.video && videos.length)) {
    const v = videos.find((m) => m.vertical) || videos[0];
    if (p.kind === 'video') return v ? [v] : [];
    if (v) return [v];
  }
  if (p.media.image && images.length) return images.slice(0, p.media.maxImages || 1);
  return [];
}

function draftContext(story, platform, config) {
  const link = utmLink(config.site.url, { source: platform, campaign: config.site.campaign, content: story.id });
  const hashtags = suggestHashtags(story.kind, platform, BRAND.coreHashtags, PLATFORMS[platform].maxHashtags);
  return { link, hashtags };
}

/**
 * Draft every requested platform for one story.
 * Returns drafts (one per platform) ready for the queue.
 */
export async function draftStory(story, manifest, { platforms, config, client = null, now = new Date() }) {
  const media = mediaForStory(story, manifest);
  const corpus = factCorpus(story);
  const usable = platforms.filter((p) => !(PLATFORMS[p].media.required && !media.some((m) => m.type === 'video')));
  const skipped = platforms.filter((p) => !usable.includes(p));
  const ctx = Object.fromEntries(usable.map((p) => [p, draftContext(story, p, config)]));
  const picked = Object.fromEntries(usable.map((p) => [p, pickMedia(p, media)]));

  const build = (p, copy, source, model) => {
    const fields = assemble(p, copy, ctx[p], config);
    const d = {
      storyId: story.id,
      platform: p,
      fields,
      media: picked[p].map((m) => ({ path: m.path, type: m.type, alt: m.alt || story.title, vertical: m.vertical, durationS: m.durationS })),
      link: ctx[p].link,
      hashtags: ctx[p].hashtags,
      source,
      model: model || null,
      corpus,
      createdAt: now.toISOString(),
    };
    d.validation = validateDraft(d, { corpus, config });
    d.contentHash = contentHash(p, fields);
    return d;
  };

  const drafts = {};
  const notes = [];
  if (client && usable.length) {
    const b = Object.fromEntries(usable.map((p) => [p, budgets(p, ctx[p], config)]));
    let feedback = null;
    for (let attempt = 0; attempt < 2; attempt++) {
      try {
        const { copy, model } = await claudeCopy({ client, model: config.claude.model, effort: config.claude.effort, story, media, budgets: b, feedback });
        const failed = [];
        for (const p of usable) {
          if (drafts[p]?.validation.ok) continue;
          if (!copy[p]) continue;
          const d = build(p, copy[p], 'claude', model);
          drafts[p] = d;
          if (!d.validation.ok) failed.push(`${p}: ${d.validation.errors.join('; ')}`);
        }
        if (!failed.length) break;
        feedback = failed.join('\n');
      } catch (e) {
        notes.push(`claude: ${e.message}`);
        break;
      }
    }
  }
  for (const p of usable) {
    if (drafts[p]?.validation.ok) continue;
    const prev = drafts[p];
    const d = build(p, templateCopy(p, story, picked[p]), 'template');
    if (prev) d.validation.warnings.unshift(`Claude copy failed checks (${prev.validation.errors.join('; ')}); template used`);
    drafts[p] = d;
  }
  return { drafts: usable.map((p) => drafts[p]), skipped, notes };
}
