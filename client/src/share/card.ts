// Shareable cards: a species portrait and a family tree, drawn on a canvas
// with the creature art library (`world/avatar.ts` through `art/canvas.ts`,
// the same drawing the founder preview and the Family tab show) and saved as
// PNG. The game's Share buttons use them (`share.ts`), and so does the
// marketing capture script (tools/marketing, through `window.evoCards`).
//
// Layout is computed by the pure helpers below (unit-tested); drawing needs
// a 2D canvas.

import { paintDrawing, type Canvas2D } from '../art/canvas';
import { drawingBounds } from '../art/render';
import { avatarDrawing, captionOf, describeAvatar, OUTLINE_PX, RIM, type AvatarModel } from '../world/avatar';
import type { FamilyModel, FamilyNode } from '../familyTree';
import { morphologyOf } from '../world/avatar';

/** Card sizes in pixels: a 4:5 portrait (feeds) and a 9:16 tree (stories). */
export const PORTRAIT_CARD = { w: 1080, h: 1350 } as const;
export const TREE_CARD = { w: 1080, h: 1920 } as const;

/** Most stages a tree card shows (founding, the latest, and the middle ones that fit). */
export const TREE_STAGES = 6;

const BG_TOP = '#14202b';
const BG_BOTTOM = '#0d1117';
const INK = '#e6edf3';
const MUTED = '#9aa7b4';
const ACCENT = '#7ee787';
const LINE = '#30363d';
const FONT = 'system-ui, -apple-system, "Segoe UI", Roboto, sans-serif';

/** What a portrait card says besides the creature. */
export interface PortraitInfo {
  /** The lineage or species name (the title). */
  title: string;
  /** A line under it ("Species: …", "by Ann"). */
  subtitle?: string;
  /** Up to four short facts ("312 alive", "Year 3", "18.7% of the world's genes"). */
  facts?: string[];
  /** Footer text (the game's name and a link). */
  footer?: string;
}

/**
 * Greedy word wrap of `text` into lines no wider than `max` by `measure`
 * (a word longer than a line gets a line of its own), at most `lines` lines
 * (the last one ends with "…" when cut).
 */
export function wrapText(text: string, max: number, measure: (s: string) => number, lines = 3): string[] {
  const words = text.split(/\s+/).filter(Boolean);
  const out: string[] = [];
  let cur = '';
  for (let i = 0; i < words.length; i++) {
    const next = cur ? `${cur} ${words[i]}` : words[i];
    if (cur && measure(next) > max) {
      out.push(cur);
      cur = words[i];
      if (out.length === lines) {
        out[lines - 1] = ellipsis(out[lines - 1], max, measure);
        return out;
      }
    } else {
      cur = next;
    }
  }
  if (cur) out.push(cur);
  if (out.length > lines) {
    out.length = lines;
    out[lines - 1] = ellipsis(out[lines - 1], max, measure);
  }
  return out;
}

function ellipsis(line: string, max: number, measure: (s: string) => number): string {
  let s = line;
  while (s.length > 1 && measure(`${s}…`) > max) s = s.slice(0, -1);
  return `${s.trimEnd()}…`;
}

/** The trunk stages a tree card shows: the first and last always, the latest middle ones in between, oldest first. */
export function treeStages(m: FamilyModel, max = TREE_STAGES): FamilyNode[] {
  const t = m.trunk;
  if (t.length <= max) return t.slice();
  return [t[0], ...t.slice(t.length - (max - 1))];
}

/** Rows of a tree card: y of each stage's centre and the portrait box size, for `n` stages between `top` and `bottom`. */
export function treeRows(n: number, top: number, bottom: number): { y: number[]; box: number } {
  if (n <= 0) return { y: [], box: 0 };
  // At most `ROW_MAX` apart: a short tree sits in the middle, not spread out.
  const step = Math.min(ROW_MAX, (bottom - top) / n);
  const start = (top + bottom) / 2 - (step * n) / 2;
  return { y: Array.from({ length: n }, (_, i) => start + step * (i + 0.5)), box: Math.min(300, step * 0.86) };
}

/** Largest distance between two tree card rows. */
const ROW_MAX = 380;

/** Paint the creature of `m` fitted into the box (x, y, w, h), standing on its bottom edge. */
export function paintCreature(ctx: CanvasRenderingContext2D, m: AvatarModel, x: number, y: number, w: number, h: number, make: () => Canvas2D): void {
  const d = avatarDrawing(m);
  const b = drawingBounds(d);
  const k = Math.min(w / (b.maxX - b.minX), h / (b.maxY - b.minY));
  ctx.save();
  ctx.setTransform(k, 0, 0, k, x + w / 2 - (k * (b.minX + b.maxX)) / 2, y + h - k * b.maxY);
  paintDrawing(ctx, { ...d, shadow: null }, { outline: (OUTLINE_PX * 1.6) / k, rim: RIM }, ctx.canvas.width, ctx.canvas.height, make);
  ctx.restore();
}

function background(ctx: CanvasRenderingContext2D, w: number, h: number): void {
  const g = ctx.createLinearGradient(0, 0, 0, h);
  g.addColorStop(0, BG_TOP);
  g.addColorStop(1, BG_BOTTOM);
  ctx.fillStyle = g;
  ctx.fillRect(0, 0, w, h);
}

function text(ctx: CanvasRenderingContext2D, s: string, x: number, y: number, size: number, color: string, weight = 400, align: CanvasTextAlign = 'left'): void {
  ctx.font = `${weight} ${size}px ${FONT}`;
  ctx.fillStyle = color;
  ctx.textAlign = align;
  ctx.textBaseline = 'alphabetic';
  ctx.fillText(s, x, y);
}

function footerLine(ctx: CanvasRenderingContext2D, w: number, h: number, footer: string): void {
  ctx.strokeStyle = LINE;
  ctx.lineWidth = 2;
  ctx.beginPath();
  ctx.moveTo(72, h - 120);
  ctx.lineTo(w - 72, h - 120);
  ctx.stroke();
  text(ctx, footer, w / 2, h - 64, 34, MUTED, 500, 'center');
}

/** A new canvas of `w` × `h` (in a page). */
export function newCanvas(w: number, h: number): HTMLCanvasElement {
  const c = document.createElement('canvas');
  c.width = w;
  c.height = h;
  return c;
}

const scratch = (): Canvas2D => document.createElement('canvas');

/** Draw a species portrait card onto `canvas` (sized `PORTRAIT_CARD`). */
export function drawPortraitCard(canvas: HTMLCanvasElement, m: AvatarModel, info: PortraitInfo): void {
  const { w, h } = PORTRAIT_CARD;
  canvas.width = w;
  canvas.height = h;
  const ctx = canvas.getContext('2d');
  if (!ctx) throw new Error('no 2D canvas');
  background(ctx, w, h);
  const measure = (size: number, weight: number) => (s: string) => {
    ctx.font = `${weight} ${size}px ${FONT}`;
    return ctx.measureText(s).width;
  };
  let y = 150;
  for (const line of wrapText(info.title, w - 144, measure(76, 700), 2)) {
    text(ctx, line, w / 2, y, 76, INK, 700, 'center');
    y += 86;
  }
  if (info.subtitle) text(ctx, info.subtitle, w / 2, y, 38, ACCENT, 500, 'center');
  // The creature on a soft spotlight and a ground line.
  const box = { x: 110, y: y + 50, w: w - 220, h: 600 };
  const glow = ctx.createRadialGradient(w / 2, box.y + box.h * 0.6, 40, w / 2, box.y + box.h * 0.6, box.w * 0.6);
  glow.addColorStop(0, 'rgba(126,231,135,0.16)');
  glow.addColorStop(1, 'rgba(126,231,135,0)');
  ctx.fillStyle = glow;
  ctx.fillRect(0, 0, w, h);
  paintCreature(ctx, m, box.x, box.y, box.w, box.h, scratch);
  ctx.strokeStyle = LINE;
  ctx.lineWidth = 4;
  ctx.beginPath();
  ctx.moveTo(140, box.y + box.h + 6);
  ctx.lineTo(w - 140, box.y + box.h + 6);
  ctx.stroke();
  y = box.y + box.h + 76;
  for (const line of wrapText(captionOf(m), w - 160, measure(34, 400), 2)) {
    text(ctx, line, w / 2, y, 34, MUTED, 400, 'center');
    y += 44;
  }
  const facts = (info.facts ?? []).slice(0, 4);
  if (facts.length) {
    y += 20;
    text(ctx, facts.join('  ·  '), w / 2, y, 36, INK, 600, 'center');
  }
  footerLine(ctx, w, h, info.footer ?? 'Evolution MMO');
  canvas.setAttribute('aria-label', `${info.title}: ${describeAvatar(m)}`);
}

/** Draw a family-tree card onto `canvas` (sized `TREE_CARD`): the lineage's stages, oldest at the top. */
export function drawTreeCard(canvas: HTMLCanvasElement, m: FamilyModel, footer = 'Evolution MMO'): void {
  const { w, h } = TREE_CARD;
  canvas.width = w;
  canvas.height = h;
  const ctx = canvas.getContext('2d');
  if (!ctx) throw new Error('no 2D canvas');
  background(ctx, w, h);
  text(ctx, m.name || 'A lineage', w / 2, 140, 70, INK, 700, 'center');
  text(ctx, m.extinct ? 'Its evolution, until it died out' : 'How it evolved', w / 2, 200, 38, ACCENT, 500, 'center');
  const stages = treeStages(m);
  const { y: ys, box } = treeRows(stages.length, 250, h - 150);
  const px = 90;
  // The trunk line behind the portraits.
  if (ys.length > 1) {
    ctx.strokeStyle = LINE;
    ctx.lineWidth = 8;
    ctx.beginPath();
    ctx.moveTo(px + box / 2, ys[0]);
    ctx.lineTo(px + box / 2, ys[ys.length - 1]);
    ctx.stroke();
  }
  const measure = (s: string) => {
    ctx.font = `500 32px ${FONT}`;
    return ctx.measureText(s).width;
  };
  stages.forEach((n, i) => {
    const cy = ys[i];
    ctx.fillStyle = n.extinct ? '#1a1f24' : '#161b22';
    ctx.strokeStyle = n.kind === 'now' ? ACCENT : LINE;
    ctx.lineWidth = 4;
    ctx.beginPath();
    ctx.roundRect(px, cy - box / 2, box, box, 28);
    ctx.fill();
    ctx.stroke();
    ctx.globalAlpha = n.extinct ? 0.55 : 1;
    paintCreature(ctx, morphologyOf(n.design, n.body), px + 18, cy - box / 2 + 18, box - 36, box - 36, scratch);
    ctx.globalAlpha = 1;
    const tx = px + box + 44;
    text(ctx, n.title, tx, cy - 40, 44, INK, 700);
    text(ctx, n.subtitle, tx, cy + 6, 30, MUTED, 400);
    const chips = n.changes.length ? n.changes.join(' · ') : n.kind === 'founding' ? 'As designed' : 'No visible change';
    wrapText(chips, w - tx - 60, measure, 2).forEach((line, k) => text(ctx, line, tx, cy + 54 + k * 40, 32, n.changes.length ? ACCENT : MUTED, 500));
  });
  footerLine(ctx, w, h, footer);
  canvas.setAttribute('aria-label', `Family tree of ${m.name}: ${stages.map((s) => s.title).join(', ')}`);
}

/** The canvas as a PNG blob. */
export function pngBlob(canvas: HTMLCanvasElement): Promise<Blob> {
  return new Promise((resolve, reject) => canvas.toBlob((b) => (b ? resolve(b) : reject(new Error('PNG encoding failed'))), 'image/png'));
}
