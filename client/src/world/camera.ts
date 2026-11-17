// Camera maths for the world view (pure, unit-tested).

/**
 * The camera centre along one axis, kept so the map fills the screen: no
 * empty space past its edge while there's map to show (following a group at
 * the edge leaves it off centre instead); a map smaller than the screen sits
 * centred.
 */
export function clampCam(c: number, half: number, size: number): number {
  if (!(half > 0)) return Math.min(size, Math.max(0, c));
  if (2 * half >= size) return size / 2;
  return Math.min(size - half, Math.max(half, c));
}
