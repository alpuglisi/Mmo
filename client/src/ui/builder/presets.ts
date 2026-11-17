// Starting points for the body builder (docs/morphology.md, section 6, plus
// the three-eyed watcher). A preset sets only the fields it names; the rest
// keep their current values.

import type { Morphology } from '../../morphology';

export interface BuilderPreset {
  id: string;
  label: string;
  /** One line on what the shape is good for. */
  hint: string;
  value: Partial<Morphology>;
  /**
   * Genes the design editor sets with it (the builder itself edits only the
   * body plan): a stalker eats meat, a grazer plants.
   */
  traits?: Record<string, number>;
}

export const DEFAULT_PRESETS: readonly BuilderPreset[] = [
  {
    id: 'grazer',
    label: 'Grazer',
    hint: 'Eyes on the sides: sees nearly all round and nothing sneaks up behind, but blind straight ahead.',
    value: { eye_count: 2, eye_azimuth: 100, eye_size: 0.5, ear_count: 2, ear_shape: 'long', ear_size: 0.6, leg_count: 4, leg_length: 0.5, snout_length: 0.6, fang_size: 0 },
    traits: { carnivory: 0.05 },
  },
  {
    id: 'stalker',
    label: 'Stalker',
    hint: 'Forward eyes and a short snout: a wide depth zone for judging the pounce, blind behind.',
    value: { eye_count: 2, eye_azimuth: 25, eye_size: 0.6, ear_count: 2, ear_shape: 'pointed', ear_size: 0.4, leg_count: 4, leg_length: 0.5, snout_length: 0.3, fang_size: 0.5 },
    traits: { carnivory: 0.85 },
  },
  {
    id: 'watcher',
    label: 'Three-eyed watcher',
    hint: 'A midline eye plus a pair set far back: no blind spot at all.',
    value: { eye_count: 3, eye_azimuth: 130, eye_size: 0.5, ear_count: 2, ear_shape: 'fan', ear_size: 0.4 },
  },
  {
    id: 'strider',
    label: 'Strider',
    hint: 'Long legs: fast over open ground.',
    value: { leg_count: 4, leg_length: 0.9, eye_count: 2, eye_azimuth: 80, tail_present: true, tail_shape: 'thin', tail_length: 0.4 },
  },
  {
    id: 'climber',
    label: 'Six-legged climber',
    hint: 'Six short legs and a grasping tail: steady on branches and slopes.',
    value: { leg_count: 6, leg_length: 0.3, tail_present: true, tail_shape: 'prehensile', tail_length: 0.7 },
  },
  {
    id: 'burrower',
    label: 'Burrower',
    hint: 'No legs and a flat face: slips through tunnels.',
    value: { leg_count: 0, snout_length: 0.1, ear_count: 0, eye_count: 2, eye_azimuth: 60, eye_size: 0, tail_present: false },
  },
  {
    id: 'swimmer',
    label: 'Swimmer',
    hint: 'A fin tail and short legs: at home in the water.',
    value: { tail_present: true, tail_shape: 'fin', tail_length: 0.7, leg_count: 4, leg_length: 0.15, ear_count: 0 },
  },
];
