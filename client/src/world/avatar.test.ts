import { describe, expect, it } from 'vitest';
import type { DesignSpec } from '../protocol';
import { DEFAULT_MORPHOLOGY, pairAzimuth as eyePairAzimuth, type Morphology as MorphologyFeatures } from '../morphology';
import {
  avatarDrawing, avatarSvg, describeAvatar, dominantCover, hasFangs, limbOutline, lookOf, morphologyOf,
  shapeBounds, wingStage, type AvatarModel as Morphology,
} from './avatar';
import { describeLook, palette } from './creature';

const design = (traits: Record<string, number> = {}, over: Partial<DesignSpec> = {}): DesignSpec => ({
  name: 'Test', thermo: 'warm', birth: 'live', litter_size: 2, founders: 10, traits: { body_size: 0.3, ...traits }, ...over,
});
const morph = (traits: Record<string, number> = {}, over: Partial<DesignSpec> = {}, f: Partial<MorphologyFeatures> = {}): Morphology =>
  morphologyOf(design(traits, over), f);
const svg = (m: Morphology) => avatarSvg(m, { animate: false });

describe('morphology mapping', () => {
  it('wings: none to 0.25, gliding flaps to 0.5, wings above', () => {
    expect(wingStage(0)).toBe('none');
    expect(wingStage(0.25)).toBe('none');
    expect(wingStage(0.26)).toBe('flaps');
    expect(wingStage(0.5)).toBe('flaps');
    expect(wingStage(0.51)).toBe('wings');
    expect(morph({ wings: 0.6 }).wings.stage).toBe('wings');
    expect(morph({ wings: 0.4 }).wings.stage).toBe('flaps');
  });

  it('fangs: none below 0.1', () => {
    expect(hasFangs(morph({ fangs: 0.09 }))).toBe(false);
    expect(hasFangs(morph({ fangs: 0.1 }))).toBe(true);
    expect(morph({ fangs: 0.8 }).fang_size).toBeCloseTo(0.8);
    expect(describeAvatar(morph({ fangs: 0.05 }))).not.toContain('fang');
    expect(describeAvatar(morph({ fangs: 0.9 }))).toContain('long fangs');
  });

  it('dominant covering follows the server: armor, then fur, then scales, from 0.5', () => {
    expect(dominantCover(0.2, 0.2, 0.2)).toBe('skin');
    expect(dominantCover(0.6, 0.4, 0)).toBe('fur');
    expect(dominantCover(0.6, 0.7, 0)).toBe('scales');
    expect(dominantCover(0.6, 0.6, 0.6)).toBe('armor');
    expect(dominantCover(0.9, 0, 0.6)).toBe('fur');
    // Combinations: the others show as secondary layers.
    const m = morph({ fur: 0.8, scales: 0.4, armor_plates: 0.1 });
    expect(m.cover.dominant).toBe('fur');
    expect(m.cover.fur.role).toBe('dominant');
    expect(m.cover.scales.role).toBe('secondary');
    expect(m.cover.armor.role).toBe('none');
    expect(describeAvatar(m)).toContain('some scales');
  });

  it('contract defaults for features no slider sets yet', () => {
    const m = morph({ tail: 0.04 });
    expect(m).toMatchObject({ ...DEFAULT_MORPHOLOGY, tail_present: false, tail_length: 0.04 });
    expect(m.eye_count).toBe(2);
    expect(m.eye_azimuth).toBe(60);
    expect(m.ear_shape).toBe('round');
    expect(m.leg_count).toBe(4);
    expect(m.snout_length).toBe(0.5);
    expect(morph({ tail: 0.05 }).tail_present).toBe(true);
    expect(morph({ large_eyes: 0.7, large_ears: 0.4, leg_length: 0.9 })).toMatchObject({ eye_size: 0.7, ear_size: 0.4, leg_length: 0.9 });
    // Overrides for later phases.
    expect(morph({}, {}, { leg_count: 6, eye_count: 3, tail_shape: 'fin' })).toMatchObject({ leg_count: 6, eye_count: 3, tail_shape: 'fin' });
  });

  it('eye pairs spread by 50° up to 170° (contract v1)', () => {
    expect([0, 1, 2].map((k) => eyePairAzimuth(60, k))).toEqual([60, 110, 160]);
    expect(eyePairAzimuth(100, 2)).toBe(170);
  });

  it('feet: hooves, claws (also from climbing or burrowing) or soft feet; webbing', () => {
    expect(morph({}).feet.kind).toBe('paw');
    expect(morph({ hooves: 0.6 }).feet.kind).toBe('hoof');
    expect(morph({ claws: 0.6 }).feet.kind).toBe('claw');
    expect(morph({ climbing: 0.7 }).feet).toMatchObject({ kind: 'claw', climbing: true });
    expect(morph({ burrowing: 0.6 }).feet).toMatchObject({ kind: 'claw', digging: true });
    expect(morph({ foot_width: 0.6 }).feet.webbed).toBe(true);
  });

  it('coloring: the world palette; bright and toxic is the warning pattern', () => {
    const m = morph({ coloration: 0.8, toxins: 0.7 });
    expect(m.coloring.kind).toBe('warning');
    expect(m.coloring.palette).toEqual(palette(m.look));
    expect(morph({ coloration: 0.8 }).coloring.kind).toBe('bright');
    expect(morph({ camouflage: 0.6 }).coloring.kind).toBe('camo');
  });

  it('look mirrors the server classes and reads like the inspector', () => {
    const l = lookOf(design({ body_size: 1, carnivory: 0.9, scales: 0.7 }, { thermo: 'cold' }));
    expect(l).toMatchObject({ size: 3, diet: 'carnivore', warm: false, cover: 'scales', locomotion: 'walk' });
    const m = morph({ body_size: 0.1, fur: 0.8, wings: 0.8 });
    expect(m.look.locomotion).toBe('fly');
    expect(describeAvatar(m).startsWith(describeLook(m.look))).toBe(true);
  });

  it('birth type and seasonal coat', () => {
    expect(describeAvatar(morph({}, { birth: 'abandoned_eggs' }))).toContain('lays eggs');
    expect(morph({ seasonal_coat: 0.6 }).extras.seasonalCoat).toBe(true);
    expect(morph({ seasonal_coat: 0.6 }, { thermo: 'cold' }).extras.seasonalCoat).toBe(false);
  });
});

describe('avatar drawing', () => {
  it('changes with every key trait', () => {
    const base = svg(morph());
    const changes: [string, Morphology][] = [
      ['body size', morph({ body_size: 0.8 })],
      ['cold-blooded', morph({}, { thermo: 'cold' })],
      ['muscle', morph({ muscle: 0.8 })],
      ['fat', morph({ fat_stores: 0.8 })],
      ['legs', morph({ leg_length: 0.9 })],
      ['hooves', morph({ hooves: 0.7 })],
      ['claws', morph({ claws: 0.7 })],
      ['foot width', morph({ foot_width: 0.8 })],
      ['tail', morph({ tail: 0.6 })],
      ['flaps', morph({ wings: 0.4 })],
      ['wings', morph({ wings: 0.6 })],
      ['fur', morph({ fur: 0.7 })],
      ['scales', morph({ scales: 0.7 })],
      ['armor', morph({ armor_plates: 0.7 })],
      ['fangs', morph({ fangs: 0.5 })],
      ['eyes', morph({ large_eyes: 0.8 })],
      ['ears', morph({ large_ears: 0.8 })],
      ['nose', morph({ keen_nose: 0.8 })],
      ['camouflage', morph({ camouflage: 0.7 })],
      ['coloration', morph({ coloration: 0.7 })],
      ['carnivory', morph({ carnivory: 0.9 })],
      ['eggs', morph({}, { birth: 'incubated_eggs' })],
      ['winter coat', morph({ seasonal_coat: 0.7 })],
    ];
    for (const [what, m] of changes) expect(svg(m), what).not.toBe(base);
  });

  it('wings 0 to 0.6 adds a wing group; flaps do not', () => {
    expect(svg(morph({ wings: 0 }))).not.toContain('data-part="wing-near"');
    expect(svg(morph({ wings: 0.4 }))).not.toContain('data-part="wing-near"');
    expect(svg(morph({ wings: 0.6 }))).toContain('data-part="wing-near"');
  });

  it('fangs below 0.1 draw nothing', () => {
    expect(svg(morph({ fangs: 0.05 }))).toBe(svg(morph({ fangs: 0 })));
    expect(svg(morph({ fangs: 0.2 }))).not.toBe(svg(morph({ fangs: 0 })));
  });

  it('secondary coverings show faintly, so mixes differ from either alone', () => {
    const fur = svg(morph({ fur: 0.8 }));
    const furScales = svg(morph({ fur: 0.8, scales: 0.4 }));
    const furArmor = svg(morph({ fur: 0.8, armor_plates: 0.3 }));
    expect(furScales).not.toBe(fur);
    expect(furArmor).not.toBe(fur);
    expect(furArmor).not.toBe(furScales);
  });

  it('draws the contract variants: legs, eyes, ears, tail shapes, snout', () => {
    const base = morph({ tail: 0.6 });
    const shapes = (f: Partial<MorphologyFeatures>) => avatarDrawing({ ...base, ...f }).shapes;
    const count = (f: Partial<MorphologyFeatures>, part: string) => shapes(f).filter((s) => s.part === part).length;
    // Each extra pair of legs adds shapes on both sides; no legs none.
    expect(count({ leg_count: 0 }, 'legs-near')).toBe(0);
    const legs = [2, 4, 6, 8].map((n) => count({ leg_count: n as 2 | 4 | 6 | 8 }, 'legs-near'));
    for (let i = 1; i < legs.length; i++) expect(legs[i]).toBeGreaterThan(legs[i - 1]);
    // Legless bodies rest on the ground.
    expect(shapeBounds(shapes({ leg_count: 0 }).filter((s) => s.part === 'body')).maxY).toBeGreaterThan(-3);
    // Eyes: 3 = one pair (one visible) + one on the midline; 6 = three visible.
    const eyeDots = (n: number) => shapes({ eye_count: n }).filter((s) => s.part === 'head' && s.kind === 'ellipse' && s.fill === 0xffffff).length;
    expect([1, 2, 3, 4, 6].map(eyeDots)).toEqual([1, 1, 2, 2, 3]);
    // Ears: none, and four.
    const ears2 = count({ ear_count: 2 }, 'head');
    expect(count({ ear_count: 0 }, 'head')).toBeLessThan(ears2);
    expect(count({ ear_count: 4 }, 'head')).toBeGreaterThan(ears2);
    const outs = new Set(['round', 'pointed', 'long', 'fan'].map((s) => svg({ ...base, ear_shape: s as never })));
    expect(outs.size).toBe(4);
    const tails = new Set(['thin', 'bushy', 'fin', 'club', 'prehensile'].map((s) => svg({ ...base, tail_shape: s as never })));
    expect(tails.size).toBe(5);
    expect(count({ tail_present: false }, 'tail')).toBe(0);
    // A longer snout reaches further forward.
    const reach = (s: number) => shapeBounds(shapes({ snout_length: s }).filter((x) => x.part === 'head')).maxX;
    expect(reach(0.9)).toBeGreaterThan(reach(0.1));
  });

  it('places eyes by azimuth in side view: forward shows both near the snout, side sits high and back', () => {
    const base = morph({});
    // The eye highlights (one white dot per drawn eye), in drawing order.
    const eyes = (eye_azimuth: number, eye_count = 2) =>
      avatarDrawing({ ...base, eye_azimuth, eye_count }).shapes.filter((s) => s.part === 'head' && s.kind === 'ellipse' && s.fill === 0xffffff) as { x: number; y: number }[];
    const [def] = eyes(60);
    const fwd = eyes(15);
    const [side] = eyes(110);
    // Forward: the far eye shows too, further forward than the near one, and the near one is nearer the snout than by default.
    expect(fwd).toHaveLength(2);
    expect(fwd[0].x).toBeGreaterThan(fwd[1].x);
    expect(fwd[1].x).toBeGreaterThan(def.x);
    // Side: further back and higher.
    expect(eyes(110)).toHaveLength(1);
    expect(side.x).toBeLessThan(def.x);
    expect(side.y).toBeLessThan(def.y);
    // Only the pairs: a lone midline eye has no far partner.
    expect(eyes(15, 1)).toHaveLength(1);
    expect(eyes(15, 3)).toHaveLength(3);
  });

  it('animates only when asked', () => {
    const m = morph({ tail: 0.6, wings: 0.8 });
    expect(avatarSvg(m, { animate: false })).not.toContain('<animate');
    expect(avatarSvg(m, { animate: true })).toContain('<animateTransform');
  });

  it('fits the view: every design stays inside the viewBox', () => {
    const extremes = [
      morph({ body_size: 1, wings: 1, tail: 1, leg_length: 1, muscle: 1, fat_stores: 1, armor_plates: 1, large_ears: 1 }),
      morph({ body_size: 1, tail: 1, carnivory: 1, fangs: 1 }, { thermo: 'cold' }, { snout_length: 1, tail_shape: 'club', leg_count: 8 }),
    ];
    for (const m of extremes) {
      const s = svg(m);
      const k = Number(/scale\(([\d.]+)\)/.exec(s)![1]);
      const tx = Number(/translate\(([-\d.]+) /.exec(s)![1]);
      const b = shapeBounds(avatarDrawing(m).shapes);
      expect(tx + k * b.minX).toBeGreaterThanOrEqual(0);
      expect(tx + k * b.maxX).toBeLessThanOrEqual(300);
      expect(180 + k * b.minY).toBeGreaterThanOrEqual(0);
    }
  });

  it('limb outlines wrap the centerline', () => {
    const o = limbOutline([[0, 0], [0, 10]], [4, 2]);
    expect(o).toHaveLength(4);
    expect(Math.abs(o[0][0] - o[3][0])).toBeCloseTo(4);
  });
});
