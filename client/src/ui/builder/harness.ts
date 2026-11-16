// Dev / test page for the body builder: `?builder` mounts it alone, with no
// login or server, for the e2e test (e2e/builder.mjs) and screenshots.
// `?builder=watcher` (any preset id) starts from that preset.

import { h } from '../dom';
import type { Morphology } from '../../morphology';
import { createBodyBuilder, type BodyBuilder, type BuilderChange } from './builder';
import { DEFAULT_PRESETS } from './presets';

declare global {
  interface Window {
    /** The harness's builder and its change log, for the e2e test. */
    evoBuilder?: { builder: BodyBuilder; changes: { value: Morphology; change: BuilderChange }[] };
  }
}

export function mountBuilderHarness(root: HTMLElement, param: string): BodyBuilder {
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
