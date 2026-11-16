import { describe, expect, it } from 'vitest';
import { designCost } from './cost';
import {
  applyMorphology, applyTrait, blindSpots, designPayload, morphPayload, morphSupport, newEditorDesign, SHARED_GENES, visionReadout,
} from './designMorph';
import { DEFAULT_MORPHOLOGY, defaultMorphology, traitsFromMorphology, type Morphology } from './morphology';
import type { GeneCatalog, MorphInfo } from './protocol';
import { morphologyOf, describeAvatar } from './world/avatar';
import { caption } from './ui/traitText';
import { visionOf } from './vision';

// A catalog shaped like today's `GET /api/genes` (no `morph` section).
const genes = [
  ['body_size', 0.3, 20], ['large_eyes', 0, 8], ['large_ears', 0, 6], ['leg_length', 0, 10], ['tail', 0, 4], ['fangs', 0, 6],
  ['fur', 0, 5], ['boldness', 0.5, 0],
] as const;
const oldCat: GeneCatalog = {
  budget: 100, base_founders: 10, cost_per_extra_founder: 1, cost_per_extra_offspring: 3, max_litter: 8,
  genes: genes.map(([name, d, cost]) => ({ name, default: d, cost })),
};
// The genome package's catalog adds a `morph` section (crates/server design.rs `morph_catalog`).
const morphRows: MorphInfo[] = [
  { name: 'eye_count', kind: 'count', values: [1, 2, 3, 4, 5, 6], default: 2, cost: 0 },
  { name: 'ear_count', kind: 'count', values: [0, 2, 4], default: 2, cost: 0 },
  { name: 'ear_shape', kind: 'enum', values: ['round', 'pointed', 'long', 'fan'], default: 'round', cost: 0 },
  { name: 'leg_count', kind: 'count', values: [0, 2, 4, 6, 8], default: 4, cost: 0 },
  { name: 'tail_shape', kind: 'enum', values: ['thin', 'bushy', 'fin', 'club', 'prehensile'], default: 'thin', cost: 0 },
  { name: 'eye_azimuth', kind: 'axis', min: 0, max: 180, default: 60, cost: 0 },
  { name: 'snout_length', kind: 'axis', min: 0, max: 1, default: 0.5, cost: 0 },
];
const newCat: GeneCatalog = { ...oldCat, morph: morphRows };

describe('feature detection', () => {
  it('only a catalog with a morph section takes morph', () => {
    expect(morphSupport(oldCat)).toBeNull();
    expect(morphSupport({ morph: [] })).toBeNull();
    expect(morphSupport({ morph: 'yes' as unknown as MorphInfo[] })).toBeNull();
    expect(morphSupport(newCat)?.map((r) => r.name)).toContain('eye_count');
  });

  it('an old server never gets morph, whatever the body plan', () => {
    const d = newEditorDesign(oldCat);
    d.name = ' Watchers ';
    applyMorphology(d, { ...d.morphology, eye_count: 3, leg_count: 6, tail_shape: 'fin' });
    const p = designPayload(d, oldCat);
    expect(p).not.toHaveProperty('morph');
    expect(p).not.toHaveProperty('morphology');
    expect(p.name).toBe('Watchers');
    expect(JSON.parse(JSON.stringify(p))).toEqual({ name: 'Watchers', thermo: 'warm', birth: 'live', litter_size: 2, traits: {}, founders: 10 });
  });

  it('a new server gets the non-default morph fields only', () => {
    const d = newEditorDesign(newCat);
    expect(designPayload(d, newCat)).not.toHaveProperty('morph');
    applyMorphology(d, { ...d.morphology, eye_count: 3, ear_shape: 'fan', snout_length: 0.5, eye_size: 0.4 });
    const p = designPayload(d, newCat);
    expect(p.morph).toEqual({ eye_count: 3, ear_shape: 'fan' });
    // The shared size goes in traits, never in morph.
    expect(p.traits).toEqual({ large_eyes: 0.4 });
  });

  it('sends only what the server lists, compared with its defaults', () => {
    const m: Morphology = { ...defaultMorphology(), eye_count: 4, leg_count: 6 };
    expect(morphPayload(m, { morph: [morphRows[0]] })).toEqual({ eye_count: 4 });
    // A server default that differs from the client's: the value is sent.
    expect(morphPayload(defaultMorphology(), { morph: [{ ...morphRows[3], default: 6 }] })).toEqual({ leg_count: 4 });
    // Rows the wire format doesn't know are skipped.
    expect(morphPayload(m, { morph: [{ name: 'wing_count', kind: 'count', default: 0, cost: 0 }] })).toBeUndefined();
  });
});

describe('builder ↔ sliders ↔ avatar', () => {
  it('a new design: default body plan, matching traits', () => {
    const d = newEditorDesign(oldCat);
    expect(d.morphology).toEqual(DEFAULT_MORPHOLOGY);
    for (const [gene, v] of Object.entries(traitsFromMorphology(d.morphology))) expect(d.traits[gene]).toBe(v);
  });

  it('a builder change copies the shared genes into traits and reports the sliders to move', () => {
    const d = newEditorDesign(oldCat);
    const moved = applyMorphology(d, { ...d.morphology, eye_size: 0.5, leg_length: 0.8, eye_count: 3 });
    expect(moved.sort()).toEqual(['large_eyes', 'leg_length']);
    expect(d.traits).toMatchObject({ large_eyes: 0.5, leg_length: 0.8 });
    expect(d.morphology.eye_count).toBe(3);
    // Adding an eye alone moves no slider and costs nothing.
    const before = designCost(designPayload(d, oldCat), oldCat).total;
    expect(applyMorphology(d, { ...d.morphology, eye_count: 4 })).toEqual([]);
    expect(designCost(designPayload(d, oldCat), oldCat).total).toBe(before);
  });

  it('a tail switched off in the builder writes 0 to the tail slider', () => {
    const d = newEditorDesign(oldCat);
    applyMorphology(d, { ...d.morphology, tail_present: true, tail_shape: 'fin', tail_length: 0.6 });
    expect(d.traits.tail).toBe(0.6);
    expect(applyMorphology(d, { ...d.morphology, tail_present: false })).toEqual(['tail']);
    expect(d.traits.tail).toBe(0);
    // The shape is kept for when the tail comes back.
    expect(d.morphology.tail_shape).toBe('fin');
  });

  it('a slider sets its field of the body plan and leaves the rest as the builder left it', () => {
    const d = newEditorDesign(oldCat);
    applyMorphology(d, { ...d.morphology, eye_count: 3, eye_azimuth: 120, leg_count: 6 });
    expect(applyTrait(d, 'large_eyes', 0.7)).toBe(true);
    expect(d.morphology).toMatchObject({ eye_size: 0.7, eye_count: 3, eye_azimuth: 120, leg_count: 6 });
    expect(applyTrait(d, 'tail', 0.3)).toBe(true);
    expect(d.morphology).toMatchObject({ tail_present: true, tail_length: 0.3 });
    expect(applyTrait(d, 'tail', 0)).toBe(true);
    expect(d.morphology.tail_present).toBe(false);
    // Other genes don't touch the body plan.
    const m = { ...d.morphology };
    expect(applyTrait(d, 'fur', 0.9)).toBe(false);
    expect(d.morphology).toEqual(m);
    expect(d.traits.fur).toBe(0.9);
    expect(SHARED_GENES).toEqual(expect.arrayContaining(['large_eyes', 'large_ears', 'leg_length', 'tail', 'fangs']));
  });

  it('the avatar draws the whole body plan', () => {
    const d = newEditorDesign(oldCat);
    const plain = describeAvatar(morphologyOf(d, d.morphology));
    applyMorphology(d, { ...d.morphology, eye_count: 3, leg_count: 6, tail_present: true, tail_shape: 'fin', tail_length: 0.5 });
    const a = morphologyOf(d, d.morphology);
    expect(a).toMatchObject({ eye_count: 3, leg_count: 6, tail_shape: 'fin', tail_present: true });
    const text = describeAvatar(a);
    expect(text).not.toBe(plain);
    expect(text).toContain('three eyes');
    expect(text).toContain('six legs');
    // A slider move shows too (eyes grow: "large three eyes").
    applyTrait(d, 'large_eyes', 0.8);
    expect(describeAvatar(morphologyOf(d, d.morphology))).toContain('large three eyes');
  });

  it('captions mention the body plan behind a size slider', () => {
    const d = newEditorDesign(oldCat);
    expect(caption('large_eyes', d)!.text).not.toContain('eyes ·');
    applyMorphology(d, { ...d.morphology, eye_count: 3, leg_count: 0, ear_count: 4, ear_shape: 'fan' });
    expect(caption('large_eyes', d)!.text).toContain('· three eyes');
    expect(caption('leg_length', d)!.text).toContain('no legs');
    expect(caption('large_ears', d)!.text).toContain('four fan ears');
    // Without a body plan (old callers), captions are as before.
    const { morphology: _m, ...plain } = d;
    expect(caption('large_eyes', plain)!.text).not.toContain('three');
  });
});

describe('vision readout', () => {
  it('the default body reads as the default', () => {
    const rows = visionReadout(defaultMorphology());
    expect(rows.map((r) => r.key)).toEqual(['coverage', 'depth', 'blind']);
    expect(rows.every((r) => r.arrow === '=' && r.trend === 'same')).toBe(true);
    expect(rows[0].value).toBe(`${Math.round(visionOf(DEFAULT_MORPHOLOGY).coverage * 100)}%`);
  });

  it('a third eye on the midline: more view, fewer blind degrees, arrows say so', () => {
    const m = { ...defaultMorphology(), eye_count: 3, eye_azimuth: 130, eye_size: 0.5 };
    const [view, , blind] = visionReadout(m);
    expect(view.trend).toBe('better');
    expect(view.arrow).toMatch(/^↑\d+ pts$/);
    expect(blind.trend).toBe('better');
    expect(blind.arrow).toMatch(/^↓\d+°$/);
    expect(blindSpots(visionOf(m))).toBe('none: sees all round');
  });

  it('eyes swung to the sides lose depth and gain a blind spot ahead', () => {
    const m = { ...defaultMorphology(), eye_azimuth: 120 };
    const [, depth, blind] = visionReadout(m);
    expect(depth.trend).toBe('worse');
    expect(blind.hint).toContain('ahead');
  });
});
