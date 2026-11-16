// The live founder preview on the design screen: a card that redraws the
// avatar (`world/avatar.ts`) from the design on every change. Desktop: above
// the Budget card in the sticky side column. Phones: a compact sticky header
// at the top of the editor that can be collapsed to a single line.

import type { DesignSpec } from '../protocol';
import { avatarSvg, describeAvatar, morphologyOf, type MorphologyFeatures } from '../world/avatar';
import { describeLook } from '../world/creature';
import { h } from './dom';

declare global {
  interface Window {
    /**
     * Preview hook for the end-to-end screenshots and the morphology builder:
     * features the sliders don't set yet (leg count, eye count, tail shape...).
     */
    evoAvatar?: { setFeatures: (f: Partial<MorphologyFeatures>) => void };
  }
}

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
  update: (d: DesignSpec) => void;
}

export function avatarView(): AvatarView {
  const figure = h('div', { id: 'design-avatar', class: 'avatar-figure', role: 'img', 'aria-label': '' });
  const caption = h('p', { class: 'avatar-caption muted small', id: 'design-avatar-caption', 'aria-hidden': 'true' });
  const toggle = h('button', { type: 'button', class: 'avatar-toggle secondary', 'aria-expanded': 'true', 'aria-controls': 'design-avatar' }, 'Hide');
  const el = h('section', { class: 'card avatar-card', 'aria-label': 'Founder preview' },
    h('div', { class: 'avatar-head' }, h('h2', {}, 'Preview'), toggle),
    figure, caption);

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
  let features: Partial<MorphologyFeatures> = {};
  let last = '';
  const render = () => {
    if (!design) return;
    const m = morphologyOf(design, features);
    const svg = avatarSvg(m, { animate: !motion?.matches });
    if (svg === last) return;
    last = svg;
    figure.innerHTML = svg;
    figure.setAttribute('aria-label', `Founder preview: ${describeAvatar(m)}`);
    caption.textContent = describeLook(m.look);
  };
  motion?.addEventListener?.('change', () => {
    last = '';
    render();
  });
  window.evoAvatar = {
    setFeatures: (f) => {
      features = { ...f };
      render();
    },
  };
  return {
    el,
    update: (d) => {
      design = d;
      render();
    },
  };
}
