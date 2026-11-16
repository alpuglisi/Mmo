// Dev / test page for the body builder: `?builder` mounts it alone, with no
// login or server, for the e2e test (e2e/builder.mjs) and screenshots.
// `?builder=watcher` (any preset id) starts from that preset.
// `?builder=gallery` shows the art library's reference creatures instead
// (`art/gallery.ts`): each as the founder preview draws it, with its caption
// and its 64 / 32 px sprites, for the e2e gallery screenshots.

import { h } from '../dom';
import type { Morphology } from '../../morphology';
import { createBodyBuilder, type BodyBuilder, type BuilderChange } from './builder';
import { DEFAULT_PRESETS } from './presets';
import { GALLERY } from '../../art/gallery';
import { avatarSpriteSvg, avatarSvg, captionOf, describeAvatar, morphologyOf } from '../../world/avatar';

declare global {
  interface Window {
    /** The harness's builder and its change log, for the e2e test. */
    evoBuilder?: { builder: BodyBuilder; changes: { value: Morphology; change: BuilderChange }[] };
  }
}

/** The reference creatures, one card each (the founder preview's drawing). */
export function mountAvatarGallery(root: HTMLElement): void {
  document.documentElement.classList.add('bb-harness-page');
  const cards = GALLERY.map((g, i) => {
    const m = morphologyOf(g.design, g.body);
    const fig = h('div', { class: 'avatar-figure', role: 'img', 'aria-label': describeAvatar(m) });
    fig.innerHTML = avatarSvg(m, { animate: false, id: `gal${i}` });
    const sprites = h('div', { class: 'gallery-sprites', 'aria-hidden': 'true' });
    sprites.innerHTML = avatarSpriteSvg(m, 64, { id: `gs${i}` }) + avatarSpriteSvg(m, 32, { id: `gt${i}` });
    return h('section', { class: 'card gallery-card', 'data-creature': g.id },
      h('h3', {}, g.label), fig, h('p', { class: 'avatar-caption muted small' }, captionOf(m)), sprites);
  });
  root.replaceChildren(h('div', { class: 'screen bb-harness' },
    h('div', { class: 'topbar' }, h('strong', {}, 'Creature gallery'), h('span', { class: 'muted small' }, 'art library reference creatures')),
    h('div', { class: 'gallery-grid', id: 'avatar-gallery' }, ...cards)));
}

export function mountBuilderHarness(root: HTMLElement, param: string): BodyBuilder | null {
  if (param === 'gallery') {
    mountAvatarGallery(root);
    return null;
  }
  const changes: { value: Morphology; change: BuilderChange }[] = [];
  const out = h('pre', { id: 'bb-json', class: 'bb-json small muted' });
  const preset = DEFAULT_PRESETS.find((p) => p.id === param);
  const builder = createBodyBuilder({
    value: preset?.value,
    onChange: (value, change) => {
      changes.push({ value, change });
      if (change.final) show();
    },
  });
  const show = () => {
    out.textContent = JSON.stringify(builder.value);
  };
  show();
  window.evoBuilder = { builder, changes };
  // The page scrolls as a whole here (the app's screens are fixed-height).
  document.documentElement.classList.add('bb-harness-page');
  root.replaceChildren(h('div', { class: 'screen bb-harness' },
    h('div', { class: 'topbar' }, h('strong', {}, 'Body builder'), h('span', { class: 'muted small' }, 'dev harness')),
    h('div', { class: 'bb-harness-body' }, builder.el, out)));
  return builder;
}
