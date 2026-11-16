// Reference creatures for the art library: one design per body plan the
// style must handle. The harness (`?builder=gallery`) and the e2e gallery
// screenshots draw these; the tests check every one renders. When changing a
// part, look at all of them (STYLE.md, "Checking your work").

import type { DesignSpec } from '../protocol';
import type { Morphology } from '../morphology';

export interface GalleryCreature {
  id: string;
  label: string;
  design: DesignSpec;
  /** The body plan (as the body builder would set it). */
  body: Partial<Morphology>;
}

const design = (name: string, traits: Record<string, number>, body: Partial<Morphology>, over: Partial<DesignSpec> = {}): Omit<GalleryCreature, 'id' | 'label'> => ({
  design: { name, thermo: 'warm', birth: 'live', litter_size: 2, founders: 10, traits: { body_size: 0.4, ...traits }, ...over },
  body,
});

export const GALLERY: readonly GalleryCreature[] = [
  {
    id: 'grazer', label: 'Grazer',
    ...design('Grazer', { body_size: 0.65, hooves: 0.7, large_ears: 0.6, large_eyes: 0.5, leg_length: 0.5, fur: 0.3, tail: 0.25 },
      { eye_count: 2, eye_azimuth: 100, eye_size: 0.5, ear_count: 2, ear_shape: 'long', ear_size: 0.6, leg_count: 4, leg_length: 0.5, snout_length: 0.6, fang_size: 0, tail_present: true, tail_shape: 'thin', tail_length: 0.25 }),
  },
  {
    id: 'stalker', label: 'Stalker',
    ...design('Stalker', { body_size: 0.5, carnivory: 0.85, claws: 0.6, muscle: 0.6, fur: 0.6, large_eyes: 0.6, fangs: 0.5, tail: 0.6, camouflage: 0.55 },
      { eye_count: 2, eye_azimuth: 25, eye_size: 0.6, ear_count: 2, ear_shape: 'pointed', ear_size: 0.4, leg_count: 4, leg_length: 0.5, snout_length: 0.3, fang_size: 0.5, tail_present: true, tail_shape: 'thin', tail_length: 0.6 }),
  },
  {
    id: 'watcher', label: 'Three-eyed watcher',
    ...design('Watcher', { body_size: 0.3, large_eyes: 0.5, large_ears: 0.4, carnivory: 0.4, tail: 0.4 },
      { eye_count: 3, eye_azimuth: 130, eye_size: 0.5, ear_count: 2, ear_shape: 'fan', ear_size: 0.4, tail_present: true, tail_shape: 'thin', tail_length: 0.4 }),
  },
  {
    id: 'strider', label: 'Strider',
    ...design('Strider', { body_size: 0.55, leg_length: 0.9, hooves: 0.4, fur: 0.2, tail: 0.4 },
      { leg_count: 4, leg_length: 0.9, eye_count: 2, eye_azimuth: 80, ear_shape: 'pointed', ear_size: 0.3, tail_present: true, tail_shape: 'thin', tail_length: 0.4, snout_length: 0.55 }),
  },
  {
    id: 'climber', label: 'Six-legged climber',
    ...design('Climber', { body_size: 0.25, climbing: 0.8, claws: 0.5, tail: 0.7, fur: 0.55, large_eyes: 0.4 },
      { leg_count: 6, leg_length: 0.3, tail_present: true, tail_shape: 'prehensile', tail_length: 0.7, ear_shape: 'round', ear_size: 0.35, eye_size: 0.4 }),
  },
  {
    id: 'burrower', label: 'Burrower',
    ...design('Burrower', { body_size: 0.2, burrowing: 0.8, fat_stores: 0.4, keen_nose: 0.7 },
      { leg_count: 0, snout_length: 0.1, ear_count: 0, eye_count: 2, eye_azimuth: 60, eye_size: 0, tail_present: false }),
  },
  {
    id: 'swimmer', label: 'Swimmer',
    ...design('Swimmer', { body_size: 0.45, swimming: 0.8, foot_width: 0.7, fat_stores: 0.5, tail: 0.7, scales: 0.3 },
      { tail_present: true, tail_shape: 'fin', tail_length: 0.7, leg_count: 4, leg_length: 0.15, ear_count: 0, snout_length: 0.55 }),
  },
  {
    id: 'fox', label: 'Fox-like carnivore',
    ...design('Fox', { body_size: 0.3, carnivory: 0.75, fur: 0.85, keen_nose: 0.6, large_ears: 0.7, claws: 0.4, tail: 0.75, coloration: 0.4, fangs: 0.25 },
      { eye_count: 2, eye_azimuth: 40, eye_size: 0.35, ear_count: 2, ear_shape: 'pointed', ear_size: 0.7, leg_count: 4, leg_length: 0.45, snout_length: 0.75, fang_size: 0.25, tail_present: true, tail_shape: 'bushy', tail_length: 0.75 }),
  },
  {
    id: 'armored', label: 'Armored',
    ...design('Armored', { body_size: 0.75, armor_plates: 0.85, muscle: 0.6, hooves: 0.2, tail: 0.6, fangs: 0.5 },
      { leg_count: 4, leg_length: 0.2, ear_count: 2, ear_shape: 'round', ear_size: 0.1, eye_size: 0.1, snout_length: 0.4, tail_present: true, tail_shape: 'club', tail_length: 0.6, fang_size: 0.5 }),
  },
  {
    id: 'feathered', label: 'Feathered',
    ...design('Feathered', { body_size: 0.15, fur: 0.8, wings: 0.8, carnivory: 0.45, coloration: 0.7, claws: 0.5, large_eyes: 0.6, tail: 0.5 },
      { leg_count: 2, leg_length: 0.55, eye_size: 0.6, eye_azimuth: 70, ear_count: 0, snout_length: 0.35, tail_present: true, tail_shape: 'bushy', tail_length: 0.5 }),
  },
  {
    id: 'scaled', label: 'Cold-blooded scaled',
    ...design('Scaled', { body_size: 0.55, scales: 0.85, carnivory: 0.6, claws: 0.5, tail: 0.85, fangs: 0.2, large_eyes: 0.3 },
      { leg_count: 4, leg_length: 0.35, ear_count: 2, ear_size: 0.2, snout_length: 0.7, eye_azimuth: 75, tail_present: true, tail_shape: 'thin', tail_length: 0.85, fang_size: 0.2 }, { thermo: 'cold', birth: 'abandoned_eggs' }),
  },
  {
    id: 'glider', label: 'Membrane-winged, eight legs',
    design: { name: 'Glider', thermo: 'cold', birth: 'abandoned_eggs', litter_size: 2, founders: 10, traits: { body_size: 0.35, wings: 0.75, scales: 0.6, coloration: 0.65, carnivory: 0.7, fangs: 0.6, tail: 0.6, claws: 0.6 } },
    body: { leg_count: 8, leg_length: 0.3, ear_count: 0, eye_count: 4, eye_azimuth: 50, snout_length: 0.6, fang_size: 0.6, tail_present: true, tail_shape: 'fin', tail_length: 0.6 },
  },
  {
    id: 'default', label: 'Default founder',
    design: { name: 'Default', thermo: 'warm', birth: 'live', litter_size: 2, founders: 10, traits: { body_size: 0.3 } },
    body: {},
  },
  {
    id: 'toxic', label: 'Toxic, warning-coloured',
    ...design('Toxic', { body_size: 0.15, coloration: 0.9, toxins: 0.85, large_eyes: 0.7, tail: 0.35, fat_stores: 0.3 },
      { leg_count: 4, leg_length: 0.3, ear_count: 0, eye_size: 0.7, eye_azimuth: 70, snout_length: 0.2, tail_present: true, tail_shape: 'thin', tail_length: 0.35 }, { thermo: 'cold' }),
  },
];
