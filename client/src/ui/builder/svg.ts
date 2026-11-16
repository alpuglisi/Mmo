// SVG helpers and the feature shapes the builder draws (side view and the
// picker icons share them).

import type { EarShape, TailShape } from '../../morphology';

export const SVG_NS = 'http://www.w3.org/2000/svg';

type SvgAttrs = Record<string, string | number | undefined | null>;

export function s<K extends keyof SVGElementTagNameMap>(tag: K, attrs: SvgAttrs = {}, ...children: (SVGElement | string | null)[]): SVGElementTagNameMap[K] {
  const el = document.createElementNS(SVG_NS, tag);
  for (const [k, v] of Object.entries(attrs)) if (v !== undefined && v !== null) el.setAttribute(k, String(v));
  for (const c of children) {
    if (c === null) continue;
    el.appendChild(typeof c === 'string' ? document.createTextNode(c) : c);
  }
  return el;
}

const f = (n: number): string => (Math.round(n * 100) / 100).toString();

/** A point at azimuth `a` (degrees, 0 = up, clockwise) and radius `r` around the origin. */
export function polar(a: number, r: number): [number, number] {
  const t = (a * Math.PI) / 180;
  return [r * Math.sin(t), -r * Math.cos(t)];
}

/** Azimuth (degrees, −180…180, 0 = up, clockwise) of a point seen from the origin. */
export function azimuthOf(x: number, y: number): number {
  return (Math.atan2(x, -y) * 180) / Math.PI;
}

/** A pie wedge from the origin: clockwise from `start` for `width` degrees, radius `r`. */
export function wedgePath(start: number, width: number, r: number): string {
  if (width >= 359.999) return `M${f(-r)},0 A${f(r)},${f(r)} 0 1 1 ${f(r)},0 A${f(r)},${f(r)} 0 1 1 ${f(-r)},0 Z`;
  const [x0, y0] = polar(start, r);
  const [x1, y1] = polar(start + width, r);
  return `M0,0 L${f(x0)},${f(y0)} A${f(r)},${f(r)} 0 ${width > 180 ? 1 : 0} 1 ${f(x1)},${f(y1)} Z`;
}

/** An ear standing on (0,0), pointing up (−y), `h` tall. */
export function earPath(shape: EarShape, h: number): string {
  switch (shape) {
    case 'pointed':
      return `M-6,0 L0,${f(-h)} L6,0 Z`;
    case 'long': {
      const t = h * 1.3;
      return `M-4,0 C-6,${f(-t * 0.7)} -3,${f(-t)} 0,${f(-t)} C3,${f(-t)} 6,${f(-t * 0.7)} 4,0 Z`;
    }
    case 'fan':
      return `M-3,0 C-16,${f(-h * 0.5)} -12,${f(-h)} 0,${f(-h * 0.85)} C12,${f(-h)} 16,${f(-h * 0.5)} 3,0 Z`;
    case 'round':
    default:
      return `M-7,0 C-8,${f(-h * 1.25)} 8,${f(-h * 1.25)} 7,0 Z`;
  }
}

/** How far the ear's tip sits above its base, for an ear `h` tall. */
export function earReach(shape: EarShape, h: number): number {
  return shape === 'long' ? h * 1.3 : shape === 'round' ? h * 0.94 : shape === 'fan' ? h * 0.9 : h;
}

/**
 * A tail from (x0,y0) to the tip (x1,y1): its elements, in the current
 * fill / stroke colours (`currentColor`).
 */
export function tailShapes(shape: TailShape, x0: number, y0: number, x1: number, y1: number): SVGElement[] {
  const dx = x1 - x0;
  const dy = y1 - y0;
  const len = Math.hypot(dx, dy) || 1;
  // A gentle droop: the control point sits below the straight line.
  const cx = x0 + dx * 0.5;
  const cy = y0 + dy * 0.5 + len * 0.18;
  const curve = `M${f(x0)},${f(y0)} Q${f(cx)},${f(cy)} ${f(x1)},${f(y1)}`;
  const stroke = (w: number) => s('path', { d: curve, fill: 'none', stroke: 'currentColor', 'stroke-width': w, 'stroke-linecap': 'round' });
  const ux = dx / len;
  const uy = dy / len;
  switch (shape) {
    case 'bushy': {
      // A plume: thick along the curve, widest near the tip.
      const nx = -uy;
      const ny = ux;
      const w = Math.max(4, len * 0.22);
      const mx = cx;
      const my = cy;
      const d = `M${f(x0)},${f(y0)} Q${f(mx + nx * w)},${f(my + ny * w)} ${f(x1)},${f(y1)} Q${f(mx - nx * w * 1.4)},${f(my - ny * w * 1.4)} ${f(x0)},${f(y0)} Z`;
      return [s('path', { d, fill: 'currentColor', stroke: 'currentColor', 'stroke-width': 2, 'stroke-linejoin': 'round' })];
    }
    case 'fin': {
      // A fish's caudal fin: two lobes at the tip, across the tail.
      const nx = -uy;
      const ny = ux;
      const w = Math.max(6, len * 0.35);
      const back = Math.max(4, len * 0.25);
      const bx = x1 + ux * back;
      const by = y1 + uy * back;
      const d = `M${f(x1)},${f(y1)} L${f(bx + nx * w)},${f(by + ny * w)} Q${f(x1 + ux * back * 0.4)},${f(y1 + uy * back * 0.4)} ${f(bx - nx * w)},${f(by - ny * w)} Z`;
      return [stroke(4), s('path', { d, fill: 'currentColor', stroke: 'currentColor', 'stroke-width': 1.5, 'stroke-linejoin': 'round' })];
    }
    case 'club':
      return [stroke(4), s('circle', { cx: f(x1), cy: f(y1), r: f(Math.max(4, len * 0.12)), fill: 'currentColor' })];
    case 'prehensile': {
      // Curls up at the end.
      const r = Math.max(3, len * 0.14);
      const d = `M${f(x0)},${f(y0)} Q${f(cx)},${f(cy)} ${f(x1)},${f(y1)} a${f(r)},${f(r)} 0 1 1 ${f(r * 1.6)},${f(-r * 1.2)}`;
      return [s('path', { d, fill: 'none', stroke: 'currentColor', 'stroke-width': 3, 'stroke-linecap': 'round' })];
    }
    case 'thin':
    default:
      return [stroke(2.5)];
  }
}

/** A small picker icon for an ear shape. */
export function earIcon(shape: EarShape): SVGSVGElement {
  return s('svg', { viewBox: '-14 -22 28 26', width: 28, height: 26, 'aria-hidden': 'true', class: 'bb-icon' },
    s('circle', { cx: 0, cy: 6, r: 7, fill: 'currentColor', opacity: 0.35 }),
    s('path', { d: earPath(shape, shape === 'long' ? 15 : 18), fill: 'currentColor' }));
}

/** A small picker icon for a tail shape (or no tail). */
export function tailIcon(shape: TailShape | null): SVGSVGElement {
  const svg = s('svg', { viewBox: '0 0 36 26', width: 36, height: 26, 'aria-hidden': 'true', class: 'bb-icon' },
    s('ellipse', { cx: 30, cy: 11, rx: 8, ry: 6, fill: 'currentColor', opacity: 0.35 }));
  if (shape === null) {
    svg.appendChild(s('path', { d: 'M6,6 L18,18 M18,6 L6,18', stroke: 'currentColor', 'stroke-width': 2.5, 'stroke-linecap': 'round' }));
  } else {
    for (const el of tailShapes(shape, 23, 11, 7, 12)) svg.appendChild(el);
  }
  return svg;
}
