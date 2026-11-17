// Coverings and markings (STYLE.md, "Coverings", "Colouration"): textures
// and patterns drawn INSIDE a silhouette (the renderer clips them to it), plus
// the silhouette edges that a covering changes (a furred or feathered rim).
//
// All functions are pure and deterministic (seeded): the same body always
// gets the same spots, so markings don't jump as sliders move.

import { lerp, rng, scallopRing, type Part, type Point, type Shape } from '../geom';
import { mix, type CreaturePalette } from '../palette';
import type { BodyPart } from './body';

export type Lod = 'portrait' | 'sprite';

/** The body rim with soft fur locks (0..1 amount: lock length). */
export function furRim(b: BodyPart, amount: number, seed = 3): Point[] {
  const reach = 1.6 + b.params.ry * 0.1 * (0.6 + amount);
  // Soft rounded clumps, longest under the belly and along the back, short on the chest.
  const n = 26;
  return scallopRing(b.points, n, (i) => reach * (0.5 + 0.5 * Math.abs(Math.sin(((i + 0.5) / n) * Math.PI * 2))), 0.5, rng(seed), true);
}

/** The body rim with overlapping rounded feathers. */
export function featherRim(b: BodyPart, amount: number, seed = 5): Point[] {
  const reach = 1.2 + b.params.ry * 0.06 * (0.6 + amount);
  return scallopRing(b.points, 14, () => reach, 0.9, rng(seed), true);
}

/** Hair strokes on the flank, swept back like the coat. */
export function furMarks(b: BodyPart, amount: number, pal: CreaturePalette, lod: Lod, part: Part = 'body'): Shape[] {
  if (lod === 'sprite') return [];
  const { rx, ry, cy } = b.params;
  const R = rng(17);
  const out: Shape[] = [];
  const len = 2.5 + 3.5 * amount;
  for (let i = 0; i < 48; i++) {
    const x = (R() * 2 - 1) * rx * 0.85;
    const y = cy + (R() * 2 - 1.1) * ry * 0.75;
    if (!b.inside(x, y, 0.85)) continue;
    const pale = i % 5 === 4;
    out.push({ part, tag: 'hair', kind: 'curve', closed: false, points: [[x, y], [x - len * 0.55, y + len * 0.35], [x - len, y + len * 0.5]], stroke: pale ? pal.body.light : pal.body.shade, width: 1, alpha: pale ? 0.55 : 0.6 });
  }
  return out;
}

/** Overlapping scale rows across the silhouette (clipped), and belly scutes. */
export function scaleField(b: BodyPart, pal: CreaturePalette, lod: Lod, faint = 0, part: Part = 'body'): Shape[] {
  const { rx, ry, cy } = b.params;
  const out: Shape[] = [];
  const s = lod === 'sprite' ? 7 : 4.2;
  const alpha = faint > 0 ? 0.25 + 0.5 * faint : 0.6;
  const stroke = mix(pal.body.shade, pal.ink, 0.25);
  const y0 = faint > 0 ? cy + ry * 0.35 : cy - ry * 1.3;
  for (let row = 0, y = y0; y < cy + ry * 1.3; y += s * 1.15, row++) {
    for (let x = -rx * 1.2 + (row % 2) * s; x < rx * 1.6; x += s * 2) {
      out.push({ part, tag: 'scale', kind: 'curve', closed: false, points: [[x - s, y], [x, y + s * 0.85], [x + s, y]], stroke, width: 0.8, alpha });
    }
  }
  if (faint === 0 && lod === 'portrait') {
    // Belly scutes: a paler band with transverse lines.
    for (let x = -rx * 0.75; x < rx * 0.8; x += 5.5) {
      const t = Math.acos(Math.max(-1, Math.min(1, x / rx)));
      const [, by] = b.edge(t);
      out.push({ part, tag: 'scute', kind: 'line', points: [[x, by - ry * 0.32], [x - 1, by]], stroke, width: 0.8, alpha: 0.5 });
    }
  }
  return out;
}

/** Feather rows over the body: rounded scallops, light-edged. */
export function featherField(b: BodyPart, pal: CreaturePalette, lod: Lod, part: Part = 'body'): Shape[] {
  if (lod === 'sprite') return [];
  const { rx, ry, cy } = b.params;
  const out: Shape[] = [];
  const s = 8;
  for (let row = 0, y = cy - ry * 0.75; y < cy + ry * 0.5; y += s * 1.2, row++) {
    for (let x = -rx * 0.9 + (row % 2) * s; x < rx * 0.95; x += s * 2.1) {
      if (!b.inside(x, y, 0.88)) continue;
      out.push({ part, tag: 'feather', kind: 'curve', closed: false, points: [[x + s * 0.8, y - s * 0.1], [x, y + s * 0.7], [x - s * 0.7, y + s * 0.1]], stroke: pal.body.shade, width: 1.1, alpha: 0.5 });
    }
  }
  return out;
}

/** Soft skin creases at the neck and over the joints. */
export function skinCreases(b: BodyPart, pal: CreaturePalette, lod: Lod): Shape[] {
  if (lod === 'sprite') return [];
  const { rx, ry, cy } = b.params;
  const out: Shape[] = [];
  for (const [x, y, s] of [[rx * 0.72, cy - ry * 0.1, 1], [rx * 0.64, cy + ry * 0.12, 0.8], [-rx * 0.45, cy + ry * 0.05, 0.9]] as [number, number, number][]) {
    out.push({ part: 'body', tag: 'crease', kind: 'curve', closed: false, points: [[x - 2, y - 7 * s], [x + 1.5, y], [x - 1, y + 6 * s]], stroke: pal.body.shade, width: 1.1, alpha: 0.55 });
  }
  return out;
}

/** Armor: a band of overlapping plates over the back (overlays with their own ink). */
export function armorPlates(b: BodyPart, amount: number, pal: CreaturePalette, dominant: boolean, lod: Lod): Shape[] {
  const { ry } = b.params;
  const out: Shape[] = [];
  const n = dominant ? 6 : 3;
  const span = dominant ? 2.3 : 1.0;
  const depth = ry * (dominant ? 0.42 + 0.22 * amount : 0.22 + 0.1 * amount);
  const plate = mix(mix(pal.body.base, pal.bone, 0.28), pal.body.shade, 0.15);
  const t0 = -Math.PI / 2 - span / 2;
  for (let i = 0; i < n; i++) {
    const a = t0 + (span * i) / n - 0.04;
    const c = t0 + (span * (i + 1)) / n + 0.06;
    const top: Point[] = [];
    for (let k = 0; k <= 4; k++) {
      const [x, y] = b.edge(lerp(a, c, k / 4));
      const [cx, cy] = b.center;
      const l = Math.hypot(x - cx, y - cy) || 1;
      top.push([x + ((x - cx) / l) * 2.5, y + ((y - cy) / l) * 2.5]);
    }
    const low = top.slice().reverse().map(([x, y]): Point => {
      const [cx, cy] = b.center;
      const l = Math.hypot(x - cx, y - cy) || 1;
      return [x - ((x - cx) / l) * depth, y - ((y - cy) / l) * depth];
    });
    out.push({ part: 'body', tag: 'plate', kind: 'curve', closed: true, points: [...top, ...low], fill: dominant ? plate : mix(plate, pal.body.base, 0.4), stroke: pal.ink, width: dominant ? 1.4 : 1, alpha: dominant ? 1 : 0.6 + 0.4 * amount });
    if (lod === 'portrait') {
      out.push({ part: 'body', tag: 'plate', kind: 'curve', closed: false, points: top.slice(1, 4).map(([x, y]): Point => [x - 0.5, y + 2.2]), stroke: mix(plate, 0xffffff, 0.5), width: 1.4, alpha: 0.8 });
      if (dominant) {
        const m = low[2];
        out.push({ part: 'body', tag: 'plate', kind: 'ellipse', x: m[0], y: m[1] - depth * 0.35, rx: 1.4, ry: 1.4, fill: pal.body.shade, alpha: 0.8 });
      }
    }
  }
  return out;
}

// ------------------------------------------------------------------ markings

/** Paler belly and chest (countershading: light from above). */
export function countershade(b: BodyPart, pal: CreaturePalette, k = 1): Shape[] {
  const { rx, ry, cy } = b.params;
  return [
    { part: 'body', tag: 'belly', kind: 'ellipse', x: rx * 0.05, y: cy + ry * 0.78, rx: rx * 0.86, ry: ry * 0.5, fill: pal.belly, alpha: 0.85 * k },
    { part: 'body', tag: 'belly', kind: 'ellipse', x: rx * 0.72, y: cy + ry * 0.35, rx: rx * 0.3, ry: ry * 0.5, fill: pal.belly, alpha: 0.7 * k },
  ];
}

/** A darker saddle along the back (gives drab coats structure). */
export function saddle(b: BodyPart, pal: CreaturePalette, k: number): Shape[] {
  const pts: Point[] = [];
  const { cy, ry } = b.params;
  for (let i = 0; i <= 10; i++) pts.push(b.edge(-Math.PI / 2 - 1.3 + (2.4 * i) / 10));
  for (let i = 10; i >= 0; i--) {
    const [x, y] = b.edge(-Math.PI / 2 - 1.3 + (2.4 * i) / 10);
    pts.push([x * 0.92, lerp(y, cy, 0.42) + ry * 0.05 * Math.sin(i * 1.7)]);
  }
  return [{ part: 'body', tag: 'saddle', kind: 'curve', closed: true, points: pts, fill: pal.pattern, alpha: 0.3 * k }];
}

/** An irregular blob (camouflage). */
function blob(x: number, y: number, r: number, R: () => number): Point[] {
  const pts: Point[] = [];
  for (let i = 0; i < 7; i++) {
    const a = (i / 7) * Math.PI * 2;
    const k = r * (0.65 + 0.55 * R());
    pts.push([x + Math.cos(a) * k * 1.3, y + Math.sin(a) * k]);
  }
  return pts;
}

/** Camouflage: mottled blotches and broken rosettes over the whole silhouette. */
export function camoBlotches(b: BodyPart, amount: number, pal: CreaturePalette, lod: Lod): Shape[] {
  const { rx, ry, cy } = b.params;
  const R = rng(11);
  const out: Shape[] = [];
  const n = Math.round((lod === 'sprite' ? 4 : 7) + 9 * amount);
  for (let i = 0; i < n; i++) {
    const x = (R() * 2 - 1) * rx * 1.05;
    const y = cy + (R() * 2 - 1.25) * ry * 0.8;
    const r = ry * (0.14 + 0.12 * R());
    out.push({ part: 'body', tag: 'camo', kind: 'curve', closed: true, points: blob(x, y, r, R), fill: i % 3 === 2 ? mix(pal.body.light, pal.pattern, 0.2) : pal.pattern, alpha: 0.35 + 0.5 * amount });
  }
  return out;
}

/** Display spots in the accent colour along the flank. */
export function spots(b: BodyPart, amount: number, color: number, lod: Lod): Shape[] {
  const { rx, ry, cy } = b.params;
  const R = rng(23);
  const out: Shape[] = [];
  const n = Math.round(3 + 8 * amount) - (lod === 'sprite' ? 3 : 0);
  for (let i = 0; i < n; i++) {
    const x = (R() * 2 - 1) * rx * 0.75;
    const y = cy - ry * 0.25 + (R() * 2 - 1) * ry * 0.4;
    if (!b.inside(x, y, 0.8)) continue;
    const s = ry * (0.08 + 0.07 * R());
    out.push({ part: 'body', tag: 'spot', kind: 'ellipse', x, y, rx: s * 1.15, ry: s, fill: color, alpha: 0.55 + 0.45 * amount });
  }
  return out;
}

/** Warning bands across the body: bold, curved with the body. */
export function warningBands(b: BodyPart, color: number): Shape[] {
  const out: Shape[] = [];
  for (const t of [-0.55, -0.12, 0.32]) {
    const w = 0.13;
    const pts: Point[] = [];
    for (let i = 0; i <= 4; i++) pts.push(b.edge(-Math.PI / 2 - t * 1.25 - w + (2 * w * i) / 4));
    const lo = [b.edge(Math.PI / 2 + t * 1.15 - w * 1.1), b.edge(Math.PI / 2 + t * 1.15 + w * 0.8)];
    // Overshoot the rim a little: the clip trims it to the silhouette.
    const out1 = pts.map(([x, y]): Point => [x, y - 4]);
    out.push({ part: 'body', tag: 'band', kind: 'curve', closed: true, points: [...out1, [lo[0][0], lo[0][1] + 4], [lo[1][0], lo[1][1] + 4]], fill: color });
  }
  return out;
}

/** A dorsal crest / frill (a silhouette piece; fill it with `crestFill`). */
export function crest(b: BodyPart, height: number, n = 7): Point[] {
  const pts: Point[] = [];
  const t0 = -Math.PI / 2 - 1.0;
  const t1 = -Math.PI / 2 + 0.55;
  for (let i = 0; i <= n * 2; i++) {
    const t = lerp(t0, t1, i / (n * 2));
    const [x, y] = b.edge(t);
    const env = Math.sin((Math.PI * i) / (n * 2));
    pts.push(i % 2 === 1 ? [x - height * 0.25, y - height * (0.45 + 0.55 * env)] : [x, y + 3]);
  }
  return pts;
}

/**
 * A coat pattern (Phase 3 `pattern`): dapples, vertical stripes or broken
 * blotches in the pattern tone, over the flank and back (clipped to the
 * silhouette). Plain draws nothing. Seeded, so it never jumps.
 */
export function coatPattern(b: BodyPart, pattern: 'plain' | 'spots' | 'stripes' | 'mottled', pal: CreaturePalette, lod: Lod): Shape[] {
  const { rx, ry, cy } = b.params;
  const out: Shape[] = [];
  const dark = mix(pal.pattern, pal.ink, 0.25);
  if (pattern === 'spots') {
    const R = rng(41);
    const n = lod === 'sprite' ? 9 : 22;
    for (let i = 0; i < n; i++) {
      const x = (R() * 2 - 1) * rx * 0.95;
      const y = cy + (R() * 2 - 1.2) * ry * 0.75;
      if (!b.inside(x, y, 0.95)) continue;
      const s = ry * (lod === 'sprite' ? 0.14 : 0.09) * (0.75 + 0.5 * R());
      out.push({ part: 'body', tag: 'pattern', kind: 'ellipse', x, y, rx: s * 1.2, ry: s, fill: dark, alpha: 0.75 });
    }
  } else if (pattern === 'stripes') {
    const n = lod === 'sprite' ? 5 : 8;
    for (let i = 0; i < n; i++) {
      const t = -0.82 + (1.64 * i) / (n - 1);
      const x = rx * t;
      const w = rx * (lod === 'sprite' ? 0.09 : 0.06);
      // From over the back (overshooting the rim: the clip trims it) to low on the flank, leaning back.
      out.push({ part: 'body', tag: 'pattern', kind: 'curve', closed: true, points: [[x - w, cy - ry * 1.4], [x + w, cy - ry * 1.4], [x - w * 0.2 + ry * 0.08, cy + ry * 0.45], [x - w * 1.6 + ry * 0.08, cy + ry * 0.4]], fill: dark, alpha: 0.7 });
    }
  } else if (pattern === 'mottled') {
    const R = rng(47);
    const n = lod === 'sprite' ? 6 : 12;
    for (let i = 0; i < n; i++) {
      const x = (R() * 2 - 1) * rx * 1.0;
      const y = cy + (R() * 2 - 1.2) * ry * 0.8;
      const r = ry * (0.16 + 0.14 * R());
      const pts: Point[] = [];
      for (let k = 0; k < 8; k++) {
        const a = (k / 8) * Math.PI * 2;
        const q = r * (0.5 + 0.7 * R());
        pts.push([x + Math.cos(a) * q * 1.4, y + Math.sin(a) * q]);
      }
      out.push({ part: 'body', tag: 'pattern', kind: 'curve', closed: true, points: pts, fill: i % 2 ? mix(pal.body.light, pal.belly, 0.5) : dark, alpha: i % 2 ? 0.55 : 0.6 });
    }
  }
  return out;
}
