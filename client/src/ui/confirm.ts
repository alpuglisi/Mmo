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
  /** Its label; false for a notice with one button. */
  cancel?: string | false;
  /** The confirming button in the danger style. */
  danger?: boolean;
  /** Id prefix for tests (`<id>-ok`, `<id>-cancel`). */
  id?: string;
}

export function confirmDialog(o: ConfirmOptions): Promise<boolean> {
  return new Promise((resolve) => {
    const id = o.id ?? 'confirm';
    const ok = h('button', { type: 'button', id: `${id}-ok`, class: o.danger ? 'danger' : '' }, o.ok) as HTMLButtonElement;
    const notice = o.cancel === false;
    const cancel = h('button', { type: 'button', id: `${id}-cancel`, class: 'secondary' }, o.cancel || 'Cancel') as HTMLButtonElement;
    const body = (Array.isArray(o.body) ? o.body : [o.body]).map((t) => h('p', { class: 'small' }, t));
    const dialog = h('div', { class: 'card help-card confirm-card', id, role: 'alertdialog', 'aria-modal': 'true', 'aria-labelledby': `${id}-title` },
      h('h2', { id: `${id}-title` }, o.title), ...body, h('div', { class: 'row wrap confirm-actions' }, ...(notice ? [ok] : [cancel, ok])));
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
        (document.activeElement === ok && !notice ? cancel : ok).focus();
      }
    };
    ok.addEventListener('click', () => done(true));
    cancel.addEventListener('click', () => done(false));
    backdrop.addEventListener('mousedown', (e) => { if (e.target === backdrop) done(false); });
    window.addEventListener('keydown', onKey, true);
    document.body.append(backdrop);
    (notice ? ok : cancel).focus({ preventScroll: true });
  });
}

/** Ask for a line of text (instead of `prompt()`): the text, or null when cancelled. */
export function textDialog(o: { title: string; body?: string; value?: string; maxLength?: number; ok: string; id?: string }): Promise<string | null> {
  return new Promise((resolve) => {
    const id = o.id ?? 'text';
    const input = h('input', { type: 'text', id: `${id}-input`, maxlength: o.maxLength ?? 80, 'aria-labelledby': `${id}-title` }) as HTMLInputElement;
    input.value = o.value ?? '';
    const ok = h('button', { type: 'button', id: `${id}-ok` }, o.ok) as HTMLButtonElement;
    const cancel = h('button', { type: 'button', id: `${id}-cancel`, class: 'secondary' }, 'Cancel') as HTMLButtonElement;
    const dialog = h('div', { class: 'card help-card confirm-card', id, role: 'dialog', 'aria-modal': 'true', 'aria-labelledby': `${id}-title` },
      h('h2', { id: `${id}-title` }, o.title), o.body ? h('p', { class: 'small' }, o.body) : null, input,
      h('div', { class: 'row wrap confirm-actions' }, cancel, ok));
    const backdrop = h('div', { class: 'help-backdrop confirm-backdrop' }, dialog);
    const before = document.activeElement as HTMLElement | null;
    const focusables = [input, cancel, ok];
    const done = (v: string | null) => {
      window.removeEventListener('keydown', onKey, true);
      backdrop.remove();
      before?.focus?.({ preventScroll: true });
      resolve(v);
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        e.preventDefault();
        e.stopPropagation();
        done(null);
      } else if (e.key === 'Enter' && document.activeElement === input) {
        e.preventDefault();
        done(input.value);
      } else if (e.key === 'Tab') {
        e.preventDefault();
        const i = focusables.indexOf(document.activeElement as HTMLInputElement & HTMLButtonElement);
        focusables[(i + (e.shiftKey ? focusables.length - 1 : 1)) % focusables.length].focus();
      }
    };
    ok.addEventListener('click', () => done(input.value));
    cancel.addEventListener('click', () => done(null));
    backdrop.addEventListener('mousedown', (e) => { if (e.target === backdrop) done(null); });
    window.addEventListener('keydown', onKey, true);
    document.body.append(backdrop);
    input.focus({ preventScroll: true });
    input.select();
  });
}

export interface PromptOptions {
  title: string;
  body: string;
  label: string;
  value: string;
  maxLength: number;
  ok: string;
  id?: string;
  /** Act on the value: null when done (the dialog closes), else why not (shown, the dialog stays open). */
  submit: (value: string) => Promise<string | null>;
}

/** A text field in the app's dialog (instead of `prompt()`); a refusal keeps it open with the reason. */
export function promptDialog(o: PromptOptions): Promise<boolean> {
  return new Promise((resolve) => {
    const id = o.id ?? 'prompt';
    const input = h('input', { id: `${id}-input`, value: o.value, maxlength: o.maxLength, 'aria-label': o.label }) as HTMLInputElement;
    const error = h('p', { class: 'error small', id: `${id}-error`, role: 'alert' });
    const ok = h('button', { type: 'button', id: `${id}-ok` }, o.ok) as HTMLButtonElement;
    const cancel = h('button', { type: 'button', id: `${id}-cancel`, class: 'secondary' }, 'Cancel') as HTMLButtonElement;
    const dialog = h('div', { class: 'card help-card confirm-card', id, role: 'dialog', 'aria-modal': 'true', 'aria-labelledby': `${id}-title` },
      h('h2', { id: `${id}-title` }, o.title), h('p', { class: 'small' }, o.body), h('label', {}, o.label, input), error,
      h('div', { class: 'row wrap confirm-actions' }, cancel, ok));
    const backdrop = h('div', { class: 'help-backdrop confirm-backdrop' }, dialog);
    const before = document.activeElement as HTMLElement | null;
    const close = (done: boolean) => {
      window.removeEventListener('keydown', onKey, true);
      backdrop.remove();
      before?.focus?.({ preventScroll: true });
      resolve(done);
    };
    const go = async () => {
      ok.disabled = true;
      error.textContent = '';
      const why = await o.submit(input.value.trim()).catch((e: unknown) => String(e));
      ok.disabled = false;
      if (why === null) close(true);
      else {
        error.textContent = why;
        input.focus();
      }
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        e.preventDefault();
        e.stopPropagation();
        close(false);
      } else if (e.key === 'Enter' && document.activeElement === input) {
        e.preventDefault();
        void go();
      }
    };
    ok.addEventListener('click', () => void go());
    cancel.addEventListener('click', () => close(false));
    backdrop.addEventListener('mousedown', (e) => { if (e.target === backdrop) close(false); });
    window.addEventListener('keydown', onKey, true);
    document.body.append(backdrop);
    input.focus();
    input.select();
  });
}
