// A searchable organism picker for lists far too long for a <select> (a
// lineage of 2,690 adults made "My adult" unusable, above all on a phone).
// A button shows the choice; it opens an inline panel with a search box (id
// or words), filter chips (sex, free, not pregnant) and the matching
// organisms nearest first, at most `MAX_SHOWN` of them. The root element has a
// `value` property (the chosen id as a string, '' for none) and fires
// `change`, like the <select> it replaces.

import { h } from './dom';
import { reconcile, setText } from './reconcile';
import './picker.css';

export const MAX_SHOWN = 50;

export interface PickItem {
  id: number;
  /** "#12 female · gen 3". */
  label: string;
  /** Extra words after the label ("12 tiles", "pregnant", "sterile with yours"). */
  detail?: string;
  sex?: 'male' | 'female';
  pregnant?: boolean;
  /** Free to act now (not promised, not busy). */
  free?: boolean;
  /** Listed but can't be picked (the detail says why). */
  disabled?: boolean;
  /** For "nearest first". */
  distance?: number;
}

export type PickFilter = 'sex' | 'free' | 'notPregnant';

export interface PickerState {
  query: string;
  sex: 'any' | 'male' | 'female';
  free: boolean;
  notPregnant: boolean;
}

/** The items a picker shows for a search and filters: matching, pickable first, nearest first, capped. */
export function pickMatches(items: readonly PickItem[], st: PickerState, max = MAX_SHOWN): { shown: PickItem[]; total: number } {
  const q = st.query.trim().toLowerCase().replace(/^#/, '');
  const hits = items.filter((it) => {
    if (st.sex !== 'any' && it.sex && it.sex !== st.sex) return false;
    if (st.free && it.free === false) return false;
    if (st.notPregnant && it.pregnant) return false;
    if (!q) return true;
    if (/^\d+$/.test(q)) return String(it.id).startsWith(q);
    return `${it.label} ${it.detail ?? ''}`.toLowerCase().includes(q);
  });
  hits.sort((a, b) => Number(!!a.disabled) - Number(!!b.disabled) || (a.distance ?? Infinity) - (b.distance ?? Infinity) || a.id - b.id);
  return { shown: hits.slice(0, max), total: hits.length };
}

export interface Picker {
  el: HTMLElement & { value: string };
  setItems(items: readonly PickItem[]): void;
  /** Choose an id (null: none) without opening; fires no change. */
  set(id: number | null): void;
  readonly value: number | null;
  /** The current item, if it is in the list. */
  readonly item: PickItem | undefined;
}

export function createPicker(opts: { id: string; label: string; placeholder?: string; filters?: PickFilter[]; noneLabel?: string; allowNone?: boolean }): Picker {
  let items: readonly PickItem[] = [];
  let value: number | null = null;
  const st: PickerState = { query: '', sex: 'any', free: false, notPregnant: false };
  const button = h('button', { type: 'button', class: 'picker-button', 'aria-haspopup': 'listbox', 'aria-expanded': 'false' }) as HTMLButtonElement;
  const search = h('input', { type: 'search', class: 'picker-search', placeholder: opts.placeholder ?? 'Search by id', 'aria-label': `Search ${opts.label}`, inputmode: 'search' }) as HTMLInputElement;
  const chips = h('div', { class: 'picker-chips', role: 'group', 'aria-label': 'Filters' });
  const list = h('div', { class: 'picker-list', role: 'listbox', 'aria-label': opts.label });
  const more = h('p', { class: 'muted small picker-more' });
  const panel = h('div', { class: 'picker-panel', hidden: true }, search, chips, list, more);
  const root = h('div', { class: 'picker', id: opts.id }, button, panel) as unknown as HTMLElement & { value: string };

  const chip = (text: string, on: () => boolean, toggle: () => void) => {
    const b = h('button', { type: 'button', class: 'chip' }, text) as HTMLButtonElement;
    b.addEventListener('click', () => { toggle(); renderList(); paintChips(); });
    return { b, on };
  };
  const chipList: { b: HTMLButtonElement; on: () => boolean }[] = [];
  const filters = opts.filters ?? [];
  if (filters.includes('sex')) {
    chipList.push(chip('♀ females', () => st.sex === 'female', () => { st.sex = st.sex === 'female' ? 'any' : 'female'; }));
    chipList.push(chip('♂ males', () => st.sex === 'male', () => { st.sex = st.sex === 'male' ? 'any' : 'male'; }));
  }
  if (filters.includes('free')) chipList.push(chip('free now', () => st.free, () => { st.free = !st.free; }));
  if (filters.includes('notPregnant')) chipList.push(chip('not pregnant', () => st.notPregnant, () => { st.notPregnant = !st.notPregnant; }));
  chips.append(...chipList.map((c) => c.b));
  chips.hidden = chipList.length === 0;
  function paintChips(): void {
    for (const c of chipList) {
      c.b.classList.toggle('active', c.on());
      c.b.setAttribute('aria-pressed', String(c.on()));
    }
  }

  const current = () => (value === null ? undefined : items.find((i) => i.id === value));
  function paintButton(): void {
    const it = current();
    setText(button, it ? `${it.label}${it.detail ? ` · ${it.detail}` : ''}` : value !== null ? `#${value}` : (opts.noneLabel ?? 'Choose…'));
  }
  function choose(id: number | null): void {
    value = id;
    paintButton();
    setOpen(false);
    root.dispatchEvent(new Event('change', { bubbles: true }));
  }
  function renderList(): void {
    if (panel.hidden) return;
    const { shown, total } = pickMatches(items, st);
    const rows: (PickItem | null)[] = opts.allowNone ? [null, ...shown] : shown;
    reconcile(list, rows, (it) => (it ? String(it.id) : ''), (it) => {
      const b = h('button', { type: 'button', class: 'picker-row', role: 'option' }) as HTMLButtonElement;
      b.addEventListener('click', () => choose(it ? it.id : null));
      return b;
    }, (b, it) => {
      setText(b, it ? `${it.label}${it.detail ? ` · ${it.detail}` : ''}` : (opts.noneLabel ?? 'None'));
      (b as HTMLButtonElement).disabled = !!it?.disabled;
      b.setAttribute('aria-selected', String((it?.id ?? null) === value));
      b.classList.toggle('chosen', (it?.id ?? null) === value);
    });
    setText(more, total === 0 ? 'Nothing matches.' : total > shown.length ? `${shown.length} of ${total} shown, nearest first: search by id to find the others.` : '');
  }
  function setOpen(open: boolean): void {
    panel.hidden = !open;
    button.setAttribute('aria-expanded', String(open));
    if (open) {
      renderList();
      paintChips();
      // On a phone the keyboard would cover the list: focus the search only with a mouse.
      if (!window.matchMedia?.('(pointer: coarse)').matches) search.focus({ preventScroll: true });
      // Bring the opened list into view (playtest 11: on phones it opened below the sheet's fold).
      requestAnimationFrame(() => { if (!panel.hidden) panel.scrollIntoView?.({ block: 'nearest' }); });
    }
  }
  button.addEventListener('click', () => setOpen(panel.hidden));
  search.addEventListener('input', () => { st.query = search.value; renderList(); });
  root.addEventListener('keydown', (e) => {
    if (e.key === 'Escape' && !panel.hidden) {
      e.stopPropagation();
      setOpen(false);
      button.focus();
    }
  });

  Object.defineProperty(root, 'value', {
    get: () => (value === null ? '' : String(value)),
    set: (v: string) => { value = v === '' || v === null ? null : Number(v); paintButton(); },
  });
  paintButton();

  return {
    el: root,
    setItems(next) {
      items = next;
      paintButton();
      renderList();
    },
    set(id) {
      value = id;
      paintButton();
      renderList();
    },
    get value() {
      return value;
    },
    get item() {
      return current();
    },
  };
}
