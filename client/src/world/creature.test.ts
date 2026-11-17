import { createHash } from 'node:crypto';
import { describe, expect, it } from 'vitest';
import type { Appearance, NearbyOrganism, OwnOrganism } from '../protocol';
import { actionHint, bodyRadius, describeLook, creatureKey, DEFAULT_LOOK, hintShapes, JUVENILE_SCALE, palette, specOf, spriteExtent, type CreatureSpec } from './creature';
import { FAR_PARTS, FAR_SHIFT_PX, shiftFar, SPRITE_OUTLINE_PX, spriteModel } from './spriteArt';
import { avatarDrawing, drawingShapes, paintOf } from './avatar';
import { EXHAUSTIVE_MS, look, adult, allLooks, draw, pieces, tagged, differs, pixelBounds } from './creature.testkit';

describe('appearance classes', () => {
  it('gives every distinct appearance and life stage its own cache key', () => {
    const keys = new Set<string>();
    const looks = allLooks();
    for (const l of looks) for (const juvenile of [false, true]) keys.add(creatureKey({ look: l, juvenile }));
    expect(keys.size).toBe(looks.length * 2);
  });

  it('draws every class with the art library: the avatar of its sprite model, at sprite detail', () => {
    for (const l of allLooks()) {
      for (const juvenile of [false, true]) {
        const s = { look: l, juvenile };
        const sd = draw(s);
        const lib = avatarDrawing(spriteModel(s), { lod: 'sprite' });
        // Swimmers alone grow a dorsal fin into the main silhouette.
        const fins = sd.drawing.groups.flatMap((g) => g.pieces.filter((p) => p.tag === 'fin'));
        expect(fins.length, creatureKey(s)).toBe(l.locomotion === 'swim' ? 1 : 0);
        const strip = (d: typeof lib) => d.groups.map((g) => ({ ...g, pieces: g.pieces.filter((p) => p.tag !== 'fin'), detail: g.detail.filter((x) => x.tag !== 'fin') }));
        // The far-side groups sit FAR_SHIFT_PX forward and up (clear of the near parts' outline); the rest is the library's.
        shiftFar(lib, FAR_SHIFT_PX.x / sd.k, FAR_SHIFT_PX.y / sd.k);
        expect(JSON.stringify(strip(sd.drawing)) === JSON.stringify(strip(lib)), creatureKey(s)).toBe(true);
        for (const g of sd.drawing.groups) if (FAR_PARTS.has(g.part)) expect(g.tones, creatureKey(s)).toEqual(paintOf(sd.model).far);
        // Sprite detail: no fine textures (scales come as a coarse pattern).
        const tags = new Set(drawingShapes(sd.drawing).map((x) => x.tag));
        for (const t of ['hair', 'feather', 'crease', 'whisker', 'scute']) expect(tags.has(t), `${creatureKey(s)} ${t}`).toBe(false);
      }
    }
  }, EXHAUSTIVE_MS);

  it('sets the far-side legs, ears and wing back in the far ramp, a pixel forward and up, clear of the near outline', () => {
    for (const juvenile of [false, true]) for (const size of [0, 3] as const) {
      const s: CreatureSpec = { look: look({ size, locomotion: 'fly', cover: 'skin' }), juvenile };
      const sd = draw(s);
      const lib = avatarDrawing(spriteModel(s), { lod: 'sprite' });
      const pal = paintOf(sd.model);
      const parts = new Set<string>();
      sd.drawing.groups.forEach((g, i) => {
        const L = lib.groups[i];
        expect(g.part).toBe(L.part);
        const [a, b] = [g.pieces[0].points[0], L.pieces[0].points[0]];
        if (FAR_PARTS.has(g.part)) {
          parts.add(g.part);
          expect(g.tones).toEqual(pal.far);
          // In screen px at marker scale 1: about the outline width forward, half that up, whatever the size.
          expect((a[0] - b[0]) * sd.k).toBeCloseTo(FAR_SHIFT_PX.x, 6);
          expect((a[1] - b[1]) * sd.k).toBeCloseTo(FAR_SHIFT_PX.y, 6);
          expect(FAR_SHIFT_PX.x).toBeGreaterThanOrEqual(SPRITE_OUTLINE_PX * 0.75);
        } else {
          expect(a).toEqual(b);
        }
      });
      expect([...parts].sort()).toEqual(['ears-far', 'legs-far', 'wing-far']);
      // The library's own points are left alone (they may be its anchors).
      const before = JSON.stringify(lib);
      shiftFar({ ...lib }, 5, -5);
      expect(JSON.stringify(lib)).toBe(before);
    }
  });

  it('maps every class and stage to the same sprite model (golden)', () => {
    // sha256 of the sprite models of all 1,536 classes × stage: what each
    // look means to the art library (sizes, coverings, feet, wings, horns,
    // colours). Changing it changes every map creature: update it only on
    // purpose, after looking at the e2e sprite galleries.
    const h = createHash('sha256');
    const replacer = (_k: string, v: unknown) => (typeof v === 'number' ? Math.round(v * 1e6) / 1e6 : v);
    for (const l of allLooks()) for (const juvenile of [false, true]) h.update(JSON.stringify(spriteModel({ look: l, juvenile }), replacer));
    expect(h.digest('hex')).toBe('e9b6c9e75ba55ed88ca07d0967ec3c91e1a02b84462f39814823d88bbdcfe719');
    // The default founder (a server without morphology), in full.
    const { look: _look, ...m } = spriteModel(adult());
    expect(m).toMatchInlineSnapshot(`
      {
        "body": {
          "fat": 0.35,
          "muscle": 0.2,
        },
        "carnivory": 0.15,
        "coloring": {
          "camo": 0,
          "display": 0,
          "kind": "drab",
          "palette": {
            "accent": 8215356,
            "base": 10779218,
            "dark": 7229235,
            "light": 13808780,
          },
          "toxins": 0,
        },
        "cover": {
          "armor": {
            "amount": 0,
            "role": "none",
          },
          "dominant": "fur",
          "fur": {
            "amount": 0.8,
            "role": "dominant",
          },
          "scales": {
            "amount": 0,
            "role": "none",
          },
        },
        "ear_count": 2,
        "ear_shape": "round",
        "ear_size": 0.5,
        "extras": {
          "birth": "live",
          "seasonalCoat": false,
          "swim": 0,
        },
        "eye_azimuth": 60,
        "eye_count": 2,
        "eye_size": 0.6,
        "fang_size": 0,
        "feet": {
          "claws": 0,
          "climbing": false,
          "digging": false,
          "kind": "paw",
          "webbed": false,
          "width": 0.4,
        },
        "horn_form": "curved",
        "horn_size": 0,
        "horns": 0,
        "leg_count": 4,
        "leg_length": 0.35,
        "mass": 4,
        "nose": 0.3,
        "pattern": "plain",
        "size": 0.3,
        "snout_length": 0.5,
        "tail_length": 0.5,
        "tail_present": true,
        "tail_shape": "thin",
        "warm": true,
        "wing_aspect": 0.5,
        "wings": {
          "amount": 0,
          "stage": "none",
        },
      }
    `);
  });

  it('reads the look from an organism, with a default for older servers and juveniles from `matured`', () => {
    const own = { id: 1, matured: false, look: look({ diet: 'carnivore' }) } as unknown as OwnOrganism;
    expect(specOf(own)).toEqual({ look: look({ diet: 'carnivore' }), juvenile: true });
    const old = { id: 2, owner: 0, matured: true } as unknown as NearbyOrganism;
    expect(specOf(old)).toEqual({ look: DEFAULT_LOOK, juvenile: false });
  });

  it('sizes bodies by size class and draws juveniles smaller, with bigger heads', () => {
    const r = [0, 1, 2, 3].map((size) => bodyRadius(adult({ size: size as Appearance['size'] })));
    for (let i = 1; i < r.length; i++) expect(r[i]).toBeGreaterThan(r[i - 1]);
    const j = bodyRadius({ look: look({ size: 2 }), juvenile: true });
    expect(j).toBeCloseTo(r[2] * JUVENILE_SCALE);
    // On screen: wider sprites for bigger classes, juveniles narrower than adults.
    const width = (s: CreatureSpec) => {
      const b = pixelBounds(draw(s));
      return b.maxX - b.minX;
    };
    for (let i = 1; i < 4; i++) expect(width(adult({ size: i as Appearance['size'] }))).toBeGreaterThan(width(adult({ size: (i - 1) as Appearance['size'] })));
    expect(width({ look: look({ size: 2 }), juvenile: true })).toBeLessThan(width(adult({ size: 2 })));
    // The head, relative to the body: bigger on the juvenile.
    const headToBody = (s: CreatureSpec) => {
      const d = draw(s).drawing;
      const head = d.groups.flatMap((g) => g.pieces).find((p) => p.tag === 'head')!;
      const xs = head.points.map(([x]) => x);
      return (Math.max(...xs) - Math.min(...xs)) / d.anchors.body.rx;
    };
    expect(headToBody({ look: look({ size: 2 }), juvenile: true })).toBeGreaterThan(headToBody(adult({ size: 2 })));
  });

  it('keeps every drawing inside its texture, with its feet on the owner ring', () => {
    for (const l of allLooks()) {
      for (const juvenile of [false, true]) {
        const s = { look: l, juvenile };
        const sd = draw(s);
        const b = pixelBounds(sd);
        expect(Math.max(-b.minX, b.maxX), creatureKey(s)).toBeLessThanOrEqual(sd.half.w);
        expect(Math.max(-b.minY, b.maxY), creatureKey(s)).toBeLessThanOrEqual(sd.half.h);
        // The ground line (the feet) on the owner ring, under the marker centre.
        expect(sd.ground).toBeCloseTo(bodyRadius(s) * 0.95);
        // Wings and long tails grow the frame a little at most.
        expect(Math.max(sd.half.w, sd.half.h), creatureKey(s)).toBeLessThanOrEqual(spriteExtent(s) * 1.6);
      }
    }
  }, EXHAUSTIVE_MS);

  it('draws visibly different body plans', () => {
    expect(differs({ diet: 'carnivore' }, {})).toBe(true);
    expect(differs({ warm: false }, {})).toBe(true);
    expect(differs({ locomotion: 'swim' }, {})).toBe(true);
    expect(differs({ locomotion: 'fly' }, {})).toBe(true);
    expect(differs({ cover: 'scales' }, {})).toBe(true);
    expect(differs({ coloring: 'warning' }, {})).toBe(true);
    // Fliers reach well above the body (the raised wings); swimmers have no legs but a fin.
    expect(pixelBounds(draw(adult({ locomotion: 'fly', cover: 'skin' }))).minY).toBeLessThan(pixelBounds(draw(adult({ cover: 'skin' }))).minY - 3);
    expect(pieces(adult(), 'leg')).toHaveLength(4);
    expect(pieces(adult({ locomotion: 'swim' }), 'leg')).toHaveLength(0);
    expect(pieces(adult({ locomotion: 'swim' }), 'fin')).toHaveLength(1);
    // Coverings and colourings as the art library draws them; feathers on furred fliers.
    expect(tagged(adult({ cover: 'armor' }), 'plate').length).toBeGreaterThan(0);
    expect(tagged(adult({ coloring: 'warning' }), 'band').length).toBeGreaterThan(0);
    expect(tagged(adult({ coloring: 'camo' }), 'camo').length).toBeGreaterThan(0);
    expect(pieces(adult({ coloring: 'bright' }), 'frill')).toHaveLength(1);
    expect(pieces(adult({ locomotion: 'fly' }), 'crest')).toHaveLength(1);
    expect(pieces(adult({ locomotion: 'glide' }), 'flaps')).toHaveLength(1);
    // Horns come from the look (a body-plan locus), on adults; no more horns for every large herbivore.
    expect(tagged(adult({ size: 3 }), 'horn')).toHaveLength(0);
    expect(tagged(adult({ horns: 2 }), 'horn').length).toBeGreaterThan(0);
    expect(tagged({ look: look({ horns: 2 }), juvenile: true }, 'horn')).toHaveLength(0);
    expect(differs({ horns: 2, horn_form: 'branched' }, { horns: 2 })).toBe(true);
    expect(differs({ horns: 2, horn_form: 'straight' }, { horns: 2 })).toBe(true);
    // Coat patterns draw markings; wing shape changes a flier's outline.
    for (const pattern of ['spots', 'stripes', 'mottled'] as const) expect(tagged(adult({ pattern }), 'pattern').length, pattern).toBeGreaterThan(0);
    expect(tagged(adult(), 'pattern')).toHaveLength(0);
    expect(differs({ locomotion: 'fly', wing_aspect: 2 }, { locomotion: 'fly' })).toBe(true);
  });

  it('colors by coloring class', () => {
    expect(palette(look({ coloring: 'warning' })).accent).toBe(0x1b1b1b);
    expect(palette(look({ coloring: 'bright' })).base).not.toBe(palette(look({ coloring: 'drab' })).base);
    expect(palette(look({ warm: false })).base).not.toBe(palette(look({ warm: true })).base);
    // The sprite's body tone follows the class palette.
    const body = (s: CreatureSpec) => draw(s).drawing.groups.find((g) => g.part === 'body')!.tones.base;
    expect(body(adult({ coloring: 'warning' }))).not.toBe(body(adult()));
  });

  it('draws one outline width on screen at every size, bolder than the portrait relative to the creature', () => {
    for (const size of [0, 1, 2, 3] as const) {
      const sd = draw(adult({ size }));
      expect(sd.outline * sd.k).toBeCloseTo(SPRITE_OUTLINE_PX);
      // The portrait's 1.9 px outline on a ~250 px creature; here over a 1.15 px on a 15-40 px one.
      const b = pixelBounds(sd);
      expect(sd.outline * sd.k / (b.maxX - b.minX)).toBeGreaterThan(1.9 / 250);
    }
  });
});

describe('describeLook', () => {
  it('puts the body plan in words', () => {
    expect(describeLook(look({ size: 3, diet: 'carnivore', warm: false, cover: 'scales', coloring: 'camo', locomotion: 'swim' })))
      .toBe('large cold-blooded carnivore · scales · camouflaged · swims');
  });
});

describe('action hints', () => {
  it('maps actions to icons', () => {
    expect(actionHint('rest')).toBe('sleep');
    expect(actionHint('dormant')).toBe('sleep');
    expect(actionHint('flee')).toBe('flee');
    expect(actionHint('hunt')).toBe('hunt');
    expect(actionHint('seek_mate')).toBe('mate');
    expect(actionHint('graze')).toBeNull();
    expect(actionHint('something_new')).toBeNull();
  });

  it('has shapes for every icon', () => {
    for (const h of ['sleep', 'flee', 'hunt', 'mate'] as const) expect(hintShapes(h).length).toBeGreaterThan(0);
  });
});
