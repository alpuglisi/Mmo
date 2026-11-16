import { describe, expect, it } from 'vitest';
import { clampMorphology, type Morphology } from '../../morphology';
import { avatarDrawing, morphologyOf } from '../../world/avatar';
import { drawingBounds } from '../../art/render';
import { FANG_KNOB_DROP, SIDE, SIDE_DESIGN, sideTracks, sideView, toSide } from './sideView';

const m = (f: Partial<Morphology> = {}) => clampMorphology(f);
const anchorsOf = (f: Partial<Morphology>) => avatarDrawing(morphologyOf(SIDE_DESIGN, m(f))).anchors;
/** The handle's position for a value (as builder.ts `linear` places it). */
const knob = (t: { from: [number, number]; to: [number, number] }, v: number): [number, number] => [t.from[0] + (t.to[0] - t.from[0]) * v, t.from[1] + (t.to[1] - t.from[1]) * v];

describe('builder side view', () => {
  it('handles sit on the feature tips they size, at every value', () => {
    for (const v of [0.25, 0.5, 0.8]) {
      const plan = { ear_count: 2 as const, tail_present: true, snout_length: v, ear_size: v, leg_length: v, tail_length: v };
      const t = sideTracks(m(plan));
      const a = anchorsOf(plan);
      const near = (p: [number, number], q: [number, number], what: string) => expect(Math.hypot(p[0] - q[0], p[1] - q[1]), `${what} at ${v}`).toBeLessThan(1);
      near(knob(t.snout, v), toSide(a.snoutTip, a), 'snout');
      near(knob(t.ears!, v), toSide(a.earTip!, a), 'ears');
      near(knob(t.legs!, v), toSide(a.foot!, a), 'legs');
      near(knob(t.tail!, v), toSide(a.tailTip!, a), 'tail');
      const fa = anchorsOf({ fang_size: v });
      const ft = toSide(fa.fangTip, fa);
      near(knob(sideTracks(m({ fang_size: v })).fangs, v), [ft[0], ft[1] + FANG_KNOB_DROP], 'fangs');
    }
  });

  it('pins the torso: longer legs lower the ground, so the leg knob moves down', () => {
    const short = sideView(m({ leg_length: 0 }));
    const long = sideView(m({ leg_length: 1 }));
    expect(long.groundY).toBeGreaterThan(short.groundY + 20);
    const t = sideTracks(m());
    expect(t.legs!.to[1]).toBeGreaterThan(t.legs!.from[1] + 20);
    expect(toSide([0, anchorsOf({ leg_length: 0 }).body.cy], anchorsOf({ leg_length: 0 }))[1]).toBeCloseTo(SIDE.bodyY);
    expect(toSide([0, anchorsOf({ leg_length: 1 }).body.cy], anchorsOf({ leg_length: 1 }))[1]).toBeCloseTo(SIDE.bodyY);
  });

  it('features that are absent have no handle', () => {
    const t = sideTracks(m({ ear_count: 0, leg_count: 0, tail_present: false }));
    expect(t.ears).toBeNull();
    expect(t.legs).toBeNull();
    expect(t.tail).toBeNull();
  });

  it('the largest body plans fit the panel (viewBox -14 -18 318 212)', () => {
    const plans: Partial<Morphology>[] = [
      { leg_length: 1, ear_count: 4, ear_shape: 'long', ear_size: 1, tail_present: true, tail_shape: 'bushy', tail_length: 1, snout_length: 1, fang_size: 1 },
      { leg_length: 1, ear_shape: 'fan', ear_size: 1, tail_present: true, tail_shape: 'prehensile', tail_length: 1, leg_count: 8 },
      { leg_count: 0, tail_present: true, tail_shape: 'fin', tail_length: 1, ear_count: 0 },
    ];
    for (const p of plans) {
      const d = avatarDrawing(morphologyOf(SIDE_DESIGN, m(p)));
      const b = drawingBounds(d);
      const [x0, y0] = toSide([b.minX, b.minY], d.anchors);
      const [x1, y1] = toSide([b.maxX, b.maxY], d.anchors);
      const what = JSON.stringify(p);
      expect(x0, what).toBeGreaterThanOrEqual(-14);
      expect(x1, what).toBeLessThanOrEqual(304);
      expect(y0, what).toBeGreaterThanOrEqual(-18);
      expect(y1, what).toBeLessThanOrEqual(194);
      // Ids are prefixed, so the builder's creature cannot collide with the preview card's.
      expect(sideView(m(p)).markup('bbx')).not.toMatch(/id="(?!bbx-)/);
    }
  });
});
