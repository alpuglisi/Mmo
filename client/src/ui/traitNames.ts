// The one place a gene's display name lives. Every screen that names a trait
// (design sliders, budget, genetics dashboard, market chips, mating
// summaries, captions that mention another trait) goes through `traitName`.
//
// The wire keys (`large_ears`, `keen_nose`, …) come from the sim's gene
// labels and stay as they are: they are storage and protocol names. A key
// such as `large_ears` names one end of its slider (0 gives small ears), so
// the display name names the axis instead ("Ear size"). Sliders whose 0 means
// "none" (claws, wings, toxins, hibernation, …) keep a noun. Sliders that run
// between two real states also carry `ends`: the words for 0 and 1, shown
// under the slider.

export interface TraitName {
  /** Sentence-case display name of the axis ("Ear size"). */
  name: string;
  /** Words for the two ends, [at 0, at 1], when both ends are real states (not "none"). */
  ends?: [string, string];
}

export const TRAIT_NAMES: Record<string, TraitName> = {
  // Design choices, not sliders (they show in the genetics dashboard).
  endothermy: { name: 'Warm-bloodedness', ends: ['cold-blooded', 'warm-blooded'] },
  birth_mode: { name: 'Birth mode', ends: ['abandoned eggs', 'live birth'] },
  litter_size: { name: 'Offspring per birth', ends: ['one', 'many'] },
  // Body
  body_size: { name: 'Body size', ends: ['small', 'large'] },
  muscle: { name: 'Muscle mass', ends: ['lean', 'powerful'] },
  fat_stores: { name: 'Body fat', ends: ['lean', 'fat'] },
  // Limbs
  leg_length: { name: 'Leg length', ends: ['short', 'long'] },
  claws: { name: 'Claws' },
  hooves: { name: 'Hooves', ends: ['soft feet', 'solid hooves'] },
  foot_width: { name: 'Foot width', ends: ['narrow', 'wide, webbed'] },
  tail: { name: 'Tail length' },
  // Locomotion
  swimming: { name: 'Swimming' },
  wings: { name: 'Wings' },
  climbing: { name: 'Climbing' },
  burrowing: { name: 'Burrowing' },
  // Covering
  fur: { name: 'Fur' },
  scales: { name: 'Scales' },
  armor_plates: { name: 'Armor' },
  // Head
  fangs: { name: 'Fang size' },
  large_eyes: { name: 'Eye size', ends: ['small', 'huge'] },
  large_ears: { name: 'Ear size', ends: ['small', 'huge'] },
  keen_nose: { name: 'Sense of smell', ends: ['weak', 'superb'] },
  // Other
  carnivory: { name: 'Diet', ends: ['plants', 'meat'] },
  camouflage: { name: 'Camouflage' },
  toxins: { name: 'Toxicity' },
  coloration: { name: 'Colour', ends: ['drab', 'vivid'] },
  vocal_ability: { name: 'Voice', ends: ['quiet', 'vocal'] },
  // Instinct
  boldness: { name: 'Boldness', ends: ['timid', 'bold'] },
  aggression: { name: 'Aggression', ends: ['peaceable', 'aggressive'] },
  wanderlust: { name: 'Roaming', ends: ['homebody', 'wanderer'] },
  group_tolerance: { name: 'Sociability', ends: ['solitary', 'herd'] },
  foraging_range: { name: 'Foraging range', ends: ['close', 'wide'] },
  pack_instinct: { name: 'Pack hunting', ends: ['lone', 'pack'] },
  pickiness: { name: 'Mate choosiness', ends: ['easygoing', 'picky'] },
  preferred_coloration: { name: 'Preferred mate colour', ends: ['drab', 'bright'] },
  coloration_tolerance: { name: 'Mate colour tolerance', ends: ['strict', 'any'] },
  preferred_body_size: { name: 'Preferred mate size', ends: ['small', 'large'] },
  body_size_tolerance: { name: 'Mate size tolerance', ends: ['strict', 'any'] },
  // Seasonal strategies (off below 0.5)
  hibernation: { name: 'Hibernation' },
  brumation: { name: 'Brumation' },
  migration: { name: 'Migration' },
  estivation: { name: 'Estivation' },
  food_caching: { name: 'Food caching' },
  torpor: { name: 'Torpor' },
  seasonal_coat: { name: 'Seasonal coat' },
  timed_breeding: { name: 'Timed breeding' },
  // Group
  vigilance: { name: 'Sentry instinct' },
  obedience: { name: 'Obedience', ends: ['headstrong', 'obedient'] },
};

/**
 * Body-plan parameters of a design's `morph` object (not genes): counts,
 * shapes and axes. Names match the sim's `MorphLocus::display_name` /
 * `MorphGene::display_name` (through `traitNames.fixture.json`).
 */
export const MORPH_NAMES: Record<string, TraitName> = {
  eye_count: { name: 'Eye count' },
  ear_count: { name: 'Ear count' },
  ear_shape: { name: 'Ear shape' },
  leg_count: { name: 'Leg count' },
  tail_shape: { name: 'Tail shape' },
  horn_form: { name: 'Horn shape' },
  pattern: { name: 'Coat pattern' },
  eye_azimuth: { name: 'Eye placement', ends: ['forward', 'sideways'] },
  snout_length: { name: 'Snout length', ends: ['flat face', 'long muzzle'] },
  horn_size: { name: 'Horn size' },
  wing_aspect: { name: 'Wing shape', ends: ['broad', 'narrow'] },
};

/** The display name of a gene ("Ear size"); an unknown key reads as its words, capitalised. */
export function traitName(gene: string): string {
  const n = TRAIT_NAMES[gene]?.name ?? MORPH_NAMES[gene]?.name;
  if (n) return n;
  const words = gene.replace(/_/g, ' ');
  return words.charAt(0).toUpperCase() + words.slice(1);
}

/** The display name for use inside a sentence ("ear size"). */
export function traitNameLower(gene: string): string {
  const n = traitName(gene);
  // Keep an all-capitals word (an acronym) as it is.
  return /^[A-Z]{2}/.test(n) ? n : n.charAt(0).toLowerCase() + n.slice(1);
}

/** The words for 0 and 1 on a two-ended slider, or null when 0 means "none". */
export function traitEnds(gene: string): [string, string] | null {
  return TRAIT_NAMES[gene]?.ends ?? MORPH_NAMES[gene]?.ends ?? null;
}
