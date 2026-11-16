import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { paintDrawing, signedArea, tracePath, type Canvas2D, type PathSink } from './canvas';
import { fmt, GLAZE, polyPath, smoothPath, type Drawing } from './render';
import type { Point } from './geom';

/** A path recorded as SVG path data, rounded as `render.ts` rounds it. */
class PathRecorder implements PathSink {
  d = '';
  cmds: [string, number[]][] = [];
  private add(c: string, ...v: number[]) {
    this.cmds.push([c, v]);
    this.d += c === 'Z' ? 'Z' : `${c}${v.map(fmt).join(' ')}`;
  }
  moveTo(x: number, y: number) { this.add('M', x, y); }
  lineTo(x: number, y: number) { this.add('L', x, y); }
  bezierCurveTo(a: number, b: number, c: number, d: number, x: number, y: number) { this.add('C', a, b, c, d, x, y); }
  closePath() { this.add('Z'); }
  ellipse(x: number, y: number, rx: number, ry: number) { this.add('E', x, y, rx, ry); }
}

const blob: Point[] = [[0, 0], [10, -4], [20, 0], [18, 10], [6, 12]];

describe('tracePath', () => {
  it('traces the same curves as the SVG renderer', () => {
    const smooth = new PathRecorder();
    tracePath(smooth, blob, true, true);
    expect(smooth.d).toBe(smoothPath(blob, true));
    const open = new PathRecorder();
    tracePath(open, blob, true, false);
    expect(open.d).toBe(smoothPath(blob, false));
    const poly = new PathRecorder();
    tracePath(poly, blob, false, true);
    expect(poly.d).toBe(polyPath(blob, true));
  });

  it('knows a closed outline’s winding', () => {
    expect(Math.sign(signedArea(blob))).toBe(-Math.sign(signedArea(blob.slice().reverse())));
    expect(signedArea([[0, 0], [10, 0], [10, 10], [0, 10]])).toBe(100);
  });
});

// ---------------------------------------------------------------- paintDrawing, against a recording 2D context

type Op = { op: string; target: string; args: unknown[] };

function recordingCanvas(name: string, ops: Op[]): Canvas2D & { ctx: Record<string, unknown> } {
  let transform = { a: 1, b: 0, c: 0, d: 1, e: 0, f: 0 };
  const ctx: Record<string, unknown> = {
    globalAlpha: 1, globalCompositeOperation: 'source-over', fillStyle: '#000', strokeStyle: '#000', lineWidth: 1, lineJoin: 'miter', lineCap: 'butt',
    getTransform: () => ({ ...transform }),
    setTransform: (a: number, b: number, c: number, d: number, e: number, f: number) => { transform = { a, b, c, d, e, f }; },
  };
  for (const op of ['fill', 'stroke', 'clip', 'save', 'restore', 'fillRect', 'clearRect', 'drawImage']) {
    ctx[op] = (...args: unknown[]) => ops.push({ op, target: name, args: [...args, { fill: ctx.fillStyle, alpha: ctx.globalAlpha, comp: ctx.globalCompositeOperation, width: ctx.lineWidth }] });
  }
  return { width: 0, height: 0, ctx, getContext: () => ctx as unknown as CanvasRenderingContext2D };
}

describe('paintDrawing', () => {
  const g0 = globalThis as unknown as { Path2D?: unknown };
  let saved: unknown;
  beforeEach(() => {
    saved = g0.Path2D;
    g0.Path2D = PathRecorder;
  });
  afterEach(() => {
    g0.Path2D = saved;
  });

  const tones = { base: 0x808080, shade: 0x404060, light: 0xc0c090 };
  const square = (x: number, y: number, s: number, cw = true): Point[] => {
    const p: Point[] = [[x, y], [x + s, y], [x + s, y + s], [x, y + s]];
    return cw ? p : p.reverse();
  };
  const drawing: Drawing = {
    ink: 0x221a16,
    shadow: null,
    groups: [
      { part: 'legs-far', tones, pieces: [{ tag: 'leg-0', points: square(0, 0, 10), smooth: false }], detail: [], over: [] },
      {
        part: 'body', tones,
        // Two overlapping pieces wound opposite ways (their union must not cancel out).
        pieces: [{ tag: 'body', points: square(0, 0, 20), smooth: false }, { tag: 'head', points: square(15, -5, 10, false), smooth: false }],
        detail: [{ part: 'body', tag: 'spot', kind: 'ellipse', x: 5, y: 5, rx: 2, ry: 2, fill: 0xff0000 }],
        over: [{ part: 'head', tag: 'glint', kind: 'ellipse', x: 20, y: 0, rx: 1, ry: 1, fill: 0xffffff }],
      },
    ],
  };

  it('paints each group as the SVG renderer layers it, with the glaze masks drawn once', () => {
    const ops: Op[] = [];
    const target = recordingCanvas('target', ops);
    const made: string[] = [];
    const make = () => { made.push('mask'); return recordingCanvas('mask', ops); };
    target.getContext('2d')!.setTransform(2, 0, 0, 2, 10, 20);
    paintDrawing(target.getContext('2d')!, drawing, { outline: 1, rim: { light: 1, shade: 2 } }, 100, 100, make);
    const seq = ops.map((o) => `${o.target}.${o.op}`);
    // Every mask is drawn before the target is touched (one rasterisation of the scratch canvas).
    const firstTarget = seq.findIndex((s) => s.startsWith('target.'));
    expect(seq.slice(firstTarget).some((s) => s.startsWith('mask.'))).toBe(false);
    // Shade and highlight of every group, the far legs included (as render.ts glazes them).
    expect(seq.slice(0, firstTarget).filter((s) => s === 'mask.fillRect')).toHaveLength(4);
    const fills = ops.filter((o) => o.target === 'mask' && o.op === 'fillRect').map((o) => o.args[4] as { fill: string; alpha: number; comp: string });
    const pair = [{ fill: '#404060', alpha: GLAZE.shade, comp: 'source-over' }, { fill: '#c0c090', alpha: GLAZE.light, comp: 'source-over' }];
    expect(fills.map(({ fill, alpha, comp }) => ({ fill, alpha, comp }))).toEqual([...pair, ...pair]);
    // Each cut out by the shifted silhouette.
    expect(ops.filter((o) => o.target === 'mask' && o.op === 'fill').every((o) => (o.args[1] as { comp: string }).comp === 'destination-out')).toBe(true);
    // On the target: far legs (outline, base, clip, two glazes), then the body: outline, base, clip, detail, two glazes, restore, then the overlay.
    expect(seq.slice(firstTarget)).toEqual([
      'target.stroke', 'target.fill', 'target.save', 'target.clip', 'target.drawImage', 'target.drawImage', 'target.restore',
      'target.stroke', 'target.fill', 'target.fill', 'target.save', 'target.clip', 'target.fill', 'target.drawImage', 'target.drawImage', 'target.restore',
      'target.fill',
    ]);
    expect(made.length).toBeLessThanOrEqual(1);
    // The outline and clip path: both body pieces, wound the same way.
    const union = ops.filter((o) => o.target === 'target' && o.op === 'clip')[1].args[0] as PathRecorder;
    const loops: Point[][] = [];
    for (const [c, v] of union.cmds) {
      if (c === 'M') loops.push([[v[0], v[1]]]);
      else if (c === 'L') loops[loops.length - 1].push([v[0], v[1]]);
    }
    expect(loops).toHaveLength(2);
    expect(Math.sign(signedArea(loops[0]))).toBe(Math.sign(signedArea(loops[1])));
    // The outline is twice the outline width (its inner half under the base).
    const stroke = ops.find((o) => o.target === 'target' && o.op === 'stroke')!;
    expect((stroke.args[1] as { width: number }).width).toBe(2);
  });
});
