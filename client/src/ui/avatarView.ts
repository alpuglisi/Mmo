// The live founder preview on the design screen: a card that redraws the
// avatar (`world/avatar.ts`) from the design on every change. Desktop: above
// the Budget card in the sticky side column. Phones: a compact sticky header
// at the top of the editor that can be collapsed to a single line.

import type { Morphology } from '../morphology';
import type { DesignSpec } from '../protocol';
import { avatarSvg, captionOf, describeAvatar, morphologyOf } from '../world/avatar';
import { sizeWord } from '../bodyMath';

import { h } from './dom';

const COLLAPSED_KEY = 'evo.avatarCollapsed';

function readCollapsed(): boolean {
  try {
    return localStorage.getItem(COLLAPSED_KEY) === '1';
  } catch {
    return false;
  }
}

function writeCollapsed(v: boolean): void {
  try {
    localStorage.setItem(COLLAPSED_KEY, v ? '1' : '0');
  } catch {
    // Private mode: the toggle still works for this visit.
  }
}

export interface AvatarView {
  el: HTMLElement;
  /** Redraw for a design and its body plan (without one, the traits' default body plan). */
  update: (d: DesignSpec, body?: Morphology) => void;
}

/** `extra`: shown under the caption (the editor's vision readout). */
export function avatarView(extra?: HTMLElement): AvatarView {
  const figure = h('div', { id: 'design-avatar', class: 'avatar-figure', role: 'img', 'aria-label': '' });
  const caption = h('p', { class: 'avatar-caption muted small', id: 'design-avatar-caption', 'aria-hidden': 'true' });
  const toggle = h('button', { type: 'button', class: 'avatar-toggle secondary', 'aria-expanded': 'true', 'aria-controls': 'design-avatar' }, 'Hide');
  const el = h('section', { class: 'card avatar-card', 'aria-label': 'Founder preview' },
    h('div', { class: 'avatar-head' }, h('h2', {}, 'Preview'), toggle),
    figure, caption, ...(extra ? [extra] : []));

  let collapsed = false;
  const setCollapsed = (v: boolean) => {
    collapsed = v;
    el.classList.toggle('collapsed', v);
    toggle.textContent = v ? 'Show' : 'Hide';
    toggle.setAttribute('aria-expanded', String(!v));
  };
  setCollapsed(readCollapsed());
  toggle.addEventListener('click', () => {
    setCollapsed(!collapsed);
    writeCollapsed(collapsed);
  });

  const motion = typeof matchMedia === 'function' ? matchMedia('(prefers-reduced-motion: reduce)') : null;
  let design: DesignSpec | null = null;
  let body: Morphology | undefined;
  let last = '';
  const render = () => {
    if (!design) return;
    const m = morphologyOf(design, body);
    const svg = avatarSvg(m, { animate: !motion?.matches, id: 'pv' });
    if (svg === last) return;
    last = svg;
    figure.innerHTML = svg;
    figure.setAttribute('aria-label', `Founder preview: ${describeAvatar(m)}`);
    // The covering the caption names is the one the avatar draws (`captionOf`).
    // One size scale everywhere: the weight class the inspector shows for
    // the same animal (a 4.2 kg body is "small" in both, playtest 7); with the weight.
    const word = sizeWord(m.mass);
    caption.textContent = captionOf(m).replace(/^(\S+)/, `${word} (adult ≈ ${m.mass >= 10 ? Math.round(m.mass) : m.mass.toFixed(1)} kg)`);
  };
  motion?.addEventListener?.('change', () => {
    last = '';
    render();
  });
  return {
    el,
    update: (d, b) => {
      design = d;
      body = b;
      render();
    },
  };
}
