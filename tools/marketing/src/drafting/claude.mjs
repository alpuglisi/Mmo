import { PLATFORMS } from '../platforms.mjs';
import { BRAND } from '../style.mjs';

// Copywriting with the Claude API (official SDK, @anthropic-ai/sdk).
// One request per story returns copy for every requested platform as JSON
// (structured outputs), so the voice stays consistent across channels.
// The caller validates the result and falls back to templates per platform.

export const FALLBACK_BETA = 'server-side-fallback-2026-07-01';

const SYSTEM = `You write social copy for ${BRAND.name}, an indie game.
${BRAND.oneLiner}

Voice:
${BRAND.voice.map((v) => `- ${v}`).join('\n')}

Never use these words or phrases: ${BRAND.banned.join(', ')}.

Facts you may state about the game in general:
${BRAND.gameFacts.map((f) => `- ${f}`).join('\n')}

Hard rules:
- Every claim about the event must come from the STORY DATA in the user message. Do not invent numbers, names, species, dates, player counts, prices, release dates or platforms. If a number is not in the data, do not write a number.
- Do not name players unless they appear in the data's "players" list.
- Do not add links or hashtags: the tool appends a tracked link and hashtags after your text. Stay within the character budget given for each field, which already leaves room for them.
- Do not ask readers to like, upvote, share, follow or subscribe.
- Reddit: write as the developer, transparent about it, in a discussion tone; the title states the event plainly with no clickbait.
- YouTube: no "<" or ">" characters.
- Devlog: Markdown, a few short paragraphs with a heading; it may explain the mechanics behind the event using only the general game facts above.
- Newsletter: plain text, friendly, short.`;

export function buildSchema(platforms) {
  const properties = {};
  for (const p of platforms) {
    const fields = Object.keys(PLATFORMS[p].fields);
    properties[p] = {
      type: 'object',
      properties: Object.fromEntries(fields.map((f) => [f, { type: 'string' }])),
      required: fields,
      additionalProperties: false,
    };
  }
  return { type: 'object', properties, required: platforms, additionalProperties: false };
}

export function buildUserMessage(story, media, budgetsByPlatform, feedback) {
  const data = {
    id: story.id,
    kind: story.kind,
    title: story.title,
    summary: story.summary,
    facts: story.facts,
    players: story.players,
    media: media.map((m) => ({ type: m.type, seconds: m.durationS, alt: m.alt })),
  };
  const lines = Object.entries(budgetsByPlatform).map(
    ([p, b]) => `- ${p} (${PLATFORMS[p].label}): ${Object.entries(b).map(([f, n]) => `${f} <= ${n} chars`).join(', ')}`,
  );
  let msg = `STORY DATA:\n${JSON.stringify(data, null, 2)}\n\nWrite copy for these platforms, staying within each budget:\n${lines.join('\n')}`;
  if (feedback) msg += `\n\nYour previous attempt failed these checks; fix them:\n${feedback}`;
  return msg;
}

export async function createClient(env = process.env) {
  if (!env.ANTHROPIC_API_KEY && !env.ANTHROPIC_AUTH_TOKEN) return null;
  try {
    const { default: Anthropic } = await import('@anthropic-ai/sdk');
    return new Anthropic();
  } catch {
    console.warn('ANTHROPIC_API_KEY is set but @anthropic-ai/sdk is not installed (npm install); using templates.');
    return null;
  }
}

/** Returns { copy: {platform: fields}, model } or throws. */
export async function claudeCopy({ client, model, effort, story, media, budgets, feedback }) {
  const platforms = Object.keys(budgets);
  const response = await client.beta.messages.create({
    model,
    max_tokens: 16000,
    betas: [FALLBACK_BETA],
    fallbacks: 'default',
    output_config: { effort, format: { type: 'json_schema', schema: buildSchema(platforms) } },
    system: SYSTEM,
    messages: [{ role: 'user', content: buildUserMessage(story, media, budgets, feedback) }],
  });
  if (response.stop_reason === 'refusal') throw new Error('Claude declined this story');
  if (response.stop_reason === 'max_tokens') throw new Error('Claude response truncated');
  const text = response.content.filter((b) => b.type === 'text').map((b) => b.text).join('');
  return { copy: JSON.parse(text), model: response.model };
}

export const _SYSTEM_FOR_TESTS = SYSTEM;
