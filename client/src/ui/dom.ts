// Minimal DOM helpers: the panels are plain elements, no UI framework.

type Attrs = Record<string, string | number | boolean | EventListener | undefined | null>;
type Child = Node | string | number | null | undefined | false;

export function h<K extends keyof HTMLElementTagNameMap>(tag: K, attrs: Attrs = {}, ...children: Child[]): HTMLElementTagNameMap[K] {
  const el = document.createElement(tag);
  for (const [k, v] of Object.entries(attrs)) {
    if (v === undefined || v === null || v === false) continue;
    if (k.startsWith('on') && typeof v === 'function') {
      el.addEventListener(k.slice(2).toLowerCase(), v as EventListener);
    } else if (k === 'class') {
      el.className = String(v);
    } else if (v === true) {
      el.setAttribute(k, '');
    } else if (k === 'value' && 'value' in el) {
      (el as HTMLInputElement).value = String(v);
    } else {
      el.setAttribute(k, String(v));
    }
  }
  append(el, children);
  return el;
}

export function append(el: Node, children: Child[]): void {
  for (const c of children) {
    if (c === null || c === undefined || c === false) continue;
    el.appendChild(typeof c === 'string' || typeof c === 'number' ? document.createTextNode(String(c)) : c);
  }
}

export function clear(el: Node): void {
  while (el.firstChild) el.removeChild(el.firstChild);
}

export function pct(v: number, digits = 1): string {
  return `${(v * 100).toFixed(digits)}%`;
}

export function label(s: string): string {
  return s.replace(/_/g, ' ');
}

/** "1 tile", "12 tiles": a distance rounded to whole tiles (playtest 9: "1 tiles"). */
export function tiles(d: number): string {
  const n = Math.max(0, Math.round(d));
  return n === 1 ? '1 tile' : `${n} tiles`;
}

/**
 * Keep a hidden element out of the page (a comment holds its place) and put
 * it back when shown: hidden text stays out of copies, searches and screen
 * readers (playtests 12–13: "Start a new lineage", "Name your lineage…").
 */
const placeholders = new WeakMap<HTMLElement, Comment>();
export function present(el: HTMLElement, on: boolean): void {
  let mark = placeholders.get(el);
  if (!on && el.parentNode) {
    if (!mark) {
      mark = document.createComment(el.id || 'notice');
      placeholders.set(el, mark);
    }
    el.replaceWith(mark);
  } else if (on && !el.parentNode && mark?.parentNode) {
    mark.replaceWith(el);
  }
}
