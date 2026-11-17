// Curated hashtag suggestions. Few, relevant tags beat many generic ones;
// platforms limit or down-rank tag stuffing. Drafts use at most
// PLATFORMS[p].maxHashtags of these, core tag first.
export const TAGS_BY_KIND = {
  default: ['gamedev', 'indiegame'],
  catastrophe: ['simulation', 'gamedev'],
  extinction: ['evolution', 'simulation'],
  species: ['evolution', 'alife'],
  speciation: ['evolution', 'alife'],
  predator: ['ecology', 'simulation'],
  migration: ['ecology', 'simulation'],
  era: ['indiegame', 'mmo'],
  lineage: ['evolution', 'genetics'],
  timelapse: ['simulation', 'gamedev'],
  devlog: ['gamedev', 'devlog'],
  // Kinds from `evo-sim film` (predator_boom matches predator above).
  boom: ['ecology', 'simulation'],
  comeback: ['evolution', 'ecology'],
  trait: ['evolution', 'genetics'],
};

const PLATFORM_EXTRA = {
  tiktok: ['indiegame', 'simulationgame'],
  instagram: ['indiegame', 'gamedev'],
  youtube: ['Shorts'],
  mastodon: ['ArtificialLife'],
};

export function kindKey(kind) {
  const k = String(kind).toLowerCase();
  for (const key of Object.keys(TAGS_BY_KIND)) if (k.includes(key)) return key;
  return 'default';
}

export function suggestHashtags(kind, platform, core = [], max = 3) {
  const list = [...core, ...TAGS_BY_KIND[kindKey(kind)], ...(PLATFORM_EXTRA[platform] || []), ...TAGS_BY_KIND.default];
  const seen = new Set();
  const out = [];
  for (const t of list) {
    const k = t.toLowerCase();
    if (seen.has(k)) continue;
    seen.add(k);
    out.push(t);
  }
  return out.slice(0, max);
}
