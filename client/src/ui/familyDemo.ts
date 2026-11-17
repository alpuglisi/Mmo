// A made-up family tree for the dev harness (`?builder=family`), the unit
// tests and the screenshots: a grazing lineage that grew longer legs and
// antlers and took on stripes, with three species splitting off (one of them
// extinct, one a split of a split). The shape is the server's `FamilyTree`.

import type { BodyPlanMode, FamilySample, FamilySpecies, FamilyTree } from '../protocol';

const plan = (over: Partial<BodyPlanMode> = {}): BodyPlanMode => ({
  eye_count: 2, eye_azimuth: 90, ear_count: 2, ear_shape: 'long', leg_count: 4, tail_shape: 'thin', snout_length: 0.6,
  horn_size: 0, horn_form: 'curved', wing_aspect: 0.5, pattern: 'plain', ...over,
});

const genes = (over: Record<string, number> = {}): Record<string, number> => ({
  endothermy: 0.9, birth_mode: 0.85, body_size: 0.5, leg_length: 0.35, hooves: 0.5, large_ears: 0.4, large_eyes: 0.4,
  fur: 0.55, tail: 0.25, carnivory: 0.1, camouflage: 0.2, coloration: 0.1, ...over,
});

const day = (d: number, alive: number, g: Record<string, number>, m: Partial<BodyPlanMode>): FamilySample => ({ day: d, alive, genes: genes(g), morph: plan(m) });

export const DEMO_TREE: FamilyTree = {
  player: 1,
  lineage: 0,
  name: 'Dune Grazers',
  day: 96,
  alive: 46,
  timeline: [
    day(0, 20, {}, {}),
    day(8, 31, { large_ears: 0.42 }, {}),
    day(16, 38, { large_ears: 0.55, leg_length: 0.42 }, {}),
    day(32, 52, { large_ears: 0.6, leg_length: 0.5, camouflage: 0.32 }, { pattern: 'stripes' }),
    day(48, 61, { large_ears: 0.62, leg_length: 0.55, camouflage: 0.35 }, { pattern: 'stripes' }),
    day(64, 58, { large_ears: 0.63, leg_length: 0.62, body_size: 0.62, camouflage: 0.36 }, { pattern: 'stripes', horn_size: 0.35, horn_form: 'branched' }),
    day(96, 46, { large_ears: 0.64, leg_length: 0.64, body_size: 0.66, camouflage: 0.38, fur: 0.4 }, { pattern: 'stripes', horn_size: 0.6, horn_form: 'branched' }),
  ],
  species: [
    {
      id: 3, name: 'Dune grazers', origin: 'founding', founded_day: 0, adults: 40, mine: 40,
      genes: genes({ large_ears: 0.64, leg_length: 0.64 }), morph: plan({ pattern: 'stripes', horn_size: 0.6, horn_form: 'branched' }),
    },
    {
      id: 7, name: 'Marsh waders', origin: 'split', parent: 3, founded_day: 20, extinct_day: 55, adults: 0, mine: 0,
      genes: genes({ large_ears: 0.5, leg_length: 0.7, foot_width: 0.55, swimming: 0.4, fur: 0.3 }), morph: plan({ tail_shape: 'fin' }),
    },
    {
      id: 9, name: 'Highland stags', origin: 'split', parent: 3, founded_day: 70, adults: 18, mine: 12,
      genes: genes({ large_ears: 0.6, leg_length: 0.6, body_size: 0.75, fur: 0.75, muscle: 0.4 }), morph: plan({ pattern: 'mottled', horn_size: 0.85, horn_form: 'branched' }),
    },
    {
      id: 12, name: 'Ridge runners', origin: 'split', parent: 9, founded_day: 88, adults: 6, mine: 6,
      genes: genes({ large_ears: 0.6, leg_length: 0.8, body_size: 0.6, fur: 0.7 }), morph: plan({ pattern: 'mottled', horn_size: 0.4, horn_form: 'straight' }),
    },
  ] satisfies FamilySpecies[],
};
