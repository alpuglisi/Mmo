import { describe, expect, it } from 'vitest';
import type { CatastropheView, EraList, SpeciesView } from '../protocol';
import { catastropheBanner, catastropheLabel, dietLabel, myEraRanks, nicheLabel, speciesTree } from './chronicle';

function sp(id: number, name: string, extra: Partial<SpeciesView> = {}): SpeciesView {
  return {
    id, name, origin: 'initial', founded_day: 0, dominant: 0, dominant_name: 'wild', dominant_share: 1,
    adults: 100, peak_adults: 100, lineages: [], carnivory: 0.1, body_size: 0.3, ...extra,
  };
}

describe('speciesTree', () => {
  it('puts daughter species under their parents, depth first', () => {
    const list = [
      sp(0, 'Voles'),
      sp(1, 'Foxes', { carnivory: 0.8 }),
      sp(2, 'Voles (2)', { origin: 'split', parent: 0, founder: 3, founder_name: 'cat' }),
      sp(3, 'Voles (3)', { origin: 'split', parent: 2 }),
      sp(4, 'Newts', { extinct_day: 40 }),
      sp(5, 'Hybrids', { extinct_day: 50, merged_into: 0 }),
    ];
    const rows = speciesTree(list);
    expect(rows.map((r) => [r.species.name, r.depth])).toEqual([
      ['Voles', 0], ['Voles (2)', 1], ['Voles (3)', 2], ['Foxes', 0], ['Newts', 0], ['Hybrids', 0],
    ]);
    expect(rows[0].status).toBe('~100 adults');
    expect(rows[4]).toMatchObject({ alive: false, status: 'extinct day 41' });
    expect(rows[5].status).toBe('merged into Voles');
  });

  it('treats a missing parent as a root and survives bad links', () => {
    const rows = speciesTree([sp(7, 'Orphan', { parent: 99 }), sp(8, 'Loop', { parent: 8 })]);
    expect(rows.map((r) => [r.species.id, r.depth])).toEqual([[7, 0], [8, 0]]);
  });
});

describe('catastrophes', () => {
  const winter: CatastropheView = {
    id: 0, kind: 'severe_winter', severity: 1, start_day: 210, end_day: 240, active: true,
    population_before: 4000, players_before: [1, 2], survivors: [],
  };

  it('banners only the active one, with days left', () => {
    expect(catastropheBanner(winter, 230)).toMatch(/^Severe winter: .* · 10 days left$/);
    expect(catastropheBanner(winter, 239)).toMatch(/1 day left$/);
    expect(catastropheBanner({ ...winter, active: false }, 230)).toBeNull();
    expect(catastropheBanner(undefined, 230)).toBeNull();
  });

  it('labels kinds, including unknown ones', () => {
    expect(catastropheLabel('flood_year')).toBe('Flood year');
    expect(catastropheLabel('meteor_strike')).toBe('meteor strike');
  });
});

describe('labels and ranks', () => {
  it('names diets and niches', () => {
    expect([dietLabel(0.1), dietLabel(0.45), dietLabel(0.9)]).toEqual(['herbivore', 'omnivore', 'carnivore']);
    expect(nicheLabel('diet:carnivore')).toBe('carnivores');
    expect(nicheLabel('biome:forest')).toBe('forest');
  });

  it('ranks the player among players in each era, ignoring the wild ecosystem', () => {
    const eras: EraList = {
      era: 2, era_days: 360, first_day: 720, last_day: 1079, days_left: 300,
      eras: [
        { era: 1, first_day: 360, last_day: 719, breeding_adults: 900, species_alive: 6, catastrophes: [0], niches: [],
          rows: [{ player: 0, name: 'wild', share: 0.6, weighted_share: 0.5 }, { player: 2, name: 'bob', share: 0.25, weighted_share: 0.3 },
            { player: 1, name: 'ann', share: 0.15, weighted_share: 0.2 }] },
        { era: 0, first_day: 0, last_day: 359, breeding_adults: 800, species_alive: 6, catastrophes: [], niches: [],
          rows: [{ player: 0, name: 'wild', share: 0.9, weighted_share: 0.9 }, { player: 2, name: 'bob', share: 0.1, weighted_share: 0.1 }] },
      ],
    };
    expect(myEraRanks(eras, 1)).toEqual([{ era: 1, rank: 2, share: 0.15 }, { era: 0, rank: null, share: 0 }]);
  });
});
