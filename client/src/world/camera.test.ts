import { describe, expect, it } from 'vitest';
import { clampCam, followAxis } from './camera';

describe('clampCam', () => {
  it('keeps the map filling the screen', () => {
    // 64-tile map, 20 tiles either side of the centre on screen.
    expect(clampCam(2, 20, 64)).toBe(20);
    expect(clampCam(63, 20, 64)).toBe(44);
    expect(clampCam(30, 20, 64)).toBe(30);
  });
  it('centres a map smaller than the screen', () => {
    expect(clampCam(5, 40, 64)).toBe(32);
  });
  it('goes past an edge by its padding (room for the toolbar)', () => {
    // 4 tiles of toolbar over the top edge: the herd at y 2 can come out from under it.
    expect(clampCam(2, 20, 64, 4, 0)).toBe(16);
    expect(clampCam(63, 20, 64, 4, 3)).toBe(47);
    expect(clampCam(30, 20, 64, 4, 3)).toBe(30);
    expect(clampCam(5, 40, 64, 4, 0)).toBe(30);
  });
  it('falls back to the map bounds before the screen has a size', () => {
    expect(clampCam(-3, 0, 64)).toBe(0);
    expect(clampCam(70, 0, 64)).toBe(64);
  });
});

describe('followAxis', () => {
  it('stays still while the target is in the dead zone, then brings it back to its edge', () => {
    expect(followAxis(10, 12, 3, 0.5)).toBe(10);
    expect(followAxis(10, 7.5, 3, 0.5)).toBe(10);
    expect(followAxis(10, 17, 3, 0.5)).toBe(12);
    expect(followAxis(10, 17, 3, 1)).toBe(14);
    expect(followAxis(10, 2, 3, 1)).toBe(5);
    expect(followAxis(10, 17, 0, 1)).toBe(17);
  });
});
