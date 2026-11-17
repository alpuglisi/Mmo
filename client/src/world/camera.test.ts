import { describe, expect, it } from 'vitest';
import { clampCam } from './camera';

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
  it('falls back to the map bounds before the screen has a size', () => {
    expect(clampCam(-3, 0, 64)).toBe(0);
    expect(clampCam(70, 0, 64)).toBe(64);
  });
});
