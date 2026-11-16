// The renderer: turns a `Drawing` (groups of silhouette pieces plus detail
// shapes) into SVG markup in the house style (STYLE.md, "Rendering").
//
// Per group, back to front:
//   1. outline  – every piece filled and stroked in ink at twice the outline
//                 width; the fills drawn next cover the inner half, so one
//                 even outline runs round the merged silhouette and there are
//                 no seams where pieces overlap (body, neck, head, legs read
//                 as one animal, not stacked shapes);
//   2. base     – clipped to the silhouette: the base tone, then the details
//                 (markings, materials, covering textures);
//   3. glazes   – still clipped: the shade tone as a translucent glaze over
//                 the bottom-right rim (the silhouette minus itself shifted
//                 up-left) and the highlight tone over the top-left rim (minus
//                 itself shifted down-right). Glazing after the details means
//                 every marking and material is lit by the same light: from
//                 the top left, one highlight and one shadow step, never more;
//   4. overlays – eyes, teeth, claws, plates: unclipped, on top.

import type { Paint, Part, Piece, Point, Shape } from './geom';
import type { Color, Tones } from './palette';

export interface Group {
  part: Part;
  tones: Tones;
  pieces: Piece[];
  /** Clipped to the merged silhouette. */
  detail: Shape[];
  /** On top, not clipped. */
  over: Shape[];
  /** Idle sway: rotation (degrees) about `pivot`. */
  sway?: { pivot: Point; deg: number; dur: number };
}

export interface Drawing {
  groups: Group[];
  /** Soft ground shadow (drawn first, outside the creature). */
  shadow: Shape | null;
  ink: Color;
}

export interface RenderOptions {
  /** Unique id prefix (clip paths): two drawings on one page need different ones. */
  id: string;
  /** Outline width in drawing units (the caller divides a px width by its scale). */
  outline: number;
  /** Highlight rim and shade rim widths in drawing units. */
  rim: { light: number; shade: number };
  animate: boolean;
}

/** Glaze strengths: how far the rims move from the base tone toward the shade / highlight tone. */
export const GLAZE = { shade: 0.78, light: 0.6 } as const;

export const fmt = (v: number): string => (Math.round(v * 10) / 10).toString();

/** A smooth closed (or open) path through the points: Catmull-Rom as cubic Béziers. */
export function smoothPath(points: Point[], closed: boolean): string {
  const n = points.length;
  if (n < 3) return `M${points.map(([x, y]) => `${fmt(x)} ${fmt(y)}`).join('L')}`;
  const at = (i: number): Point => (closed ? points[(i + n) % n] : points[Math.max(0, Math.min(n - 1, i))]);
  let d = `M${fmt(points[0][0])} ${fmt(points[0][1])}`;
  const last = closed ? n : n - 1;
  for (let i = 0; i < last; i++) {
    const p0 = at(i - 1), p1 = at(i), p2 = at(i + 1), p3 = at(i + 2);
    const c1: Point = [p1[0] + (p2[0] - p0[0]) / 6, p1[1] + (p2[1] - p0[1]) / 6];
    const c2: Point = [p2[0] - (p3[0] - p1[0]) / 6, p2[1] - (p3[1] - p1[1]) / 6];
    d += `C${fmt(c1[0])} ${fmt(c1[1])} ${fmt(c2[0])} ${fmt(c2[1])} ${fmt(p2[0])} ${fmt(p2[1])}`;
  }
  return closed ? `${d}Z` : d;
}

export function polyPath(points: Point[], closed: boolean): string {
  return `M${points.map(([x, y]) => `${fmt(x)} ${fmt(y)}`).join('L')}${closed ? 'Z' : ''}`;
}

export const css = (c: Color): string => `#${c.toString(16).padStart(6, '0')}`;

function paint(s: Paint, filled: boolean): string {
  const a: string[] = [];
  a.push(filled && s.fill !== undefined ? `fill="${css(s.fill)}"` : 'fill="none"');
  if (s.stroke !== undefined && (s.width ?? 1) > 0) {
    a.push(`stroke="${css(s.stroke)}" stroke-width="${fmt(s.width ?? 1)}" stroke-linejoin="round"`);
    if (!filled || s.fill === undefined) a.push(`stroke-linecap="${s.cap ?? 'round'}"`);
  }
  if (s.alpha !== undefined && s.alpha < 1) a.push(`opacity="${Math.max(0, Math.round(s.alpha * 100) / 100)}"`);
  return a.join(' ');
}

export function shapeSvg(s: Shape): string {
  switch (s.kind) {
    case 'ellipse': {
      const rot = s.rot ? ` transform="rotate(${fmt((s.rot * 180) / Math.PI)} ${fmt(s.x)} ${fmt(s.y)})"` : '';
      return `<ellipse cx="${fmt(s.x)}" cy="${fmt(s.y)}" rx="${fmt(Math.max(0, s.rx))}" ry="${fmt(Math.max(0, s.ry))}"${rot} ${paint(s, true)}/>`;
    }
    case 'poly':
      return `<path d="${polyPath(s.points, true)}" ${paint(s, true)}/>`;
    case 'curve':
      return `<path d="${smoothPath(s.points, s.closed)}" ${paint(s, s.closed)}/>`;
    case 'line':
      return `<path d="${polyPath(s.points, false)}" ${paint(s, false)}/>`;
  }
}

export function pieceD(p: Piece): string {
  return p.smooth ? smoothPath(p.points, true) : polyPath(p.points, true);
}

/** One group as SVG markup (see the header for the layer order). */
export function groupSvg(g: Group, gid: string, o: RenderOptions, ink: Color): string {
  if (g.pieces.length === 0 && g.over.length === 0) return '';
  const anim = o.animate && g.sway
    ? (() => {
        const [x, y] = g.sway.pivot.map(fmt);
        const d = g.sway.deg;
        return `<animateTransform attributeName="transform" type="rotate" values="0 ${x} ${y};${fmt(d)} ${x} ${y};0 ${x} ${y};${fmt(-d * 0.6)} ${x} ${y};0 ${x} ${y}" dur="${g.sway.dur}s" repeatCount="indefinite" calcMode="spline" keySplines=".45 0 .55 1;.45 0 .55 1;.45 0 .55 1;.45 0 .55 1"/>`;
      })()
    : '';
  let out = `<g data-part="${g.part}">${anim}`;
  if (g.pieces.length) {
    const ds = g.pieces.map(pieceD);
    const id = `${o.id}-${gid}`;
    const defs = ds.map((d, i) => `<path id="${id}-${i}" d="${d}"/>`).join('');
    const uses = (attrs = '') => ds.map((_, i) => `<use href="#${id}-${i}"${attrs}/>`).join('');
    const L = o.rim.light;
    const S = o.rim.shade;
    // Light from the top left: highlight rim on the upper-left edges, shade rim on the lower-right.
    const lx = fmt(L * 0.55), ly = fmt(L * 0.85);
    const sx = fmt(-S * 0.3), sy = fmt(-S);
    const b = piecesBounds(g.pieces, Math.max(L, S) + 4);
    const box = `x="${fmt(b.minX)}" y="${fmt(b.minY)}" width="${fmt(b.maxX - b.minX)}" height="${fmt(b.maxY - b.minY)}"`;
    const mask = (mid: string, dx: string, dy: string) =>
      `<mask id="${mid}" maskUnits="userSpaceOnUse" ${box}><rect ${box} fill="#fff"/><g fill="#000" transform="translate(${dx} ${dy})">${uses()}</g></mask>`;
    out +=
      `<defs>${defs}<clipPath id="${id}-c">${uses()}</clipPath>${mask(`${id}-ms`, sx, sy)}${mask(`${id}-ml`, lx, ly)}</defs>` +
      (o.outline > 0 ? `<g fill="${css(ink)}" stroke="${css(ink)}" stroke-width="${fmt(o.outline * 2)}" stroke-linejoin="round">${uses()}</g>` : '') +
      `<g clip-path="url(#${id}-c)">` +
      `<g fill="${css(g.tones.base)}">${uses()}</g>` +
      g.detail.map(shapeSvg).join('') +
      `<rect ${box} fill="${css(g.tones.shade)}" opacity="${GLAZE.shade}" mask="url(#${id}-ms)"/>` +
      `<rect ${box} fill="${css(g.tones.light)}" opacity="${GLAZE.light}" mask="url(#${id}-ml)"/>` +
      `</g>`;
  }
  out += g.over.map(shapeSvg).join('');
  return `${out}</g>`;
}

/** The creature as SVG markup (no <svg> wrapper): shadow, then the groups back to front. */
export function drawingSvg(d: Drawing, o: RenderOptions): string {
  const shadow = d.shadow ? shapeSvg(d.shadow) : '';
  return shadow + d.groups.map((g, i) => groupSvg(g, `g${i}`, o, d.ink)).join('');
}

function piecesBounds(pieces: Piece[], pad: number): { minX: number; maxX: number; minY: number; maxY: number } {
  let minX = Infinity, maxX = -Infinity, minY = Infinity, maxY = -Infinity;
  for (const p of pieces) for (const [x, y] of p.points) {
    minX = Math.min(minX, x); maxX = Math.max(maxX, x); minY = Math.min(minY, y); maxY = Math.max(maxY, y);
  }
  return { minX: minX - pad, maxX: maxX + pad, minY: minY - pad, maxY: maxY + pad };
}

/** Bounds of a drawing in its own units (pieces, details and overlays). */
export function drawingBounds(d: Drawing): { minX: number; maxX: number; minY: number; maxY: number } {
  let minX = Infinity, maxX = -Infinity, minY = Infinity, maxY = -Infinity;
  const addPt = (x: number, y: number, pad = 0) => {
    minX = Math.min(minX, x - pad); maxX = Math.max(maxX, x + pad); minY = Math.min(minY, y - pad); maxY = Math.max(maxY, y + pad);
  };
  for (const g of d.groups) {
    for (const p of g.pieces) for (const [x, y] of p.points) addPt(x, y, 1.5);
    for (const s of g.over) {
      const pad = (s.width ?? 0) / 2;
      if (s.kind === 'ellipse') {
        addPt(s.x - s.rx, s.y - s.ry, pad);
        addPt(s.x + s.rx, s.y + s.ry, pad);
      } else for (const [x, y] of s.points) addPt(x, y, pad);
    }
  }
  return { minX, maxX, minY, maxY };
}
