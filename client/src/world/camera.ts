// Camera maths for the world view (pure, unit-tested).

/**
 * The camera centre along one axis, kept so the map fills the screen: no
 * empty space past its edge while there's map to show (following a group at
 * the edge leaves it off centre instead); a map smaller than the screen sits
 * centred. `padLo` / `padHi` (map units) let it go that far past the low /
 * high edge: room for what floats over the map there (the toolbar on top,
 * playtest 3: a herd at the north edge sat under it and could not be tapped).
 */
export function clampCam(c: number, half: number, size: number, padLo = 0, padHi = 0): number {
  if (!(half > 0)) return Math.min(size, Math.max(0, c));
  if (2 * half >= size + padLo + padHi) return (size + padHi - padLo) / 2;
  return Math.min(size - half + padHi, Math.max(half - padLo, c));
}

/**
 * One frame of Follow along one axis: the camera moves toward `target` only
 * while it lies more than `dead` (map units) from the centre, and then only
 * as far as the edge of that dead zone (eased by `k`, 0..1). A herd drifting
 * about the middle of the screen leaves the camera still, so what is under a
 * finger stays there (playtest 3: with Follow on, the camera recentred all the
 * time and 4 taps in 12 on a moving animal picked a neighbour).
 */
export function followAxis(cam: number, target: number, dead: number, k: number): number {
  const d = target - cam;
  if (Math.abs(d) <= dead) return cam;
  return cam + (d - Math.sign(d) * dead) * k;
}
