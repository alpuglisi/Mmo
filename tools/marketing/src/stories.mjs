import fs from 'node:fs';
import path from 'node:path';

// Input contracts (see PUBLISHING.md "Inputs"). Both loaders are lenient
// about wrapper shape and field names so they keep working while the
// generator branches settle their formats.
//
// stories.json:  [Story] | { stories: [Story] } | { events: [Story] }
//   Story = { id, kind, title, summary?, day?, year?, world?,
//             facts?: { name: value }, media?: [mediaId | path], players?: [{name, optedIn}] }
//
// manifest.json: [Item] | { items | clips | files | outputs: [Item] }
//   Item = { id?, path | file, type | kind ('video'|'image'|'card'|'chart'),
//            duration_s?, width?, height?, story_id?, alt?, title? }

function arr(json, keys) {
  if (Array.isArray(json)) return json;
  for (const k of keys) if (Array.isArray(json?.[k])) return json[k];
  return [];
}

export function normalizeStory(raw, i = 0) {
  const facts = { ...(raw.facts || raw.data || raw.stats || {}) };
  for (const k of ['day', 'year', 'season', 'species', 'lineage', 'count', 'deaths', 'share', 'start_day', 'end_day', 'when']) {
    if (raw[k] !== undefined && facts[k] === undefined) facts[k] = raw[k];
  }
  const story = {
    id: String(raw.id ?? raw.story_id ?? `story-${i + 1}`),
    kind: String(raw.kind ?? raw.type ?? raw.category ?? 'event'),
    title: String(raw.title ?? raw.headline ?? raw.hook ?? raw.summary ?? '').trim(),
    summary: String(raw.summary ?? raw.description ?? raw.text ?? [raw.hook, raw.when].filter(Boolean).join(' (') + (raw.hook && raw.when ? ')' : '')).trim(),
    facts,
    media: (raw.media ?? raw.media_ids ?? raw.clips ?? []).map(String),
    // Players are only named in copy when they opted in to being featured.
    players: (raw.players ?? []).filter((p) => p && p.optedIn === true).map((p) => String(p.name)),
    world: raw.world ?? raw.seed ?? null,
    score: Number(raw.score ?? raw.interest ?? 0),
  };
  if (!story.title) throw new Error(`story ${story.id}: no title`);
  return story;
}

export function normalizeMediaItem(raw, i = 0, baseDir = '.') {
  const p = raw.path ?? raw.file ?? raw.output ?? raw.src;
  if (!p) throw new Error(`manifest item ${i}: no path`);
  let type = String(raw.type ?? raw.kind ?? '').toLowerCase();
  if (!type) type = /\.(mp4|webm|mov)$/i.test(p) ? 'video' : 'image';
  if (['card', 'share_card', 'share-card', 'chart', 'png', 'screenshot'].includes(type)) type = 'image';
  if (['clip', 'film', 'timelapse', 'time-lapse', 'mp4'].includes(type)) type = 'video';
  const width = Number(raw.width ?? raw.w ?? 0) || null;
  const height = Number(raw.height ?? raw.h ?? 0) || null;
  const abs = path.isAbsolute(p) ? p : path.resolve(baseDir, p);
  // For embedding in copy (devlog bodies): relative to the manifest, never
  // the machine's absolute path; just the file name if it lies outside.
  let rel = path.relative(path.resolve(baseDir), abs);
  if (!rel || rel.startsWith('..') || path.isAbsolute(rel)) rel = path.basename(abs);
  return {
    id: String(raw.id ?? raw.name ?? path.basename(p)),
    path: abs,
    relPath: rel.split(path.sep).join('/'),
    type,
    durationS: Number(raw.duration_s ?? raw.duration ?? raw.seconds ?? 0) || null,
    width,
    height,
    vertical: width && height ? height > width : null,
    storyId: raw.story_id ?? raw.storyId ?? null,
    alt: raw.alt ?? raw.alt_text ?? raw.caption ?? raw.title ?? null,
  };
}

export function loadStories(file) {
  const json = JSON.parse(fs.readFileSync(file, 'utf8'));
  return arr(json, ['stories', 'events', 'items']).map(normalizeStory);
}

export function loadManifest(file) {
  if (!file) return [];
  const json = JSON.parse(fs.readFileSync(file, 'utf8'));
  const base = path.dirname(path.resolve(file));
  return arr(json, ['items', 'clips', 'files', 'outputs', 'media']).map((r, i) => normalizeMediaItem(r, i, base));
}

/** Media for a story: explicit story.media ids/paths first, then items whose
 * storyId matches. */
export function mediaForStory(story, manifest) {
  const byId = new Map(manifest.map((m) => [m.id, m]));
  const out = [];
  for (const ref of story.media) {
    const m = byId.get(ref) || manifest.find((x) => x.path.endsWith(ref));
    if (m && !out.includes(m)) out.push(m);
  }
  for (const m of manifest) if (m.storyId === story.id && !out.includes(m)) out.push(m);
  return out;
}

/** Every string a piece of copy may cite as a fact. */
export function factCorpus(story) {
  const parts = [story.title, story.summary, ...story.players];
  for (const [k, v] of Object.entries(story.facts)) parts.push(`${k} ${typeof v === 'object' ? JSON.stringify(v) : v}`);
  return parts.join('\n');
}
