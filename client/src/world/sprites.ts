// Renders creature sprites into textures, once per distinct drawing
// (`creatureKey`): the body from the creature art library (`spriteArt.ts`,
// rasterised by `art/canvas.ts` onto a canvas texture, facing right), the
// status overlays from the shapes of `creature.ts`; in a bounded least-recently-used cache
// that never drops a texture on screen, and builds the per-organism marker from shared textures:
// a soft contact shadow with a thin owner ring at the feet (tinted by owner),
// the body (flipped to face its way), an optional status mark (a coral ring
// at the feet when possessed, a small pip when pregnant) and an action icon.
// One mark language, all at the feet (STYLE.md): nothing rings the body.
// Hundreds of markers then cost a few sprites each and no per-frame drawing.

import { CanvasSource, Container, Graphics, Rectangle, Sprite, Texture, type Renderer } from 'pixi.js';
import { paintDrawing, type Canvas2D } from '../art/canvas';
import { bodyRadius, creatureKey, hintShapes, spriteExtent, type CreatureSpec, type Hint, type Shape } from './creature';
import { spriteDrawing } from './spriteArt';
import { TextureCache } from './textureCache';
import { MARK } from './palette';

/** What the status mark shows. */
export type StatusKind = 'possessed' | 'pregnant';

/** Textures are drawn this much finer than one screen pixel, so they stay crisp when markers grow with zoom. */
const SUPERSAMPLE = 2.5;
/**
 * GPU memory the generated textures may hold before the least recently used
 * ones not on screen are dropped (estimated as width × height × 4 bytes). A
 * medium body is ~60 kB at resolution 2.5 and ~250 kB at 5 (a 2× screen),
 * so this keeps a few hundred distinct drawings.
 */
export const TEXTURE_BUDGET_BYTES = 48 * 1024 * 1024;
/** And at most this many textures (docs/morphology.md: about 256 on phones). */
export const TEXTURE_CAP = 256;

export function textureBytes(t: Texture): number {
  return (t.source?.pixelWidth ?? 0) * (t.source?.pixelHeight ?? 0) * 4;
}

export function drawShapes(g: Graphics, shapes: Shape[]): Graphics {
  for (const s of shapes) {
    if (s.kind === 'ellipse') g.ellipse(s.x, s.y, s.rx, s.ry);
    else if (s.kind === 'poly') g.poly(s.points.flat(), true);
    else {
      const [first, ...rest] = s.points;
      g.moveTo(first[0], first[1]);
      for (const p of rest) g.lineTo(p[0], p[1]);
      g.stroke({ width: s.width, color: s.stroke, alpha: s.alpha ?? 1, cap: 'round', join: 'round' });
      continue;
    }
    if (s.fill !== undefined) g.fill({ color: s.fill, alpha: s.alpha ?? 1 });
    if (s.stroke !== undefined && (s.width ?? 1) > 0) g.stroke({ width: s.width ?? 1, color: s.stroke, join: 'round' });
  }
  return g;
}

const newCanvas = (): Canvas2D => document.createElement('canvas');

/**
 * Rasterise a creature's sprite drawing onto a canvas texture `resolution`
 * times finer than a screen pixel at marker scale 1, the marker's centre in
 * the middle (so the sprite's anchor 0.5 holds it and a flip mirrors in place).
 */
export function rasterBody(s: CreatureSpec, resolution: number): Texture {
  const sd = spriteDrawing(s);
  const source = new CanvasSource({ resource: newCanvas() as HTMLCanvasElement, width: sd.half.w * 2, height: sd.half.h * 2, resolution });
  const canvas = source.resource as HTMLCanvasElement;
  const ctx = canvas.getContext('2d')!;
  const px = resolution * sd.k;
  ctx.setTransform(px, 0, 0, px, canvas.width / 2, canvas.height / 2 + sd.ground * resolution);
  paintDrawing(ctx, sd.drawing, { outline: sd.outline, rim: sd.rim }, canvas.width, canvas.height, newCanvas);
  source.update();
  return new Texture({ source, label: `body:${creatureKey(s)}` });
}

/** The parts of one organism's marker; the textures are shared. */
export interface MarkerParts {
  root: Container;
  ring: Sprite;
  body: Sprite;
  status: Sprite;
  icon: Sprite;
}

/** Cache keys of a marker's textures. */
export const bodyKey = (s: CreatureSpec): string => `body:${creatureKey(s)}`;
const ringR = (s: CreatureSpec): number => Math.round(bodyRadius(s) * 2) / 2;
export const ringKey = (s: CreatureSpec, own: boolean): string => `ring:${ringR(s)}:${own}`;
export const statusKey = (s: CreatureSpec, kind: StatusKind): string => `status:${kind}:${ringR(s)}`;
export const hintKey = (h: Hint): string => `hint:${h}`;

export class CreatureSprites {
  readonly cache: TextureCache<Texture>;
  private resolution: number;
  /** Textures drawn outside the markers (`hold`), kept with those `inUse` names. */
  private holds = new Set<() => Iterable<Texture>>();

  /** `inUse`: the textures on screen now, which the cache must not drop when it passes its budget. */
  constructor(private renderer: Renderer, inUse?: () => ReadonlySet<Texture>, maxBytes = TEXTURE_BUDGET_BYTES, maxCount = TEXTURE_CAP) {
    this.resolution = Math.min(6, SUPERSAMPLE * (renderer.resolution || 1));
    const held = (): ReadonlySet<Texture> => {
      if (this.holds.size === 0) return inUse?.() ?? new Set();
      const all = new Set(inUse?.() ?? []);
      for (const h of this.holds) for (const t of h()) all.add(t);
      return all;
    };
    this.cache = new TextureCache<Texture>({ dispose: (t) => t.destroy(true), cost: textureBytes, maxCost: maxBytes, maxCount, inUse: held });
  }

  /**
   * Keep textures shown by something other than the markers (a gallery, a
   * preview) from being dropped while it is on screen: the cache destroys
   * what it evicts, and a destroyed texture still drawn throws in PixiJS
   * ("reading 'alphaMode'"). Returns the release.
   */
  hold(textures: () => Iterable<Texture>): () => void {
    const h = () => textures();
    this.holds.add(h);
    return () => { this.holds.delete(h); };
  }

  /** Whether every texture of a marker is made (so showing it costs no drawing). */
  ready(s: CreatureSpec, own: boolean, status: StatusKind | null, hint: Hint | null): boolean {
    const c = this.cache;
    return c.has(bodyKey(s)) && c.has(ringKey(s, own)) && (status === null || c.has(statusKey(s, status))) && (hint === null || c.has(hintKey(hint)));
  }

  private generate(extent: number, draw: (g: Graphics) => void): Texture {
    const g = new Graphics();
    draw(g);
    const tex = this.renderer.generateTexture({
      target: g,
      frame: new Rectangle(-extent, -extent, extent * 2, extent * 2),
      resolution: this.resolution,
      antialias: true,
      defaultAnchor: { x: 0.5, y: 0.5 },
    });
    g.destroy();
    return tex;
  }

  private render(key: string, extent: number, draw: (g: Graphics) => void): Texture {
    return this.cache.get(key, () => this.generate(extent, draw));
  }

  /** The body texture: the art library's sprite drawing, rasterised (facing right; a marker flips it). */
  body(s: CreatureSpec): Texture {
    return this.cache.get(bodyKey(s), () => rasterBody(s, this.resolution));
  }

  /**
   * At the feet: a soft contact shadow, offset down-right (light from the
   * top-left), and a thin white ring tinted with the owner's colour (bolder
   * for your own). The shadow is dark, so the tint leaves it as it is.
   */
  ownerRing(s: CreatureSpec, own: boolean): Texture {
    const r = ringR(s);
    return this.render(ringKey(s, own), spriteExtent(s), (g) => {
      const rx = r * 1.55;
      const ry = r * 0.6;
      const y = r * 0.95;
      // A blurred shadow from a few stacked ellipses.
      for (const k of [1.25, 1.1, 0.95]) g.ellipse(r * 0.12, y + r * 0.08, rx * k, ry * k).fill({ color: MARK.shadow, alpha: MARK.shadowAlpha / 3 });
      if (own) g.ellipse(0, y, rx, ry).fill({ color: 0xffffff, alpha: 0.16 });
      g.ellipse(0, y, rx, ry).stroke({ width: own ? 1.8 : 1.2, color: 0xffffff, alpha: own ? 1 : 0.8 });
    });
  }

  /** A status mark, tinted: possessed, a second ring just outside the owner ring; pregnant, a pip beside the feet. */
  statusMark(s: CreatureSpec, kind: StatusKind): Texture {
    const r = ringR(s);
    return this.render(statusKey(s, kind), spriteExtent(s), (g) => {
      const y = r * 0.95;
      if (kind === 'possessed') {
        g.ellipse(0, y, r * 1.95, r * 0.8).stroke({ width: 3.2, color: 0x0d1117, alpha: 0.35 });
        g.ellipse(0, y, r * 1.95, r * 0.8).stroke({ width: 1.8, color: 0xffffff });
      } else {
        const pr = Math.max(2.2, r * 0.3);
        g.circle(r * 1.55, y - r * 0.45, pr).fill({ color: 0xffffff }).stroke({ width: 1, color: 0x15181c, alpha: 0.85 });
      }
    });
  }

  hint(h: Hint): Texture {
    return this.render(hintKey(h), 7, (g) => drawShapes(g, hintShapes(h)));
  }

  marker(): MarkerParts {
    const root = new Container();
    const ring = new Sprite();
    const body = new Sprite();
    const status = new Sprite();
    const icon = new Sprite();
    for (const s of [ring, body, status, icon]) s.anchor.set(0.5);
    status.visible = false;
    icon.visible = false;
    root.addChild(ring, body, status, icon);
    return { root, ring, body, status, icon };
  }

  destroy(): void {
    this.cache.clear();
  }
}
