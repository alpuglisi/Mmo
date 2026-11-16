import { describe, expect, it } from 'vitest';
import { analyseVision, arcContains, visionFields, visionOf } from './vision';

const v = (eye_count: number, eye_azimuth: number, eye_size = 0, snout_length?: number) => visionOf({ eye_count, eye_azimuth, eye_size, snout_length });

describe('vision geometry (contract v1)', () => {
  it('field width is 140° + 60° × size', () => {
    expect(visionFields({ eye_count: 1, eye_azimuth: 0, eye_size: 0 })[0].width).toBe(140);
    expect(visionFields({ eye_count: 1, eye_azimuth: 0, eye_size: 1 })[0].width).toBe(200);
  });

  // The spec's prototype table (docs/morphology.md, appendix).
  it.each([
    ['default: 2 eyes, 60°, size 0', v(2, 60), 260, 20, 0, 100],
    ['side eyes: 90°', v(2, 90), 280, 0, 40, 40],
    ['side eyes + midline eye', v(3, 90), 320, 80, 0, 40],
    ['grazer: 100°, size 0.5', v(2, 100, 0.5), 330, 0, 30, 0],
    ['stalker: 25°, size 0.6', v(2, 25, 0.6), 226, 120, 0, 134],
    ['stalker + rear pair', v(4, 25, 0.6), 326, 120, 0, 34],
    ['stalker, snout 1.0', v(2, 25, 0.6, 1), 226, 100, 0, 134],
    ['default, snout 1.0', v(2, 60, 0, 1), 260, 0, 0, 100],
  ])('%s', (_name, s, view, depth, ahead, behind) => {
    expect(s.coveredDeg).toBeCloseTo(view, 6);
    expect(s.overlapDeg).toBeCloseTo(depth, 6);
    expect(s.frontBlindDeg).toBeCloseTo(ahead, 6);
    expect(s.rearBlindDeg).toBeCloseTo(behind, 6);
  });

  it('one eye sits on the midline', () => {
    const s = v(1, 77); // the azimuth is ignored
    expect(s.coveredDeg).toBe(140);
    expect(s.coverage).toBeCloseTo(140 / 360, 6);
    expect(s.overlapDeg).toBe(0);
    expect(s.rearBlindDeg).toBe(220);
    expect(s.frontBlindDeg).toBe(0);
  });

  it('reports the arcs it counts', () => {
    const s = v(2, 60);
    expect(s.overlap).toEqual([{ start: -10, width: 20 }]);
    expect(s.rearBlind).toEqual({ start: 130, width: 100 });
    expect(s.covered).toEqual([{ start: -130, width: 260 }]);
    expect(v(2, 90).blind).toHaveLength(2);
  });

  it('two forward eyes: depth counts only ±60°', () => {
    const s = v(2, 0);
    expect(s.coveredDeg).toBe(140);
    expect(s.rawOverlapDeg).toBe(120);
    expect(s.rearBlindDeg).toBe(220);
  });

  it('snout occlusion: 40° per unit above 0.5, never below zero', () => {
    expect(v(2, 0, 0, 0.5).overlapDeg).toBe(120);
    expect(v(2, 0, 0, 0.75).overlapDeg).toBe(110);
    expect(v(2, 0, 0, 0).overlapDeg).toBe(120); // a short snout doesn't add
    const s = v(2, 60, 0, 1);
    expect(s.rawOverlapDeg).toBe(20);
    expect(s.occludedDeg).toBe(20);
    expect(s.overlapDeg).toBe(0);
  });

  it('a midline eye closes the front gap of side eyes', () => {
    const two = v(2, 90);
    const three = v(3, 90);
    expect(two.frontBlindDeg).toBe(40);
    expect(three.frontBlindDeg).toBe(0);
    expect(three.blindDeg).toBeLessThan(two.blindDeg);
  });

  it('three eyes with the pair far back see all round', () => {
    const s = v(3, 130, 0.5); // midline −85…85, pair 45…215
    expect(s.coverage).toBe(1);
    expect(s.blindDeg).toBe(0);
    expect(s.rearBlind).toBeNull();
    expect(s.frontBlind).toBeNull();
    expect(s.covered).toEqual([{ start: -180, width: 360 }]);
    expect(s.overlapDeg).toBe(30); // 45…60 on each side
  });

  it('four and five eyes: pair k at azimuth + 50°·k', () => {
    const four = v(4, 60); // pairs at 60° and 110°
    expect(four.coverage).toBe(1);
    expect(four.overlapDeg).toBe(60);
    const five = v(5, 0); // midline plus pairs at 0° and 50°
    expect(five.coveredDeg).toBe(240);
    expect(five.rearBlindDeg).toBe(120);
    expect(five.overlapDeg).toBe(120);
  });

  it('six eyes, and the 170° cap', () => {
    const s = v(6, 0); // pairs at 0°, 50°, 100°
    expect(s.coveredDeg).toBe(340);
    expect(s.rearBlindDeg).toBe(20);
    expect(s.frontBlindDeg).toBe(0);
    const back = v(6, 180, 1); // every pair capped at 170°
    expect(back.rearBlindDeg).toBe(0);
    expect(back.frontBlindDeg).toBe(140);
    expect(back.coveredDeg).toBe(220);
  });

  it('fields that wrap through 180°', () => {
    const s = v(2, 180); // eyes at ±170°
    expect(s.rearBlindDeg).toBe(0);
    expect(s.frontBlindDeg).toBe(200);
    expect(s.overlapDeg).toBe(0); // the overlap is behind, not in front
    expect(s.covered).toEqual([{ start: 100, width: 160 }]);
  });

  it('explicit fields: none, a full circle, asymmetric', () => {
    const none = analyseVision([]);
    expect(none.coverage).toBe(0);
    expect(none.rearBlindDeg).toBe(360);
    expect(analyseVision([{ azimuth: 10, width: 400 }]).coverage).toBe(1);
    const odd = analyseVision([{ azimuth: 45, width: 90 }]);
    expect(odd.coveredDeg).toBe(90);
    expect(odd.frontBlindDeg).toBe(0); // 0° is the field's edge: the blind arc doesn't contain it
    expect(odd.rearBlindDeg).toBe(270);
  });

  it('stays consistent and mirror-symmetric for every count', () => {
    for (let n = 1; n <= 6; n++) {
      for (let az = 0; az <= 180; az += 15) {
        for (const size of [0, 0.5, 1]) {
          const s = v(n, az, size);
          expect(s.coveredDeg + s.blindDeg).toBeCloseTo(360, 6);
          expect(s.rearBlindDeg + s.frontBlindDeg).toBeLessThanOrEqual(s.blindDeg + 1e-6);
          expect(s.overlapDeg).toBeGreaterThanOrEqual(0);
          expect(s.overlapDeg).toBeLessThanOrEqual(120);
          for (const a of s.blind) {
            const mirror = -(a.start + a.width);
            expect(s.blind.some((b) => Math.abs(((b.start - mirror + 540) % 360) - 180) < 1e-6 && Math.abs(b.width - a.width) < 1e-6)).toBe(true);
          }
        }
      }
    }
  });

  it('arcContains handles wrap-around', () => {
    expect(arcContains({ start: 170, width: 20 }, -175)).toBe(true);
    expect(arcContains({ start: 170, width: 20 }, 0)).toBe(false);
    expect(arcContains({ start: -180, width: 360 }, 42)).toBe(true);
  });
});
