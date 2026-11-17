// Keyed list updates in place. Panels that rebuilt their rows on every state
// push lost input: an open <select> closed and its choice could change, and
// buttons moved under the cursor. `reconcile` keeps each row's element (found
// by its key), updates it, adds new rows and removes gone ones, and moves an
// element only when it is out of order, so focus, selection, scroll and a
// half-made click survive.

/** The bits of a DOM parent `reconcile` uses (a fake in the unit tests). */
export interface ParentLike<C> {
  readonly children: ArrayLike<object>;
  insertBefore(node: C, ref: never): unknown;
  removeChild(node: never): unknown;
}

const KEY = Symbol('reconcile-key');
type Keyed = { [KEY]?: string };

/**
 * Make `parent`'s children one element per item, in `items` order, keyed by
 * `keyOf`. A row whose key is new is made with `create`; every row (new or
 * kept) then gets `update`. Children that `reconcile` did not make (a "None."
 * placeholder) are removed.
 */
export function reconcile<T, C extends object>(
  parent: ParentLike<NoInfer<C>>,
  items: readonly T[],
  keyOf: (item: T) => string,
  create: (item: T) => C,
  update?: (el: C, item: T) => void,
): void {
  const existing = new Map<string, C>();
  for (const c of Array.from(parent.children)) {
    const k = (c as Keyed)[KEY];
    if (k !== undefined && !existing.has(k)) existing.set(k, c as C);
    else parent.removeChild(c as never);
  }
  const wanted = new Set(items.map(keyOf));
  for (const [k, c] of existing) {
    if (!wanted.has(k)) {
      parent.removeChild(c as never);
      existing.delete(k);
    }
  }
  for (let i = 0; i < items.length; i++) {
    const item = items[i];
    const k = keyOf(item);
    let el = existing.get(k);
    if (!el) {
      el = create(item);
      (el as Keyed)[KEY] = k;
      existing.set(k, el);
    }
    update?.(el, item);
    const at = parent.children[i] ?? null;
    if (at !== el) parent.insertBefore(el, at as never);
  }
}

/** Set a node's text only when it differs (no needless layout or a11y churn). */
export function setText(el: { textContent: string | null }, text: string): void {
  if (el.textContent !== text) el.textContent = text;
}
