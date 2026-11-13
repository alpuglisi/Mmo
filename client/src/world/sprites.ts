// Renders the creature shapes of `creature.ts` into textures, once per
// appearance class, and builds the per-organism marker from shared textures:
// an owner ring at the feet (tinted by owner), the body (flipped to face its
// way), an optional status ring (possessed, pregnant) and an action icon.
// Hundreds of markers then cost a few sprites each and no per-frame drawing.

import { Container, Graphics, Rectangle, Sprite, type Renderer, type Texture } from 'pixi.js';
import { bodyRadius, creatureKey, creatureShapes, hintShapes, spriteExtent, type CreatureSpec, type Hint, type Shape } from './creature';
import { TextureCache } from './textureCache';

/** Textures are drawn this much finer than one screen pixel, so they stay crisp when markers grow with zoom. */
const SUPERSAMPLE = 2.5;

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

/** The parts of one organism's marker; the textures are shared. */
export interface MarkerParts {
  root: Container;
  ring: Sprite;
  body: Sprite;
  status: Sprite;
  icon: Sprite;
}

export class CreatureSprites {
  readonly cache = new TextureCache<Texture>((t) => t.destroy(true));
  private resolution: number;

  constructor(private renderer: Renderer) {
    this.resolution = Math.min(6, SUPERSAMPLE * (renderer.resolution || 1));
  }

  private render(key: string, extent: number, draw: (g: Graphics) => void): Texture {
    return this.cache.get(key, () => {
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
    });
  }

  body(s: CreatureSpec): Texture {
    return this.render(`body:${creatureKey(s)}`, spriteExtent(s), (g) => drawShapes(g, creatureShapes(s)));
  }

  /** White ellipse at the feet, tinted with the owner's color. */
  ownerRing(s: CreatureSpec, own: boolean): Texture {
    const r = Math.round(bodyRadius(s) * 2) / 2;
    return this.render(`ring:${r}:${own}`, spriteExtent(s), (g) => {
      const rx = r * 1.7;
      const ry = r * 0.7;
      const y = r * 0.95;
      g.ellipse(0, y, rx, ry).fill({ color: 0xffffff, alpha: own ? 0.35 : 0.22 });
      g.ellipse(0, y, rx, ry).stroke({ width: own ? 2.2 : 1.5, color: 0xffffff });
    });
  }

  /** White circle around the whole creature (possessed, pregnant), tinted. */
  statusRing(s: CreatureSpec): Texture {
    const r = Math.round(bodyRadius(s) * 2) / 2;
    return this.render(`status:${r}`, spriteExtent(s), (g) => {
      g.circle(0, 0, r * 2.1).stroke({ width: 2.2, color: 0xffffff });
    });
  }

  hint(h: Hint): Texture {
    return this.render(`hint:${h}`, 7, (g) => drawShapes(g, hintShapes(h)));
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
