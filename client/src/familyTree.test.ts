import { describe, expect, it } from 'vitest';
import fixture from './familyTree.fixture.json';
import {
  bodyOf, buildFamilyTree, changeLabels, changesBetween, checkpoints, describeFamily, designOf, MAX_LABELS, MAX_MIDDLE,
} from './familyTree';
import type { BodyPlanMode, FamilySample, FamilyTree } from './protocol';
import { DEMO_TREE } from './ui/familyDemo';
import { morphologyOf, avatarDrawing, drawingShapes } from './world/avatar';

const plan = (over: Partial<BodyPlanMode> = {}): BodyPlanMode => ({
  eye_count: 2, eye_azimuth: 60, ear_count: 2, ear_shape: 'round', leg_count: 4, tail_shape: 'thin', snout_length: 0.5,
  horn_size: 0, horn_form: 'curved', wing_aspect: 0.5, pattern: 'plain', ...over,
});
const sample = (day: number, genes: Record<string, number>, m: Partial<BodyPlanMode> = {}, alive = 10): FamilySample => ({ day, alive, genes, morph: plan(m) });

describe('trait changes', () => {
  it('names what changed in the players’ trait names, largest first', () => {
    const a = sample(0, { large_ears: 0.3, leg_length: 0.3 });
    const b = sample(9, { large_ears: 0.5, leg_length: 0.34 }, { pattern: 'stripes' });
    expect(changeLabels(a, b)).toEqual(['Stripes appeared', 'Ear size ↑']);
    // Below the step nothing is said.
    expect(changesBetween(a, sample(1, { large_ears: 0.35, leg_length: 0.3 }))).toEqual([]);
  });

  it('notices structures, shapes and placement', () => {
    const a = sample(0, { tail: 0.4 });
    expect(changeLabels(a, sample(1, { tail: 0.4 }, { horn_size: 0.5, horn_form: 'branched' }))).toEqual(['Antlers appeared']);
    expect(changeLabels(sample(0, {}, { horn_size: 0.5 }), sample(1, {}))).toEqual(['Horns lost']);
    expect(changeLabels(a, sample(1, { tail: 0.4 }, { leg_count: 6 }))).toEqual(['Six legs']);
    expect(changeLabels(a, sample(1, { tail: 0.4 }, { tail_shape: 'bushy' }))).toEqual(['Bushy tail']);
    expect(changeLabels(a, sample(1, { tail: 0.4 }, { ear_count: 0 }))).toEqual(['No ears']);
    expect(changeLabels(a, sample(1, { tail: 0.4 }, { eye_azimuth: 100 }))).toEqual(['Eyes more to the side']);
    expect(changeLabels(sample(0, {}, { pattern: 'spots' }), sample(1, {}))).toEqual(['Spots faded']);
    expect(changeLabels(sample(0, { endothermy: 0.9 }), sample(1, { endothermy: 0.2 }))).toEqual(['Became cold-blooded']);
    // Never more than a few labels.
    const many = sample(1, { large_ears: 0.9, leg_length: 0.9, fur: 0.9, tail: 0.9 }, { pattern: 'spots', leg_count: 6 });
    expect(changeLabels(sample(0, {}), many)).toHaveLength(MAX_LABELS);
  });
});

describe('the trunk', () => {
  it('keeps the founding, today and the days the look changed', () => {
    const t = [
      sample(0, { large_ears: 0.3 }), sample(1, { large_ears: 0.31 }), sample(2, { large_ears: 0.45 }),
      sample(3, { large_ears: 0.46 }), sample(4, { large_ears: 0.47 }, { pattern: 'spots' }), sample(5, { large_ears: 0.48 }, { pattern: 'spots' }),
    ];
    expect(checkpoints(t).map((s) => s.day)).toEqual([0, 2, 4, 5]);
    expect(checkpoints(t.slice(0, 1)).map((s) => s.day)).toEqual([0]);
    // At most MAX_MIDDLE between, the largest changes, in time order.
    const wobble = Array.from({ length: 20 }, (_, i) => sample(i, { large_ears: i % 2 ? 0.2 : 0.2 + 0.1 + i * 0.02 }));
    const kept = checkpoints(wobble);
    expect(kept.length).toBe(MAX_MIDDLE + 2);
    expect(kept.map((s) => s.day)).toEqual([...kept.map((s) => s.day)].sort((a, b) => a - b));
  });
});

describe('the demo tree', () => {
  const m = buildFamilyTree(DEMO_TREE);

  it('runs from the founding to today with the changes on each stage', () => {
    expect(m.empty).toBe(false);
    expect(m.extinct).toBe(false);
    expect(m.speciesName).toBe('Dune grazers');
    expect(m.trunk[0]).toMatchObject({ kind: 'founding', title: 'Founding', changes: [] });
    expect(m.trunk[m.trunk.length - 1]).toMatchObject({ kind: 'now', title: 'Today' });
    const all = m.trunk.flatMap((n) => n.changes);
    expect(all).toContain('Stripes appeared');
    expect(all).toContain('Antlers appeared');
    expect(all.some((c) => c.startsWith('Ear size ↑'))).toBe(true);
  });

  it('hangs the split species off their stage, marking the extinct one', () => {
    const ids = m.branches.map((b) => b.species);
    expect(ids).toEqual([7, 9, 12]);
    const marsh = m.branches.find((b) => b.species === 7)!;
    expect(marsh.node.extinct).toBe(true);
    expect(marsh.node.subtitle).toContain('extinct on day 56');
    expect(m.trunk[marsh.fromTrunk].day).toBeLessThanOrEqual(20);
    const ridge = m.branches.find((b) => b.species === 12)!;
    expect(ridge).toMatchObject({ fromBranch: 9, depth: 1 });
    expect(ridge.node.changes).toContain('Straight horns');
    expect(m.branches.find((b) => b.species === 9)!.node.subtitle).toContain('18 adults (12 yours)');
    expect(describeFamily(m)).toContain('3 branches split off, 1 extinct');
  });

  it('gives every node a portrait with its real covering and features', () => {
    const today = m.trunk[m.trunk.length - 1];
    const model = morphologyOf(today.design, today.body);
    // The founders' coat was fur (0.55); the lineage shed it (0.4): the portraits follow.
    expect(morphologyOf(m.trunk[0].design, m.trunk[0].body).cover.dominant).toBe('fur');
    expect(model.cover.dominant).toBe('skin');
    expect(model.pattern).toBe('stripes');
    expect(model.horns).toBeGreaterThan(0);
    const tags = drawingShapes(avatarDrawing(model)).map((s) => s.tag);
    expect(tags).toContain('horn');
    expect(tags).toContain('pattern');
  });

  it('marks a lineage that died out', () => {
    const gone: FamilyTree = { ...DEMO_TREE, alive: 0, timeline: DEMO_TREE.timeline.slice(0, 4) };
    const g = buildFamilyTree(gone);
    expect(g.extinct).toBe(true);
    expect(g.trunk[g.trunk.length - 1]).toMatchObject({ kind: 'extinct', extinct: true, title: 'Last seen' });
    expect(describeFamily(g)).toContain('died out');
  });

  it('draws nothing without a lineage', () => {
    const none = buildFamilyTree({ player: 1, lineage: null, name: null, day: 3, alive: 0, timeline: [], species: [] });
    expect(none.empty).toBe(true);
    expect(describeFamily(none)).toMatch(/No lineage yet/);
  });
});

describe('designs and bodies for portraits', () => {
  it('reads thermoregulation and birth from the genes, and clamps the body plan', () => {
    expect(designOf('x', { endothermy: 0.2, birth_mode: 0.2 })).toMatchObject({ thermo: 'cold', birth: 'abandoned_eggs' });
    expect(designOf('x', { endothermy: 0.9, birth_mode: 0.5 })).toMatchObject({ thermo: 'warm', birth: 'incubated_eggs' });
    expect(bodyOf(plan({ leg_count: 5, ear_count: 3, pattern: 'plaid', horn_form: 'spiral' }))).toMatchObject({ leg_count: 6, ear_count: 4, pattern: 'plain', horn_form: 'curved' });
    expect(bodyOf(undefined)).toEqual({});
  });
});

// familyTree.fixture.json is written by the Rust test
// `game::family::tests::the_client_family_fixture_is_current` (crates/server/src/game/family.rs):
// a real lineage with antlers and stripes, three recorded days and today.
describe('the server’s tree', () => {
  it('lays out a real tree', () => {
    const t = fixture as unknown as FamilyTree;
    const m = buildFamilyTree(t);
    expect(m.empty).toBe(false);
    expect(m.name).toBe('Stags');
    expect(m.trunk[0].kind).toBe('founding');
    expect(m.trunk[m.trunk.length - 1].kind).toBe(t.alive > 0 ? 'now' : 'extinct');
    const body = m.trunk[0].body;
    expect(body).toMatchObject({ pattern: 'stripes', horn_form: 'branched' });
    const model = morphologyOf(m.trunk[0].design, body);
    expect(model.horns).toBeGreaterThan(0);
    expect(model.leg_length).toBeGreaterThan(0.3);
  });
});
