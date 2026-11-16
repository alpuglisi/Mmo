import { describe, expect, it } from 'vitest';
import type { VisionView } from '../protocol';
import {
  coneLayout, dir, dotGrid, easeAngle, facingOf, hatchSegments, headingDeg, visionLines, visionStats, wedgeContains, wedgePolygon, type Wedge,
} from './visionCones';

/** The contract's default eyes: two at ±60°, field 140° (eye size 0). */
const DEFAULT: VisionView = { eyes: 2, azimuth: 60, field: 140, coverage: 72, binocular: 20, blind: 100 };
/** Side eyes leave a gap straight ahead. */
const SIDE: VisionView = { eyes: 2, azimuth: 90, field: 140, coverage: 78, binocular: 0, blind: 40, front_gap: 40 };

const near = (a: number, b: number, eps = 1e-6) => Math.abs(a - b) <= eps;
const mid = (w: Wedge) => (w.start + w.width / 2) % 360;

describe('vision cones', () => {
  it('maps a quantized heading to screen degrees (east 0, south 90)', () => {
    expect([0, 8, 16, 24, 31, 32].map(headingDeg)).toEqual([0, 90, 180, 270, 348.75, 0]);
  });

  it('agrees with the server numbers for a descriptor', () => {
    const s = visionStats(DEFAULT);
    expect(s.coveredDeg).toBeCloseTo(260, 6);
    expect(s.overlapDeg).toBeCloseTo(DEFAULT.binocular, 6);
    expect(s.rearBlindDeg).toBeCloseTo(DEFAULT.blind, 6);
    const side = visionStats(SIDE);
    expect(side.frontBlindDeg).toBeCloseTo(40, 6);
    expect(side.rearBlindDeg).toBeCloseTo(40, 6);
    // A long snout hides depth.
    expect(visionStats({ ...DEFAULT, occlusion: 10 }).overlapDeg).toBeCloseTo(10, 6);
  });

  it('turns the cones with the heading: depth ahead, the blind spot behind', () => {
    for (const h of [0, 90, 200, 315]) {
      const l = coneLayout(DEFAULT, h);
      expect(l.heading).toBe(h);
      expect(l.overlap).toHaveLength(1);
      expect(near(mid(l.overlap[0]), h)).toBe(true);
      const rear = l.blind.find((b) => b.kind === 'rear')!;
      expect(rear.width).toBeCloseTo(100, 6);
      expect(near(mid(rear), (h + 180) % 360)).toBe(true);
      expect(l.eyes).toHaveLength(2);
      // The right eye looks to the animal's right: clockwise of the heading on screen.
      const centres = l.eyes.map((e) => mid(e)).sort((a, b) => a - b);
      expect(centres.some((c) => near(c, (h + 60) % 360))).toBe(true);
      expect(centres.some((c) => near(c, (h + 300) % 360))).toBe(true);
    }
    // Facing east, the right eye covers south (screen y down) and not north.
    const east = coneLayout(DEFAULT, 0);
    expect(east.covered.some((w) => wedgeContains(w, 100))).toBe(true);
    expect(east.blind.some((w) => wedgeContains(w, 180))).toBe(true);
  });

  it('marks the gap ahead of side eyes and the snout occlusion', () => {
    const l = coneLayout(SIDE, 90);
    const kinds = l.blind.map((b) => b.kind).sort();
    expect(kinds).toEqual(['front', 'rear']);
    expect(l.overlap).toHaveLength(0);
    expect(near(mid(l.blind.find((b) => b.kind === 'front')!), 90)).toBe(true);
    const snouty = coneLayout({ ...DEFAULT, binocular: 10, occlusion: 10 }, 45);
    expect(snouty.occluded).toEqual({ start: 40, width: 10 });
    expect(coneLayout(DEFAULT, 45).occluded).toBeNull();
  });

  it('builds wedge polygons from the center along the rim', () => {
    const p = wedgePolygon(10, 20, 50, { start: 0, width: 90 }, 30);
    expect(p.slice(0, 2)).toEqual([10, 20]);
    expect(p.length).toBe(2 + 2 * 4);
    expect(p[2]).toBeCloseTo(60);
    expect(p[3]).toBeCloseTo(20);
    expect(p[p.length - 2]).toBeCloseTo(10);
    expect(p[p.length - 1]).toBeCloseTo(70);
    // A full circle has no center point.
    expect(wedgePolygon(0, 0, 1, { start: 0, width: 360 }, 90).length).toBe(8);
  });

  it('clips hatch lines to the wedge, at 45 degrees, and only inside it', () => {
    const w: Wedge = { start: 120, width: 120 };
    const segs = hatchSegments(0, 0, 60, w, 6);
    expect(segs.length).toBeGreaterThan(8);
    for (const [x1, y1, x2, y2] of segs) {
      expect(near(Math.abs(Math.atan2(y2 - y1, x2 - x1) * 180 / Math.PI), 45, 1e-6) || near(Math.abs(Math.atan2(y2 - y1, x2 - x1) * 180 / Math.PI), 135, 1e-6)).toBe(true);
      for (const t of [0.01, 0.5, 0.99]) {
        const x = x1 + (x2 - x1) * t, y = y1 + (y2 - y1) * t;
        expect(Math.hypot(x, y)).toBeLessThanOrEqual(60 + 1e-6);
        expect(wedgeContains(w, Math.atan2(y, x) * 180 / Math.PI)).toBe(true);
      }
    }
    // The pattern rides with the animal: moving the center moves it rigidly.
    const moved = hatchSegments(100, 50, 60, w, 6);
    expect(moved.length).toBe(segs.length);
    expect(moved[0][0] - segs[0][0]).toBeCloseTo(100);
    expect(moved[0][1] - segs[0][1]).toBeCloseTo(50);
    // Nothing for an empty wedge.
    expect(hatchSegments(0, 0, 60, { start: 0, width: 0 })).toEqual([]);
  });

  it('places depth dots inside the wedge only', () => {
    const w: Wedge = { start: -20, width: 40 };
    const dots = dotGrid(0, 0, 50, w, 5, 5);
    expect(dots.length).toBeGreaterThan(5);
    for (const [x, y] of dots) {
      expect(Math.hypot(x, y)).toBeLessThanOrEqual(50);
      expect(Math.hypot(x, y)).toBeGreaterThanOrEqual(5);
      expect(wedgeContains(w, Math.atan2(y, x) * 180 / Math.PI)).toBe(true);
    }
  });

  it('faces the sprite by the heading and keeps it when nearly vertical', () => {
    expect(facingOf(headingDeg(0), -1)).toBe(1);
    expect(facingOf(headingDeg(16), 1)).toBe(-1);
    expect(facingOf(headingDeg(8), -1)).toBe(-1);
    expect(facingOf(headingDeg(24), 1)).toBe(1);
  });

  it('eases the drawn heading the short way round', () => {
    expect(easeAngle(350, 10, 0.5)).toBeCloseTo(0);
    expect(easeAngle(10, 350, 1)).toBeCloseTo(350);
    expect(easeAngle(90, 270, 0)).toBe(90);
    expect(dir(90).y).toBeCloseTo(1);
  });

  it('labels the panel by whether directional vision is on', () => {
    expect(visionLines(DEFAULT).mechanic).toMatch(/off/);
    expect(visionLines({ ...DEFAULT, directional: true }).mechanic).toMatch(/is on/);
    expect(visionLines(SIDE).ahead).toBe('40°');
    expect(visionLines(DEFAULT)).toMatchObject({ view: '72% (259°)', depth: '20°', blind: '100°', ahead: null });
  });
});
