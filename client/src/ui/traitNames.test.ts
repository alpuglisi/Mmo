import { describe, expect, it } from 'vitest';
import { TRAIT_TEXT } from './traitText';
import { TRAIT_NAMES, traitEnds, traitName, traitNameLower } from './traitNames';

describe('trait names', () => {
  it('names every slider the editor shows, plus the three design-choice genes', () => {
    for (const g of Object.keys(TRAIT_TEXT)) expect(TRAIT_NAMES[g]?.name, g).toBeTruthy();
    for (const g of ['endothermy', 'birth_mode', 'litter_size']) expect(TRAIT_NAMES[g]?.name, g).toBeTruthy();
    expect(Object.keys(TRAIT_NAMES).length).toBe(Object.keys(TRAIT_TEXT).length + 3);
  });

  it('names the axis, not one end of it', () => {
    expect(traitName('large_ears')).toBe('Ear size');
    expect(traitName('large_eyes')).toBe('Eye size');
    expect(traitName('keen_nose')).toBe('Sense of smell');
    expect(traitName('armor_plates')).toBe('Armor');
    expect(traitName('vocal_ability')).toBe('Voice');
    for (const [g, { name }] of Object.entries(TRAIT_NAMES)) {
      expect(name, g).not.toMatch(/^(large|keen|small|big|huge|high|low|strong|weak)\b/i);
      // Sentence case, shown as written (the CSS no longer capitalises).
      expect(name[0], g).toBe(name[0].toUpperCase());
    }
  });

  it('gives two-ended sliders words for both ends', () => {
    // A slider whose low setting is a real state, not "none" or "off".
    for (const g of ['body_size', 'large_ears', 'large_eyes', 'keen_nose', 'carnivory', 'coloration', 'boldness', 'obedience']) {
      const e = traitEnds(g);
      expect(e, g).not.toBeNull();
      expect(e![0], g).not.toBe(e![1]);
    }
    for (const g of ['claws', 'wings', 'toxins', 'camouflage', 'hibernation']) expect(traitEnds(g), g).toBeNull();
  });

  it('falls back to the key’s words, and lowers for sentences', () => {
    expect(traitName('some_new_gene')).toBe('Some new gene');
    expect(traitNameLower('large_ears')).toBe('ear size');
    expect(traitNameLower('group_tolerance')).toBe('sociability');
  });
});
