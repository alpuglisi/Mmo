// Brand voice, encoded so templates and the Claude prompt share one source.
// The human-readable version is STYLE.md; keep the two in step.

export const BRAND = {
  name: 'Evolution MMO',
  oneLiner:
    'A persistent multiplayer world where you design a founding organism and steer its lineage through breeding, not direct editing.',
  // Facts about the game that copy may state without a story backing them.
  // Keep this list short and true; everything else must come from story data.
  gameFacts: [
    'Each player designs a founding organism on a 100-point budget.',
    'Players steer their lineage through breeding rather than direct editing.',
    'The world is persistent and shared: player lineages live alongside a wild ecosystem.',
    'The score is genetic legacy: your alleles’ share of the breeding adults alive in the world.',
    'Seasons, weather, droughts, floods and catastrophes shape who survives.',
    'It runs in a web browser; mobile apps are in development.',
  ],
  // Numbers that may appear in copy without a story backing them.
  allowedNumbers: ['100'],
  coreHashtags: ['EvolutionMMO'],
  voice: [
    'Curious and observational, like a field naturalist narrating what the simulation did.',
    'Concrete: name the species, the season, the number. Let the event be the hook.',
    'Plain words, short sentences. No hype words, no superlatives about the game itself.',
    'Honest about what is simulated: these are emergent events, not scripted ones.',
    'Warm, never snarky about players. Players are named only if the story data names them and they opted in.',
    'At most one emoji per post, and none in titles.',
  ],
  banned: [
    'revolutionary',
    'groundbreaking',
    'game-changer',
    'game changer',
    'best game',
    'ultimate',
    'mind-blowing',
    'insane',
    'epic',
    'must-play',
    'viral',
    'you won’t believe',
    "you won't believe",
    'smash that',
    'like and subscribe',
    'guaranteed',
  ],
  callToAction: {
    default: 'Watch it unfold yourself:',
    wishlist: 'Follow along:',
  },
};

export function bannedWordsIn(text) {
  const lower = text.toLowerCase();
  return BRAND.banned.filter((w) => lower.includes(w));
}
