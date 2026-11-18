// The app's own confirmation dialog (instead of the browser's `confirm()`,
// which looked foreign, blocked the page and couldn't be styled or read on a
// phone). Modal: focus goes to Cancel, Escape or a click outside cancels,
// Tab stays inside. Resolves true only for the confirming button.

import { h } from './dom';
import './onboarding.css';

export interface ConfirmOptions {
  title: string;
  body: string | string[];
  ok: string;
  cancel?: string;
  /** The confirming button in the danger style. */
  danger?: boolean;
  /** Id prefix for tests (`<id>-ok`, `<id>-cancel`). */
  id?: string;
}

export function confirmDialog(o: ConfirmOptions): Promise<boolean> {
  return new Promise((resolve) => {
    const id = o.id ?? 'confirm';
    const ok = h('button', { type: 'button', id: `${id}-ok`, class: o.danger ? 'danger' : '' }, o.ok) as HTMLButtonElement;
    const cancel = h('button', { type: 'button', id: `${id}-cancel`, class: 'secondary' }, o.cancel ?? 'Cancel') as HTMLButtonElement;
    const body = (Array.isArray(o.body) ? o.body : [o.body]).map((t) => h('p', { class: 'small' }, t));
    const dialog = h('div', { class: 'card help-card confirm-card', id, role: 'alertdialog', 'aria-modal': 'true', 'aria-labelledby': `${id}-title` },
      h('h2', { id: `${id}-title` }, o.title), ...body, h('div', { class: 'row wrap confirm-actions' }, cancel, ok));
    const backdrop = h('div', { class: 'help-backdrop confirm-backdrop' }, dialog);
    const before = document.activeElement as HTMLElement | null;
    const done = (yes: boolean) => {
      window.removeEventListener('keydown', onKey, true);
      backdrop.remove();
      before?.focus?.({ preventScroll: true });
      resolve(yes);
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        e.preventDefault();
        e.stopPropagation();
        done(false);
      } else if (e.key === 'Tab') {
        // Keep focus on the two buttons.
        e.preventDefault();
        (document.activeElement === ok ? cancel : ok).focus();
      }
    };
    ok.addEventListener('click', () => done(true));
    cancel.addEventListener('click', () => done(false));
    backdrop.addEventListener('mousedown', (e) => { if (e.target === backdrop) done(false); });
    window.addEventListener('keydown', onKey, true);
    document.body.append(backdrop);
    cancel.focus({ preventScroll: true });
  });
}
