import fs from 'node:fs';
import path from 'node:path';
import { BRAND } from './style.mjs';
import { suggestHashtags, kindKey } from './hashtags.mjs';
import { PLATFORMS } from './platforms.mjs';
import { utmLink } from './utm.mjs';
import { slug } from './text.mjs';

// Communities worth a human look. Rules change and differ per community:
// read each one's rules before posting, post as the developer, and expect
// most to allow self-promotion only in designated threads or days. Nothing
// here posts anywhere; third-party posts go through `approve --confirm-third-party`.
export const COMMUNITIES = [
  { where: 'reddit', name: 'r/IndieGaming', fit: ['default', 'timelapse', 'era'], note: 'Check the self-promotion rules in the sidebar before posting.' },
  { where: 'reddit', name: 'r/indiegames', fit: ['default', 'timelapse'], note: 'Check posting rules; video posts usually do better than links.' },
  { where: 'reddit', name: 'r/IndieDev', fit: ['default', 'devlog'], note: 'Developer audience; devlog-style posts fit.' },
  { where: 'reddit', name: 'r/gamedev', fit: ['devlog'], note: 'Strict about promotion: use the designated feedback or showcase threads.' },
  { where: 'reddit', name: 'r/alife', fit: ['species', 'speciation', 'lineage', 'extinction'], note: 'Artificial life community; explain the mechanics, not the marketing.' },
  { where: 'reddit', name: 'r/simulationgames', fit: ['default', 'catastrophe', 'timelapse'], note: 'Check self-promotion rules.' },
  { where: 'reddit', name: 'r/proceduralgeneration', fit: ['timelapse'], note: 'Only for terrain/world-generation content; explain the technique.' },
  { where: 'reddit', name: 'r/WebGames', fit: ['default'], note: 'Only once the game is playable in a browser without a paywall; check rules.' },
  { where: 'mastodon', name: '#gamedev, #indiedev, #ArtificialLife hashtags', fit: ['default', 'devlog', 'species'], note: 'Hashtags are how Mastodon discovery works; keep to 2–4.' },
  { where: 'mastodon', name: '#ScreenshotSaturday', fit: ['timelapse', 'default'], note: 'Weekly tradition: post on Saturdays only.' },
  { where: 'bluesky', name: 'Game dev feeds (posts tagged #gamedev / #indiedev)', fit: ['default', 'devlog'], note: 'Feeds pick up hashtags; 1–2 tags is plenty.' },
  { where: 'x', name: '#indiedev, #gamedev, #screenshotsaturday', fit: ['default', 'timelapse'], note: 'One or two tags; more reads as spam.' },
  { where: 'discord', name: 'Your own server announcements channel', fit: ['default', 'era', 'catastrophe'], note: 'Only your own server via webhook. For other servers, ask a moderator first.' },
];

export function suggestFor(story) {
  const k = kindKey(story.kind);
  const communities = COMMUNITIES.filter((c) => c.fit.includes(k) || c.fit.includes('default'));
  const hashtags = Object.fromEntries(
    Object.keys(PLATFORMS)
      .filter((p) => PLATFORMS[p].maxHashtags > 0)
      .map((p) => [p, suggestHashtags(story.kind, p, BRAND.coreHashtags, PLATFORMS[p].maxHashtags + 2)]),
  );
  return { story: story.id, kind: k, communities, hashtags };
}

/** Press kit: a fact sheet and media index for journalists and creators. */
export function writePressKit({ config, stories, manifest, outDir }) {
  const dir = path.join(outDir, 'press-kit');
  fs.mkdirSync(path.join(dir, 'media'), { recursive: true });
  const pk = config.pressKit || {};
  const tbd = (v) => v || '_TBD: set pressKit in marketing.config.json_';
  const media = [];
  for (const m of manifest) {
    if (fs.existsSync(m.path)) fs.copyFileSync(m.path, path.join(dir, 'media', path.basename(m.path)));
    media.push(`| \`media/${path.basename(m.path)}\` | ${m.type} | ${m.width && m.height ? `${m.width}×${m.height}` : ''} | ${m.durationS ? `${m.durationS}s` : ''} | ${m.alt || ''} |`);
  }
  const link = utmLink(config.site.url, { source: 'press', campaign: config.site.campaign, content: 'press-kit' });
  const md = [
    `# ${BRAND.name}: press kit`,
    '',
    '## Fact sheet',
    '',
    `- **Developer:** ${tbd(pk.developer)}`,
    `- **Release date:** ${tbd(pk.releaseDate)}`,
    `- **Platforms:** ${tbd(pk.platforms)}`,
    `- **Price:** ${tbd(pk.price)}`,
    `- **Website:** ${link}`,
    `- **Press contact:** ${tbd(pk.contact)}`,
    '',
    '## Description',
    '',
    BRAND.oneLiner,
    '',
    ...BRAND.gameFacts.map((f) => `- ${f}`),
    '',
    '## Moments from the simulation',
    '',
    ...stories.slice(0, 8).map((s) => `- **${s.title}**${s.summary ? `: ${s.summary}` : ''}`),
    '',
    '## Media',
    '',
    'Free to use in coverage of the game.',
    '',
    '| File | Type | Size | Length | Description |',
    '| --- | --- | --- | --- | --- |',
    ...media,
    '',
  ].join('\n');
  fs.writeFileSync(path.join(dir, 'README.md'), md);
  return dir;
}

/**
 * Personalised outreach drafts for a human to review and send one by one.
 * creators.json: [{ name, channel, url?, email?, why, recentWork? }]
 * `why` (why this creator, specifically) is required: no generic blasts.
 */
export function writeOutreach({ config, creators, stories, outDir, max = 15 }) {
  const dir = path.join(outDir, 'outreach');
  fs.mkdirSync(dir, { recursive: true });
  const written = [];
  const refused = [];
  for (const c of creators) {
    if (!c?.name || !c.why || String(c.why).trim().length < 20) {
      refused.push(`${c?.name || '(no name)'}: needs a specific "why" (20+ characters) about this creator`);
      continue;
    }
    if (written.length >= max) {
      refused.push(`${c.name}: over the per-run limit of ${max}; send these in small batches`);
      continue;
    }
    const link = utmLink(config.site.url, { source: 'outreach', campaign: config.site.campaign, content: slug(c.name) });
    const story = stories[0];
    const body = [
      `To: ${c.email || '(find their business contact)'}`,
      `Subject: ${BRAND.name}: an evolution sim you might like for ${c.channel || 'your channel'}`,
      '',
      `Hi ${c.name},`,
      '',
      `${c.why.trim()}${c.recentWork ? ` (I especially liked ${c.recentWork}.)` : ''}`,
      '',
      `I'm making ${BRAND.name}. ${BRAND.oneLiner}`,
      '',
      story ? `A recent moment from the simulation: ${story.title}${story.summary ? ` ${story.summary}` : ''}` : '',
      '',
      `Press kit and footage: ${link}`,
      '',
      "If it's a fit I'd be glad to set you up with access. If not, no reply needed, and I won't follow up more than once.",
      '',
      'Thanks,',
      (config.pressKit && config.pressKit.developer) || '[your name]',
      '',
      '<!-- Review and personalise before sending. Send from your own mail client, one at a time. -->',
    ].join('\n');
    const f = path.join(dir, `${slug(c.name)}.md`);
    fs.writeFileSync(f, body + '\n');
    written.push(f);
  }
  return { dir, written, refused };
}
