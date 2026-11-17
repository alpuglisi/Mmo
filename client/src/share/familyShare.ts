// The Family tab's Share bar: the lineage's portrait card (today's average
// animal) and its family-tree card, shared through `share.ts` with the
// lineage's watch link.

import { h } from '../ui/dom';
import { morphologyOf } from '../world/avatar';
import type { FamilyModel } from '../familyTree';
import { drawPortraitCard, drawTreeCard, newCanvas, pngBlob } from './card';
import { cardFileName, shareCard, watchUrl } from './share';

export interface FamilyShareOptions {
  /** The world id ('' for the default). */
  world: string;
  /** The player's lineage index, for the watch link (none: no link). */
  lineage: number | null;
}

declare global {
  interface Window {
    /** The latest cards of the player's lineage as PNG data URLs (the marketing capture script). */
    evoCards?: { portrait: () => string | null; tree: () => string | null };
  }
}

/** The portrait card of the lineage as it is today (its latest stage). */
export function familyPortraitCard(m: FamilyModel): HTMLCanvasElement | null {
  const now = m.trunk[m.trunk.length - 1];
  if (m.empty || !now) return null;
  const c = newCanvas(1, 1);
  drawPortraitCard(c, morphologyOf(now.design, now.body), {
    title: m.name,
    subtitle: m.speciesName ? `Species: ${m.speciesName}` : m.extinct ? 'Died out' : 'A living lineage',
    facts: [now.subtitle],
    footer: 'Evolution MMO',
  });
  return c;
}

/** The family-tree card of the lineage. */
export function familyTreeCard(m: FamilyModel): HTMLCanvasElement | null {
  if (m.empty || !m.trunk.length) return null;
  const c = newCanvas(1, 1);
  drawTreeCard(c, m);
  return c;
}

/** Register `window.evoCards` for the latest model. */
export function exposeCards(m: FamilyModel): void {
  window.evoCards = {
    portrait: () => familyPortraitCard(m)?.toDataURL('image/png') ?? null,
    tree: () => familyTreeCard(m)?.toDataURL('image/png') ?? null,
  };
}

/** The Share bar for the Family tab (nothing while there is no lineage). */
export function familyShareBar(m: FamilyModel, opts: FamilyShareOptions): HTMLElement | null {
  if (m.empty) return null;
  const status = h('span', { class: 'muted small', id: 'family-share-status', 'aria-live': 'polite' });
  const url = opts.lineage !== null ? watchUrl(window.location.href, opts.world, opts.lineage) : undefined;
  const send = async (kind: 'portrait' | 'tree') => {
    const c = kind === 'portrait' ? familyPortraitCard(m) : familyTreeCard(m);
    if (!c) return;
    status.textContent = '';
    const out = await shareCard({
      blob: await pngBlob(c),
      fileName: cardFileName(m.name, kind),
      title: m.name,
      text: kind === 'portrait' ? `Meet ${m.name}, evolved in Evolution MMO` : `How ${m.name} evolved in Evolution MMO`,
      url,
    });
    status.textContent = out.cancelled ? '' : out.method === 'download' ? (out.copied ? 'Card saved; watch link copied.' : 'Card saved.') : 'Shared.';
  };
  const copy = async () => {
    if (!url) return;
    const ok = await navigator.clipboard?.writeText(url).then(() => true, () => false);
    status.textContent = ok ? 'Watch link copied: anyone can watch your lineage, no login.' : url;
  };
  return h('div', { class: 'ft-share', id: 'family-share' },
    h('button', { class: 'secondary', id: 'share-portrait', onclick: () => void send('portrait') }, 'Share portrait'),
    h('button', { class: 'secondary', id: 'share-tree', onclick: () => void send('tree') }, 'Share tree'),
    url ? h('button', { class: 'secondary', id: 'share-watch', onclick: () => void copy() }, 'Copy watch link') : null,
    status);
}
