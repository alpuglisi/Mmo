import { describe, expect, it } from 'vitest';
import { designPlan, fertility, gapBetween, hybridAdvice, neighbours, originExplain } from './hybrid';
import { foundingHint } from './foodHint';
import type { DesignSpec, SpeciesView, WorldSummary } from './protocol';

const design = (traits: Record<string, number> = {}, over: Partial<DesignSpec> = {}): DesignSpec => ({
  name: 'Mine', thermo: 'warm', birth: 'live', litter_size: 2, founders: 20, traits: { body_size: 0.3, carnivory: 0, ...traits }, ...over,
});

const sp = (id: number, name: string, over: Partial<SpeciesView> = {}): SpeciesView => ({
  id, name, origin: 'initial', founded_day: 0, dominant: 0, dominant_name: 'wild', dominant_share: 1, adults: 120, peak_adults: 150,
  lineages: [{ lineage: id, name, owner: 0, adults: 120 }], carnivory: 0.05, body_size: 0.32, endothermy: 0.9, birth_mode: 0.85,
  leg_count: 4, eye_count: 2, ...over,
});

describe('the body-plan gap', () => {
  it('takes the largest difference and ramps fertility from 0.12 to 0.30', () => {
    const a = designPlan(design({ carnivory: 0.5 }));
    expect(gapBetween(a, { ...a, carnivory: 0.1 })).toEqual({ gap: 0.4, by: 'diet' });
    expect(gapBetween(a, { ...a, leg_count: 8 }).by).toBe('legs');
    expect([0, 0.12, 0.21, 0.3, 0.5].map(fertility)).toEqual([1, 1, 0.5, 0, 0]);
  });
});

describe('warm and cold blood', () => {
  it('are a third as fertile together, not sterile (sim thermo_hybrid 0.7)', () => {
    const warm = designPlan(design());
    const cold = designPlan(design({}, { thermo: 'cold' }));
    const g = gapBetween(warm, cold);
    expect(g.by).toBe('thermoregulation');
    expect(g.gap).toBeCloseTo(0.24, 5);
    expect(fertility(g.gap)).toBeCloseTo(1 / 3, 2);
    // Abandoned eggs on top still make them sterile.
    const frog = designPlan(design({}, { thermo: 'cold', birth: 'abandoned_eggs' }));
    expect(fertility(gapBetween(warm, frog).gap)).toBe(0);
  });
});

describe('the builder’s interbreeding check', () => {
  const species = [
    sp(1, 'Steppe antelope'),
    sp(2, 'Marsh foxes', { carnivory: 0.85, dominant: 0 }),
    sp(3, 'Broker voles', { dominant: 7, dominant_name: 'GeneBroker', body_size: 0.38, lineages: [{ lineage: 9, name: 'Voles', owner: 7, adults: 80 }] }),
    sp(4, 'Gone', { extinct_day: 40 }),
    sp(5, 'Mine', { lineages: [{ lineage: 2, name: 'Mine', owner: 3, adults: 50 }], dominant: 3 }),
  ];

  it('warns about wild species in range and says what to change', () => {
    const near = neighbours(design(), species, 3);
    expect(near.map((n) => n.species.name)).toEqual(['Steppe antelope', 'Broker voles']);
    const a = hybridAdvice(design(), species, 3);
    expect(a.level).toBe('warn');
    expect(a.headline).toBe('Interbreeds with Steppe antelope and 1 more');
    expect(a.points[0]).toMatch(/wild genes, which count for the wild ecosystem/);
    // Only changes that get there alone: diet within 0..1, body size, another birth type.
    expect(a.points[1]).toMatch(/^To keep your genes your own from Steppe antelope, one of these is enough: raise diet \(carnivory\) to 0\.43 or more; .*body size/);
    expect(a.points[1]).not.toMatch(/thermoregulation further|birth mode 0\./);
    // A player's species is the player's, named by them, never "wild".
    expect(a.points.some((p) => p.includes("GeneBroker's Broker voles"))).toBe(true);
    expect(a.points[0]).not.toMatch(/Broker voles/);
    expect(a.market).toMatch(/you could breed with GeneBroker/);
    expect(a.confidence).toMatch(/averages: animals vary/);
  });

  it('shows the server-measured fertility instead of the estimate, one figure per species', () => {
    const est = hybridAdvice(design(), species, 3);
    expect(est.points[0]).toMatch(/Steppe antelope \(120 adults, 100% fertile\)/);
    const m = hybridAdvice(design(), species, 3, new Map([[1, 0.62]]));
    expect(m.points[0]).toMatch(/Steppe antelope \(120 adults, 62% fertile, measured\)/);
    expect(m.points[0]).not.toMatch(/100% fertile/);
  });

  it('says a distant design keeps its genes, and who it can never breed with', () => {
    const a = hybridAdvice(design({ carnivory: 0.5 }), species, 3);
    expect(a.level).toBe('ok');
    expect(a.market).toMatch(/no other player's species is within breeding range of this design's averages/);
    // Never a flat "no one": a species just past the sterile gap reads as possible.
    expect(a.points.join(' ')).toMatch(/Marsh foxes \(120 adults, sterile on average, some pairs fertile\)/);
    // Older servers (no thermoregulation or birth in the list): nothing to say.
    expect(neighbours(design(), species.map((s) => ({ ...s, endothermy: undefined })), 3)).toEqual([]);
  });
});

describe('species that are mostly wild genes', () => {
  it('stay a player\'s species, with the wild share said', () => {
    const species = [sp(9, 'Broker voles', { dominant: 0, dominant_name: 'wild', dominant_share: 0.59,
      lineages: [{ lineage: 9, name: 'Voles', owner: 7, owner_name: 'GeneBroker2', adults: 900 }, { lineage: 1, name: 'Steppe antelope', owner: 0, adults: 200 }] })];
    const a = hybridAdvice(design(), species, 3);
    expect(a.level).toBe('warn');
    expect(a.headline).toBe("Interbreeds with GeneBroker2's Broker voles");
    expect(a.points.join(' ')).not.toMatch(/wild species/);
    expect(a.points.join(' ')).toMatch(/mostly wild genes by now \(59%\)/);
    expect(a.market).toMatch(/breed with GeneBroker2/);
  });
});

describe('whose genes', () => {
  it('explains why the share is below the head count', () => {
    const e = originExplain([
      { player: 3, name: 'me', alleles: 30, share: 0.3 },
      { player: 0, name: 'wild', alleles: 50, share: 0.5 },
      { player: 7, name: 'Broker', alleles: 20, share: 0.2 },
    ], 3);
    expect(e.headline).toBe('Only 30% of your animals\' genes are yours');
    expect(e.points.join(' ')).toMatch(/counts genes \(alleles\), not heads/);
    expect(e.points.join(' ')).toMatch(/50% came from wild animals/);
    expect(e.points.join(' ')).toMatch(/20% came from other players/);
    expect(originExplain([{ player: 3, name: 'me', alleles: 9, share: 1 }], 3).headline).toBe('All of your animals\' genes are yours');
  });
});

describe('the founding-size hint', () => {
  const world = (season: string, drought: number): WorldSummary => ({
    clock: { year: 0, day: 3, hour: 6, season, daytime: true, tick: 0 } as unknown as WorldSummary['clock'],
    weather: { drought, flood: 0, mean_wind: 0, storm: false } as unknown as WorldSummary['weather'],
    width: 96, height: 96, organisms: 500, agents: 500, lineages: 4, lineage_cap: 1500,
  });
  it('warns at fewer founders when the land offers less food', () => {
    expect(foundingHint(design({}, { founders: 40 }), world('spring', 0)).level).toBe('ok');
    // Summer and a drought: 60 × 0.75 × 0.6 = 27 founders.
    const dry = foundingHint(design({}, { founders: 40 }), world('summer', 0.4));
    expect(dry.level).toBe('warn');
    expect(dry.text).toMatch(/around \d+ founders spread out and grow into the land now/i);
  });

  it('warns about big foundings, more so in summer drought', () => {
    expect(foundingHint(design(), world('spring', 0)).level).toBe('ok');
    expect(foundingHint(design(), world('spring', 0)).text).toMatch(/population cap of 1,500/);
    const big = foundingHint(design({}, { founders: 120 }), world('summer', 0.5));
    expect(big.level).toBe('warn');
    expect(big.text).toMatch(/120 founders .* worse now that it is summer and there is a drought/);
    // Heavy animals: the advice is a smaller head count, never more than the founding.
    const heavy = foundingHint(design({ body_size: 1 }), world('spring', 0));
    expect(heavy.level).toBe('warn');
    const m = heavy.text.match(/around (\d+) founders/);
    expect(m && Number(m[1])).toBeLessThan(20);
    expect(heavy.text).toMatch(/big animals/);
    // The soft cap, not "births stop at 1,500".
    expect(foundingHint(design(), { ...world('spring', 0), lineage_hard_cap: 3000 }).text).toMatch(/past 1,500, where breeding slows to half pace; births stop at 3,000/);
  });
});

describe('sterile options', () => {
  it('offers only changes that reach the sterile gap alone, within the editor\'s range', async () => {
    const { sterileOptions } = await import('./hybrid');
    const wild = sp(1, 'W', { carnivory: 0.05, body_size: 0.32, birth_mode: 0.85, endothermy: 0.9 });
    const o = sterileOptions(design(), wild);
    expect(o[0]).toBe('raise diet (carnivory) to 0.43 or more');
    expect(o.some((x) => x.startsWith('raise body size to 0.70'))).toBe(true);
    expect(o).toContain('choose abandoned eggs');
    expect(o).not.toContain('choose incubated eggs');
    // Already far enough in birth: no birth advice; no thermoregulation ever.
    expect(sterileOptions(design({}, { birth: 'abandoned_eggs' }), wild).some((x) => x.includes('birth') || x.includes('eggs'))).toBe(false);
    expect(o.join(' ')).not.toMatch(/thermo/);
  });
});
