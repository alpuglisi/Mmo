import { describe, expect, it } from 'vitest';
import {
  clampMorphology,
  DEFAULT_MORPHOLOGY,
  defaultMorphology,
  eyePlacements,
  hasFangs,
  morphologyFromTraits,
  nearestEarCount,
  nearestLegCount,
  snoutOcclusion,
  sameMorphology,
  traitsFromMorphology,
} from './morphology';

describe('morphology', () => {
  it('defaults follow the contract', () => {
    const m = defaultMorphology();
    expect(m).toEqual({
      eye_count: 2, eye_azimuth: 60, eye_size: 0, ear_count: 2, ear_shape: 'round', ear_size: 0, leg_count: 4, leg_length: 0,
      tail_present: false, tail_shape: 'thin', tail_length: 0, snout_length: 0.5, fang_size: 0,
    });
    expect(Object.isFrozen(DEFAULT_MORPHOLOGY)).toBe(true);
    m.eye_count = 5;
    expect(DEFAULT_MORPHOLOGY.eye_count).toBe(2);
  });

  it('clamps numbers, rounds counts and replaces bad values', () => {
    const m = clampMorphology({
      eye_count: 9.4, eye_azimuth: -20, eye_size: 1.7, ear_count: 2.6, ear_shape: 'floppy', ear_size: -1,
      leg_count: 5, leg_length: NaN, tail_present: 'yes', tail_shape: 'fin', tail_length: 2, snout_length: '0.3', fang_size: 0.05,
    });
    expect(m).toEqual({
      eye_count: 6, eye_azimuth: 0, eye_size: 1, ear_count: 2, ear_shape: 'round', ear_size: 0,
      leg_count: 6, leg_length: 0, tail_present: false, tail_shape: 'fin', tail_length: 1, snout_length: 0.5, fang_size: 0.05,
    });
    expect(clampMorphology({ eye_count: 0, ear_count: 7, eye_azimuth: 999 })).toMatchObject({ eye_count: 1, ear_count: 4, eye_azimuth: 180 });
    expect(clampMorphology({ ear_count: 1 }).ear_count).toBe(2);
  });

  it('takes missing fields from the base, and garbage gives the defaults', () => {
    expect(clampMorphology(null)).toEqual(DEFAULT_MORPHOLOGY);
    expect(clampMorphology('nope')).toEqual(DEFAULT_MORPHOLOGY);
    const base = { ...defaultMorphology(), eye_count: 3 };
    expect(clampMorphology({ ear_count: 0 }, base)).toMatchObject({ eye_count: 3, ear_count: 0 });
  });

  it('leg counts snap to 0, 2, 4, 6, 8', () => {
    expect([-3, 0, 1, 2.9, 3, 7, 8, 20].map(nearestLegCount)).toEqual([0, 0, 2, 2, 4, 8, 8, 8]);
  });

  it('ear counts snap to 0, 2, 4', () => {
    expect([-1, 0, 0.9, 1, 2, 3, 3.4, 9].map(nearestEarCount)).toEqual([0, 0, 0, 2, 2, 4, 4, 4]);
  });

  it('snout occlusion is 40° per unit above 0.5', () => {
    expect([0, 0.5, 0.75, 1, 3].map(snoutOcclusion)).toEqual([0, 0, 10, 20, 20]);
  });

  it('places eyes in mirrored pairs, the odd one on the midline', () => {
    expect(eyePlacements({ eye_count: 1, eye_azimuth: 60 })).toEqual([{ azimuth: 0, pair: null, side: 'mid' }]);
    expect(eyePlacements({ eye_count: 2, eye_azimuth: 60 }).map((e) => e.azimuth)).toEqual([60, -60]);
    expect(eyePlacements({ eye_count: 3, eye_azimuth: 60 }).map((e) => e.azimuth)).toEqual([0, 60, -60]);
    expect(eyePlacements({ eye_count: 4, eye_azimuth: 60 }).map((e) => e.azimuth)).toEqual([60, -60, 110, -110]);
    expect(eyePlacements({ eye_count: 5, eye_azimuth: 30 }).map((e) => e.azimuth)).toEqual([0, 30, -30, 80, -80]);
    expect(eyePlacements({ eye_count: 6, eye_azimuth: 100 }).map((e) => e.azimuth)).toEqual([100, -100, 150, -150, 170, -170]);
    expect(eyePlacements({ eye_count: 2, eye_azimuth: 180 }).map((e) => e.azimuth)).toEqual([170, -170]);
  });

  it('fangs count from 0.1', () => {
    expect(hasFangs({ ...defaultMorphology(), fang_size: 0.09 })).toBe(false);
    expect(hasFangs({ ...defaultMorphology(), fang_size: 0.1 })).toBe(true);
  });

  it('maps to and from the existing genes', () => {
    const m = morphologyFromTraits({ large_eyes: 0.8, large_ears: 0.2, leg_length: 0.7, tail: 0.02, fangs: 0.4, muscle: 1 });
    expect(m).toMatchObject({ eye_size: 0.8, ear_size: 0.2, leg_length: 0.7, tail_present: false, tail_length: 0.02, fang_size: 0.4 });
    expect(traitsFromMorphology(m)).toEqual({ large_eyes: 0.8, large_ears: 0.2, leg_length: 0.7, tail: 0, fangs: 0.4 });
    expect(traitsFromMorphology({ ...m, tail_present: true, tail_length: 0.01 }).tail).toBe(0.05);
  });

  it('sameMorphology compares every field', () => {
    expect(sameMorphology(defaultMorphology(), defaultMorphology())).toBe(true);
    expect(sameMorphology(defaultMorphology(), { ...defaultMorphology(), tail_shape: 'club' })).toBe(false);
  });
});
