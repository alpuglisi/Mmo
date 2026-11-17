import { describe, expect, it } from 'vitest';
import { creaturePalette } from '../palette';
import { headTop, type HeadTopParams } from './headTop';

const pal = creaturePalette({ world: { base: 0xa47a52, dark: 0x6e4f33, light: 0xd2b48c, accent: 0x7d5b3c }, coloring: 'drab', vividness: 0.3, warm: true });
const base: HeadTopParams = {
  r: 30, snoutTip: 51, carn: 0.4, cold: false,
  ears: { count: 2, shape: 'round', size: 0 },
  eyes: [{ x: 26, y: -15, az: 60, r: 5 }, { x: -26, y: -15, az: -60, r: 5 }],
  furry: true, lod: 'portrait',
};
const tip = (p: HeadTopParams) => Math.min(...headTop(p, pal).head.pieces.find((q) => q.tag === 'snout')!.points.map(([, y]) => y));

describe('headTop', () => {
  it('reaches the snout tip it is given', () => {
    for (const t of [36, 51, 66]) expect(tip({ ...base, snoutTip: t })).toBeCloseTo(-t, 5);
  });

  it('draws the ears it is given, mirrored, sticking out of the skull', () => {
    for (const count of [0, 2, 4]) {
      const d = headTop({ ...base, ears: { count, shape: 'pointed', size: 0.5 } }, pal);
      expect(d.head.pieces.filter((q) => q.tag.startsWith('ear-')).length).toBe(count);
      expect(d.anchors.earTips.length).toBe(count);
      for (const [x, y] of d.anchors.earTips) expect(Math.hypot(x, y)).toBeGreaterThan(base.r);
    }
    // Bigger ears reach further.
    const reach = (size: number) => Math.hypot(...headTop({ ...base, ears: { count: 2, shape: 'fan', size } }, pal).anchors.earTips[0]);
    expect(reach(1)).toBeGreaterThan(reach(0));
  });

  it('puts one eye where each is asked, sized by its radius, with a glint', () => {
    const d = headTop({ ...base, eyes: [...base.eyes, { x: 0, y: -17, az: 0, r: 8 }] }, pal);
    expect(d.eyes.length).toBe(3);
    for (const shapes of d.eyes) expect(shapes.filter((s) => s.tag === 'glint').length).toBe(1);
    const socket = d.eyes[2][0];
    expect(socket.kind === 'ellipse' && socket.rx).toBeGreaterThan(8);
  });

  it('wears the covering it is given on the back', () => {
    const tags = (p: Partial<HeadTopParams>) => new Set(headTop({ ...base, furry: false, ...p }, pal).body.detail.map((d) => d.tag));
    expect(tags({ cover: 'scales' }).has('scale')).toBe(true);
    expect(tags({ cover: 'armor' }).has('plate')).toBe(true);
    expect(tags({ cover: 'feathers' }).has('feather')).toBe(true);
    expect(tags({ furry: true }).has('hair')).toBe(true);
    const plain = tags({});
    for (const t of ['scale', 'plate', 'feather', 'hair']) expect(plain.has(t)).toBe(false);
  });
});
