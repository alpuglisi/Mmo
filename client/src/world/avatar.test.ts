import { describe, expect, it } from 'vitest';
import type { DesignSpec } from '../protocol';
import { DEFAULT_MORPHOLOGY, pairAzimuth as eyePairAzimuth, type Morphology as MorphologyFeatures } from '../morphology';
import {
  avatarDrawing, avatarSpriteSvg, avatarSvg, captionOf, coveringOf, describeAvatar, dominantCover, drawingShapes, hasFangs, lookOf, morphologyOf,
  shapeBounds, wingStage, type AvatarDrawing, type AvatarModel as Morphology,
} from './avatar';
import { describeLook, palette } from './creature';
import { spriteDrawing } from './spriteArt';
import { limbOutline } from '../art/geom';
import { drawingBounds } from '../art/render';
import { GALLERY } from '../art/gallery';

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
    expect(describeAvatar(m).startsWith(captionOf(m))).toBe(true);
    expect(captionOf(m)).toBe(describeLook(m.look).replace(" fur ", " feathers "));
  });

  it('birth type and seasonal coat', () => {
    expect(describeAvatar(morph({}, { birth: 'abandoned_eggs' }))).toContain('lays eggs');
    expect(morph({ seasonal_coat: 0.6 }).extras.seasonalCoat).toBe(true);
    expect(morph({ seasonal_coat: 0.6 }, { thermo: 'cold' }).extras.seasonalCoat).toBe(false);
  });
});

/** Shapes with a tag (eyes' glints, scales, hair...). */
const tagged = (d: AvatarDrawing, tag: string) => drawingShapes(d).filter((s) => s.tag === tag);
/** Silhouette pieces with a tag or tag prefix (body, head, leg-0, ear-0, tail...). */
const pieces = (d: AvatarDrawing, tag: string) => d.groups.flatMap((g) => g.pieces.filter((p) => p.tag === tag || p.tag.startsWith(`${tag}-`)));
const groupOf = (d: AvatarDrawing, part: string) => d.groups.find((g) => g.part === part);

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

  it('wings 0 to 0.6 adds a wing group; gliding flaps join the body silhouette', () => {
    expect(svg(morph({ wings: 0 }))).not.toContain('data-part="wing-near"');
    expect(svg(morph({ wings: 0.4 }))).not.toContain('data-part="wing-near"');
    expect(pieces(avatarDrawing(morph({ wings: 0.4 })), 'flaps')).toHaveLength(1);
    expect(svg(morph({ wings: 0.6 }))).toContain('data-part="wing-near"');
  });

  it('fangs below 0.1 draw nothing; herbivores grow tusks, others fangs', () => {
    expect(svg(morph({ fangs: 0.05 }))).toBe(svg(morph({ fangs: 0 })));
    expect(svg(morph({ fangs: 0.2 }))).not.toBe(svg(morph({ fangs: 0 })));
    expect(tagged(avatarDrawing(morph({ fangs: 0.5 })), 'tusk')).toHaveLength(1);
    expect(tagged(avatarDrawing(morph({ fangs: 0.5, carnivory: 0.8 })), 'fang')).toHaveLength(1);
  });

  it('secondary coverings show faintly, so mixes differ from either alone', () => {
    const fur = svg(morph({ fur: 0.8 }));
    const furScales = svg(morph({ fur: 0.8, scales: 0.4 }));
    const furArmor = svg(morph({ fur: 0.8, armor_plates: 0.3 }));
    expect(furScales).not.toBe(fur);
    expect(furArmor).not.toBe(fur);
    expect(furArmor).not.toBe(furScales);
  });

  it('merges body, neck, head, near ears and near legs into one silhouette (one outline, no seams)', () => {
    const d = avatarDrawing(morph({ tail: 0.6 }));
    const main = groupOf(d, 'body')!;
    const tags = main.pieces.map((p) => p.tag);
    for (const t of ['body', 'neck', 'head', 'ear-0', 'leg-0', 'leg-1', 'foot-0']) expect(tags, t).toContain(t);
    // Far legs and the far ear are darker groups behind; the tail is its own group so it can sway.
    expect(groupOf(d, 'legs-far')!.tones.base).not.toBe(main.tones.base);
    expect(groupOf(d, 'ears-far')).toBeDefined();
    expect(groupOf(d, 'tail')!.sway).toBeDefined();
    const order = d.groups.map((g) => g.part);
    expect(order.indexOf('legs-far')).toBeLessThan(order.indexOf('body'));
    expect(order.indexOf('tail')).toBeLessThan(order.indexOf('body'));
    // The renderer outlines the merged pieces once, then glazes the shade and highlight rims.
    const s = svg(morph());
    expect(s).toContain('mask=');
    expect(s).toContain('clip-path=');
  });

  it('draws the contract variants: legs, eyes, ears, tail shapes, snout', () => {
    const base = morph({ tail: 0.6 });
    const draw = (f: Partial<MorphologyFeatures>) => avatarDrawing({ ...base, ...f });
    // Leg pieces: one per leg on each side; none without legs.
    expect(pieces(draw({ leg_count: 0 }), 'leg')).toHaveLength(0);
    expect([2, 4, 6, 8].map((n) => pieces(draw({ leg_count: n as 2 | 4 | 6 | 8 }), 'leg').length)).toEqual([2, 4, 6, 8]);
    // Legless bodies rest on the ground.
    expect(Math.max(...pieces(draw({ leg_count: 0 }), 'body')[0].points.map(([, y]) => y))).toBeGreaterThan(-3);
    // Eyes: 3 = one pair (one visible) + one on the midline; 6 = three visible.
    const glints = (n: number) => tagged(draw({ eye_count: n }), 'glint').length;
    expect([1, 2, 3, 4, 6].map(glints)).toEqual([1, 1, 2, 2, 3]);
    // Ears: none, a pair (the near ear in the silhouette, the far one behind), two pairs.
    expect(pieces(draw({ ear_count: 0 }), 'ear')).toHaveLength(0);
    expect(pieces(draw({ ear_count: 2 }), 'ear')).toHaveLength(2);
    expect(pieces(draw({ ear_count: 4 }), 'ear')).toHaveLength(4);
    expect(new Set(['round', 'pointed', 'long', 'fan'].map((s) => svg({ ...base, ear_shape: s as never }))).size).toBe(4);
    expect(new Set(['thin', 'bushy', 'fin', 'club', 'prehensile'].map((s) => svg({ ...base, tail_shape: s as never }))).size).toBe(5);
    expect(groupOf(draw({ tail_present: false }), 'tail')).toBeUndefined();
    // A longer snout reaches further forward.
    expect(draw({ snout_length: 0.9 }).anchors.snoutTip[0]).toBeGreaterThan(draw({ snout_length: 0.1 }).anchors.snoutTip[0]);
  });

  it('anchors move linearly with their sizes (the builder handles ride them)', () => {
    const base = morph({ tail: 0.6, carnivory: 0.8 });
    const at = (f: Partial<MorphologyFeatures>) => avatarDrawing({ ...base, ...f }).anchors;
    const linear = (pick: (a: ReturnType<typeof at>) => [number, number], field: keyof MorphologyFeatures, lo = 0) => {
      const [a, b, c] = [lo, (lo + 1) / 2, 1].map((v) => pick(at({ [field]: v })));
      expect(b[0], field).toBeCloseTo((a[0] + c[0]) / 2, 0);
      expect(b[1], field).toBeCloseTo((a[1] + c[1]) / 2, 0);
      expect(Math.hypot(c[0] - a[0], c[1] - a[1]), field).toBeGreaterThan(5);
    };
    linear((a) => a.snoutTip, 'snout_length');
    linear((a) => a.earTip!, 'ear_size');
    linear((a) => a.tailTip!, 'tail_length');
    // Feet stay on the ground: it is the body that rises, so measure the foot from the torso centre.
    linear((a) => [a.foot![0], a.foot![1] - a.body.cy], "leg_length");
    linear((a) => a.fangTip, 'fang_size', 0.2);
  });

  it('places eyes by azimuth in side view: forward shows both near the snout, side sits high and back', () => {
    const base = morph({});
    const eyes = (eye_azimuth: number, eye_count = 2) => tagged(avatarDrawing({ ...base, eye_azimuth, eye_count }), 'glint') as unknown as { x: number; y: number }[];
    const [def] = eyes(60);
    const fwd = eyes(15);
    const [side] = eyes(110);
    expect(fwd).toHaveLength(2);
    expect(fwd[0].x).toBeGreaterThan(fwd[1].x);
    expect(fwd[1].x).toBeGreaterThan(def.x);
    expect(eyes(110)).toHaveLength(1);
    expect(side.x).toBeLessThan(def.x);
    expect(side.y).toBeLessThan(def.y);
    expect(eyes(15, 1)).toHaveLength(1);
    expect(eyes(15, 3)).toHaveLength(3);
  });

  it('animates only when asked', () => {
    const m = morph({ tail: 0.6, wings: 0.8 });
    expect(avatarSvg(m, { animate: false })).not.toContain('<animate');
    expect(avatarSvg(m, { animate: true })).toContain('<animateTransform');
  });

  it('prefixes clip and mask ids, so two avatars can share a page', () => {
    const a = avatarSvg(morph({ tail: 0.5 }), { animate: false, id: 'one' });
    const ids = [...a.matchAll(/ id="([^"]+)"/g)].map((x) => x[1]);
    expect(ids.length).toBeGreaterThan(3);
    for (const id of ids) expect(id.startsWith('one-')).toBe(true);
    expect(new Set(ids).size).toBe(ids.length);
  });

  it('fits the view: every design stays inside the viewBox', () => {
    const extremes = [
      morph({ body_size: 1, wings: 1, tail: 1, leg_length: 1, muscle: 1, fat_stores: 1, armor_plates: 1, large_ears: 1 }),
      morph({ body_size: 1, tail: 1, carnivory: 1, fangs: 1 }, { thermo: 'cold' }, { snout_length: 1, tail_shape: 'club', leg_count: 8 }),
      morph({ body_size: 0, fur: 1, wings: 1, tail: 1, large_ears: 1 }, {}, { ear_shape: 'long', tail_shape: 'bushy' }),
    ];
    for (const m of extremes) {
      const s = svg(m);
      const k = Number(/scale\(([\d.]+)\)/.exec(s)![1]);
      const tx = Number(/translate\(([-\d.]+) /.exec(s)![1]);
      const b = drawingBounds(avatarDrawing(m));
      expect(tx + k * b.minX).toBeGreaterThanOrEqual(0);
      expect(tx + k * b.maxX).toBeLessThanOrEqual(300);
      expect(180 + k * b.minY).toBeGreaterThanOrEqual(0);
    }
  });

  it('renders every gallery creature, portrait and sprite', () => {
    for (const g of GALLERY) {
      const m = morphologyOf(g.design, g.body);
      const s = avatarSvg(m, { animate: false });
      expect(s, g.id).not.toMatch(/NaN|Infinity|undefined/);
      const sp = avatarSpriteSvg(m, 48);
      expect(sp, g.id).not.toMatch(/NaN|Infinity|undefined/);
      // The sprite level drops textures: less markup.
      expect(sp.length, g.id).toBeLessThan(s.length);
    }
  });

  it('limb outlines wrap the centerline', () => {
    const o = limbOutline([[0, 0], [0, 10]], [4, 2]);
    expect(o).toHaveLength(4);
    expect(Math.abs(o[0][0] - o[3][0])).toBeCloseTo(4);
  });
});

describe('covering: caption and drawing agree', () => {
  /** What the silhouette and textures say the covering is. */
  const drawnCover = (m: Morphology) => {
    const d = avatarDrawing(m);
    const body = pieces(d, 'body')[0];
    if (tagged(d, 'plate').length && m.cover.armor.role === 'dominant') return 'armor';
    if (tagged(d, 'feather').length) return 'feathers';
    if (tagged(d, 'hair').length && body.points.length > 32) return 'fur';
    if (tagged(d, 'scale').length > 10 && tagged(d, 'scute').length) return 'scales';
    return 'skin';
  };

  it('names in the caption the covering the avatar draws, warm or cold', () => {
    const coverings: [Record<string, number>, string][] = [
      [{ fur: 0.5 }, 'fur'], [{ fur: 1 }, 'fur'], [{ fur: 0.6, scales: 0.55 }, 'fur'], [{ scales: 0.6 }, 'scales'],
      [{ scales: 0.6, fur: 0.4 }, 'scales'], [{ armor_plates: 0.7, fur: 0.6 }, 'armor'], [{ fur: 0.3 }, 'skin'], [{ fur: 0.7, wings: 0.7 }, 'feathers'],
    ];
    for (const thermo of ['warm', 'cold'] as const) {
      for (const [traits, cover] of coverings) {
        const m = morph(traits, { thermo });
        const what = `${thermo} ${JSON.stringify(traits)}`;
        expect(coveringOf(m), what).toBe(cover);
        expect(captionOf(m), what).toContain(` ${cover} `);
        expect(describeAvatar(m).startsWith(captionOf(m)), what).toBe(true);
        expect(drawnCover(m), what).toBe(cover);
      }
    }
  });

  it('the caption is the world description, but for plumage on a winged furred body', () => {
    const m = morph({ fur: 0.5 }, { thermo: 'cold' });
    expect(captionOf(m)).toBe(describeLook(m.look));
    expect(captionOf(m)).toBe('small cold-blooded herbivore · fur · drab · walks');
    const bird = morph({ fur: 0.8, wings: 0.8, body_size: 0.1 });
    expect(bird.look.cover).toBe('fur');
    expect(captionOf(bird)).toContain(' feathers ');
  });

  it('draws the e2e founder (small cold-blooded herbivore with fur 0.5) furred: a pelt, no scales, no lizard ear hole', () => {
    const m = morph({ fur: 0.5 }, { thermo: 'cold' });
    // The cold-blooded tint stays (the world sprite's palette).
    expect(m.coloring.palette).toEqual(palette(m.look));
    const d = avatarDrawing(m);
    expect(pieces(d, 'body')[0].points.length).toBeGreaterThan(32);
    expect(tagged(d, 'scale')).toHaveLength(0);
    expect(tagged(d, 'ear-hole')).toHaveLength(0);
    expect(tagged(avatarDrawing(morph({}, { thermo: 'cold' })), 'ear-hole')).toHaveLength(1);
  });

  it('a fuller coat reaches further out', () => {
    const extent = (m: Morphology) => {
      const b = shapeBounds([{ part: 'body', kind: 'poly', points: pieces(avatarDrawing(m), 'body')[0].points }]);
      return b.maxY - b.minY;
    };
    expect(extent(morph({ fur: 1 }))).toBeGreaterThan(extent(morph({ fur: 0.5 })));
  });

  it('the world sprite of a furred cold-blooded body draws the same pelt (the art library at sprite detail)', () => {
    const l = { ...lookOf(design({ fur: 0.5 }, { thermo: 'cold' })) };
    expect(l.cover).toBe('fur');
    const d = spriteDrawing({ look: l, juvenile: false }).drawing;
    // The soft clumped rim of fur on the body's silhouette, as on the avatar.
    expect(pieces(d, 'body')[0].points.length).toBeGreaterThan(32);
  });
});
