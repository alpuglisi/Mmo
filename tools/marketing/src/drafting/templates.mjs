import { BRAND } from '../style.mjs';

// Deterministic copy used when no ANTHROPIC_API_KEY is configured, or when
// Claude's copy fails validation. Everything is drawn from the story itself.

function sentence(s) {
  s = String(s || '').trim();
  if (!s) return '';
  return /[.!?…]$/.test(s) ? s : s + '.';
}

function factLines(story, max = 6) {
  return Object.entries(story.facts)
    .filter(([, v]) => v !== null && typeof v !== 'object')
    .slice(0, max)
    .map(([k, v]) => `- **${k.replace(/_/g, ' ')}:** ${v}`);
}

export function templateCopy(platform, story, media) {
  const hook = sentence(story.title);
  const detail = sentence(story.summary);
  const emergent = 'Nothing here was scripted: it emerged from the simulation.';
  const facts = factLines(story);
  const hasVideo = media.some((m) => m.type === 'video');
  switch (platform) {
    case 'x':
    case 'bluesky':
    case 'mastodon':
      return { text: [hook, detail].filter(Boolean).join(' ') };
    case 'tiktok':
    case 'instagram':
      return { caption: [hook, detail, emergent, 'Link in bio.'].filter(Boolean).join(' ') };
    case 'youtube':
      return {
        title: story.title.replace(/[.!]$/, ''),
        description: [hook, detail, emergent, '', BRAND.oneLiner].join('\n'),
      };
    case 'reddit':
      return {
        title: story.title.replace(/[.!]$/, ''),
        body: [
          detail || hook,
          '',
          ...(facts.length ? ['What the simulation recorded:', '', ...facts, ''] : []),
          `For context: ${BRAND.oneLiner} ${emergent}`,
          '',
          'I am the developer; happy to answer questions about how this works.',
        ].join('\n'),
      };
    case 'discord':
      return { content: [`**${story.title.replace(/[.!]$/, '')}**`, detail, hasVideo ? 'Clip attached.' : ''].filter(Boolean).join('\n') };
    case 'devlog': {
      const embeds = media.map((m) => (m.type === 'video' ? `<video src="${m.path}" controls></video>` : `![${m.alt || story.title}](${m.path})`));
      return {
        title: story.title.replace(/[.!]$/, ''),
        body: [
          `# ${story.title.replace(/[.!]$/, '')}`,
          '',
          detail || hook,
          '',
          ...embeds.flatMap((e) => [e, '']),
          ...(facts.length ? ['## By the numbers', '', ...facts, ''] : []),
          '## About the game',
          '',
          BRAND.oneLiner,
          '',
          emergent,
        ].join('\n'),
      };
    }
    case 'email':
      return {
        subject: story.title.replace(/[.!]$/, ''),
        body: [
          'Hello,',
          '',
          detail || hook,
          '',
          ...(facts.length ? [...facts.map((l) => l.replace(/\*\*/g, '')), ''] : []),
          emergent,
          '',
          'You are getting this because you subscribed to Evolution MMO updates. Unsubscribe any time with the link below.',
        ].join('\n'),
      };
    default:
      throw new Error(`no template for ${platform}`);
  }
}
