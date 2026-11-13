import { WILD, type PlayerId } from '../protocol';

/** Own organisms are always this yellow. */
export const OWN_COLOR = 0xffd33d;
export const WILD_COLOR = 0x9aa4ae;

/**
 * A stable color per player: hues spread by the golden angle, skipping the
 * yellow band reserved for the viewer's own organisms.
 */
export function ownerColor(owner: PlayerId): number {
  if (owner === WILD) return WILD_COLOR;
  let hue = (owner * 137.508 + 200) % 360;
  if (hue > 40 && hue < 70) hue = (hue + 40) % 360;
  return hslToRgb(hue, 0.7, 0.58);
}

export function hslToRgb(h: number, s: number, l: number): number {
  const c = (1 - Math.abs(2 * l - 1)) * s;
  const x = c * (1 - Math.abs(((h / 60) % 2) - 1));
  const m = l - c / 2;
  const [r, g, b] = h < 60 ? [c, x, 0] : h < 120 ? [x, c, 0] : h < 180 ? [0, c, x] : h < 240 ? [0, x, c] : h < 300 ? [x, 0, c] : [c, 0, x];
  const to = (v: number) => Math.round((v + m) * 255);
  return (to(r) << 16) | (to(g) << 8) | to(b);
}

export function cssColor(c: number): string {
  return `#${c.toString(16).padStart(6, '0')}`;
}
