// The raster renderer: paints a `Drawing` onto a 2D canvas in the house
// style, layer for layer as `render.ts` writes it as SVG (STYLE.md,
// "Rendering"), with the same curves (`tracePath` makes the Catmull-Rom
// Béziers of `smoothPath`), so a world sprite rasterised here looks like the
// library's SVG of it. Per group, on the target:
//   1. outline  – one ink stroke, twice the outline width, round the union
//                 of the pieces;
//   2. base     – every piece filled with the base tone (covering the
//                 stroke's inner half); then, clipped to the union, the
//                 details;
//   3. glazes   – still clipped: the shade (highlight) tone everywhere except
//                 the silhouette shifted up-left (down-right), at the glaze
//                 strength: the rim masks of the SVG renderer;
//   4. overlays – on top, unclipped.
// The union is one path with every piece wound the same way (the nonzero
// rule then fills their union; pieces of opposite winding would cancel out
// where they overlap). The glaze masks need a scratch canvas: all the
// groups' masks are drawn into one atlas first, so it is rasterised once per
// sprite rather than once per group (each canvas-to-canvas copy flushes the
// source). Synchronous and DOM-light (one reused scratch canvas, paths built
// point by point without SVG strings), so it works under WebGL and canvas
// renderers and in the mobile webviews alike.

import type { Point, Shape } from './geom';
import { css, GLAZE, type Drawing } from './render';

export interface CanvasRenderOptions {
  /** Outline width in drawing units. */
  outline: number;
  /** Highlight and shade rim widths in drawing units (0, 0: no glazes). */
  rim: { light: number; shade: number };
  /**
   * Leave the far-side groups (far legs, ears and wing: already in the
   * shadow ramp) flat, without glazes: at sprite size their rims are a pixel
   * or two.
   */
  flatFar?: boolean;
}

const FAR = new Set<string>(['legs-far', 'ears-far', 'wing-far']);

/** A canvas with a 2D context; the caller makes it (`document.createElement('canvas')`, or a test double). */
export interface Canvas2D {
  width: number;
  height: number;
  getContext(kind: '2d'): CanvasRenderingContext2D | null;
}

/** What a path is built with (a `Path2D`, or a recorder in tests). */
export interface PathSink {
  moveTo(x: number, y: number): void;
  lineTo(x: number, y: number): void;
  bezierCurveTo(c1x: number, c1y: number, c2x: number, c2y: number, x: number, y: number): void;
  closePath(): void;
}

/**
 * Trace points as `render.ts` does: a smooth path is the Catmull-Rom spline
 * of `smoothPath` (the same cubic Béziers), a straight one the polyline of
 * `polyPath`.
 */
export function tracePath(p: PathSink, points: Point[], smooth: boolean, closed: boolean): void {
  const n = points.length;
  if (n === 0) return;
  p.moveTo(points[0][0], points[0][1]);
  if (!smooth || n < 3) {
    for (let i = 1; i < n; i++) p.lineTo(points[i][0], points[i][1]);
  } else {
    const at = (i: number): Point => (closed ? points[(i + n) % n] : points[Math.max(0, Math.min(n - 1, i))]);
    const last = closed ? n : n - 1;
    for (let i = 0; i < last; i++) {
      const p0 = at(i - 1), p1 = at(i), p2 = at(i + 1), p3 = at(i + 2);
      p.bezierCurveTo(
        p1[0] + (p2[0] - p0[0]) / 6, p1[1] + (p2[1] - p0[1]) / 6,
        p2[0] - (p3[0] - p1[0]) / 6, p2[1] - (p3[1] - p1[1]) / 6,
        p2[0], p2[1],
      );
    }
  }
  // (`smoothPath` closes only smooth paths of 3+ points; `polyPath` closes when asked.)
  if (closed && (!smooth || n >= 3)) p.closePath();
}

function shapePath(s: Shape): Path2D {
  const p = new Path2D();
  switch (s.kind) {
    case 'ellipse':
      p.ellipse(s.x, s.y, Math.max(0, s.rx), Math.max(0, s.ry), s.rot ?? 0, 0, Math.PI * 2);
      break;
    case 'poly':
      tracePath(p, s.points, false, true);
      break;
    case 'curve':
      tracePath(p, s.points, true, s.closed);
      break;
    case 'line':
      tracePath(p, s.points, false, false);
      break;
  }
  return p;
}

/** One detail or overlay shape, painted as `shapeSvg` paints it. */
export function paintShape(ctx: CanvasRenderingContext2D, s: Shape): void {
  const filled = s.kind === 'ellipse' || s.kind === 'poly' || (s.kind === 'curve' && s.closed);
  const fill = filled && s.fill !== undefined;
  const stroked = s.stroke !== undefined && (s.width ?? 1) > 0;
  if (!fill && !stroked) return;
  const p = shapePath(s);
  const a = ctx.globalAlpha;
  if (s.alpha !== undefined && s.alpha < 1) ctx.globalAlpha = a * Math.max(0, s.alpha);
  if (fill) {
    ctx.fillStyle = css(s.fill!);
    ctx.fill(p);
  }
  if (stroked) {
    ctx.strokeStyle = css(s.stroke!);
    ctx.lineWidth = s.width ?? 1;
    ctx.lineJoin = 'round';
    // SVG strokes of filled shapes keep the default butt cap.
    ctx.lineCap = fill ? 'butt' : (s.cap ?? 'round');
    ctx.stroke(p);
  }
  ctx.globalAlpha = a;
}

/** Signed area (shoelace): the winding of a closed outline. */
export function signedArea(points: Point[]): number {
  let a = 0;
  for (let i = 0, n = points.length; i < n; i++) {
    const [x1, y1] = points[i];
    const [x2, y2] = points[(i + 1) % n];
    a += x1 * y2 - x2 * y1;
  }
  return a / 2;
}

let maskCanvas: Canvas2D | null = null;

/** The scratch canvas for the glaze masks, at least `w` × `h` and cleared there. */
function maskOf(make: () => Canvas2D, w: number, h: number): CanvasRenderingContext2D {
  if (!maskCanvas) maskCanvas = make();
  const c = maskCanvas;
  // Grow only (a resize reallocates; most sprites fit the last size).
  if (c.width < w || c.height < h) {
    c.width = Math.max(c.width, w);
    c.height = Math.max(c.height, h);
  }
  const ctx = c.getContext('2d')!;
  ctx.setTransform(1, 0, 0, 1, 0, 0);
  ctx.globalCompositeOperation = 'source-over';
  ctx.globalAlpha = 1;
  ctx.clearRect(0, 0, w, h);
  return ctx;
}

interface GroupPlan {
  /** Each piece on its own (base fill, mask cut-outs). */
  pieces: Path2D[];
  /** All pieces wound the same way: under the nonzero rule, their union (outline, clip). */
  union: Path2D;
  /** Pixel bounds on the target; its masks' row in the atlas (shade at x 0, highlight at x `bw`). */
  x0: number;
  y0: number;
  bw: number;
  bh: number;
  my: number;
  glazed: boolean;
}

/**
 * Paint `d` onto `ctx` (its current transform, a scale and a translation,
 * maps drawing units to pixels; the target's pixel size is `w` × `h`).
 * `make` makes the scratch canvas the first time. The shadow is left out
 * (sprites stand on their own ring). A glaze is drawn as an image with its
 * strength in its alpha, which composites exactly as the SVG's translucent
 * rect ("over" is associative).
 */
export function paintDrawing(ctx: CanvasRenderingContext2D, d: Drawing, o: CanvasRenderOptions, w: number, h: number, make: () => Canvas2D): void {
  const t = ctx.getTransform();
  const ink = css(d.ink);
  const pad = Math.max(o.outline * 2, o.rim.light, o.rim.shade) + 2;
  const glazes = o.rim.shade > 0 || o.rim.light > 0;
  // Plan: paths and pixel bounds per group, mask rows stacked in the atlas.
  let atlasH = 0;
  let atlasW = 0;
  const plans: (GroupPlan | null)[] = d.groups.map((g) => {
    if (!g.pieces.length) return null;
    let minX = Infinity, maxX = -Infinity, minY = Infinity, maxY = -Infinity;
    const union = new Path2D();
    const pieces = g.pieces.map((pc) => {
      for (const [x, y] of pc.points) {
        if (x < minX) minX = x;
        if (x > maxX) maxX = x;
        if (y < minY) minY = y;
        if (y > maxY) maxY = y;
      }
      const p = new Path2D();
      tracePath(p, pc.points, pc.smooth, true);
      // (A Catmull-Rom curve is the same traced backwards.)
      tracePath(union, signedArea(pc.points) < 0 ? pc.points.slice().reverse() : pc.points, pc.smooth, true);
      return p;
    });
    const x0 = Math.max(0, Math.floor(t.a * (minX - pad) + t.e));
    const y0 = Math.max(0, Math.floor(t.d * (minY - pad) + t.f));
    const bw = Math.max(0, Math.min(w, Math.ceil(t.a * (maxX + pad) + t.e)) - x0);
    const bh = Math.max(0, Math.min(h, Math.ceil(t.d * (maxY + pad) + t.f)) - y0);
    const glazed = glazes && bw > 0 && bh > 0 && !(o.flatFar && FAR.has(g.part));
    const plan: GroupPlan = { pieces, union, x0, y0, bw, bh, my: atlasH, glazed };
    if (glazed) {
      atlasH += bh;
      atlasW = Math.max(atlasW, bw * 2);
    }
    return plan;
  });
  // The glaze masks: the tone at the glaze strength, minus the shifted silhouette.
  if (atlasH > 0) {
    const M = maskOf(make, atlasW, atlasH);
    d.groups.forEach((g, i) => {
      const p = plans[i];
      if (!p?.glazed) return;
      const slot = (mx: number, tone: number, alpha: number, dx: number, dy: number) => {
        M.setTransform(1, 0, 0, 1, 0, 0);
        M.globalCompositeOperation = 'source-over';
        M.globalAlpha = alpha;
        M.fillStyle = css(tone);
        M.fillRect(mx, p.my, p.bw, p.bh);
        M.globalAlpha = 1;
        M.globalCompositeOperation = 'destination-out';
        // Target pixel (x0, y0) lands on the slot's corner; the silhouette moves by (dx, dy) units.
        M.setTransform(t.a, t.b, t.c, t.d, t.e - p.x0 + mx + t.a * dx + t.c * dy, t.f - p.y0 + p.my + t.b * dx + t.d * dy);
        M.fillStyle = '#000';
        for (const q of p.pieces) M.fill(q);
      };
      // Light from the top left (offsets as in render.ts).
      if (o.rim.shade > 0) slot(0, g.tones.shade, GLAZE.shade, -o.rim.shade * 0.3, -o.rim.shade);
      if (o.rim.light > 0) slot(p.bw, g.tones.light, GLAZE.light, o.rim.light * 0.55, o.rim.light * 0.85);
    });
  }
  d.groups.forEach((g, i) => {
    const p = plans[i];
    if (p) {
      // 1. Outline.
      if (o.outline > 0) {
        ctx.strokeStyle = ink;
        ctx.lineWidth = o.outline * 2;
        ctx.lineJoin = 'round';
        ctx.stroke(p.union);
      }
      // 2. Base, then the details clipped to the silhouette.
      ctx.fillStyle = css(g.tones.base);
      for (const q of p.pieces) ctx.fill(q);
      ctx.save();
      ctx.clip(p.union);
      for (const s of g.detail) paintShape(ctx, s);
      // 3. Glazes, clipped too: the shade, then the highlight.
      if (p.glazed) {
        const src = maskCanvas as unknown as CanvasImageSource;
        ctx.setTransform(1, 0, 0, 1, 0, 0);
        if (o.rim.shade > 0) ctx.drawImage(src, 0, p.my, p.bw, p.bh, p.x0, p.y0, p.bw, p.bh);
        if (o.rim.light > 0) ctx.drawImage(src, p.bw, p.my, p.bw, p.bh, p.x0, p.y0, p.bw, p.bh);
      }
      ctx.restore();
    }
    // 4. Overlays.
    for (const s of g.over) paintShape(ctx, s);
  });
}
