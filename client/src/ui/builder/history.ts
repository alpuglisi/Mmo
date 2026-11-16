// Undo / redo for an immutable value. Pushes that share a `group` within
// `mergeMs` of each other merge into one step: a drag gesture (one group per
// gesture) or a run of arrow-key presses on one handle undoes as a whole.

export class History<T> {
  private past: T[] = [];
  private future: T[] = [];
  private lastGroup: string | null = null;
  private lastAt = 0;

  constructor(private current: T, private readonly opts: { limit?: number; mergeMs?: number; same?: (a: T, b: T) => boolean } = {}) {}

  get value(): T {
    return this.current;
  }

  get canUndo(): boolean {
    return this.past.length > 0;
  }

  get canRedo(): boolean {
    return this.future.length > 0;
  }

  /** Record `next` as the new value. Returns false (and records nothing) when it equals the current one. */
  push(next: T, group: string | null = null, now = Date.now()): boolean {
    if (this.opts.same?.(this.current, next)) return false;
    const merge = group !== null && group === this.lastGroup && now - this.lastAt <= (this.opts.mergeMs ?? 1000) && this.past.length > 0;
    if (!merge) {
      this.past.push(this.current);
      const limit = this.opts.limit ?? 200;
      if (this.past.length > limit) this.past.splice(0, this.past.length - limit);
    }
    this.current = next;
    this.future = [];
    this.lastGroup = group;
    this.lastAt = now;
    return true;
  }

  /** End any merge run: the next push starts a new step. */
  seal(): void {
    this.lastGroup = null;
  }

  undo(): T | null {
    const prev = this.past.pop();
    if (prev === undefined) return null;
    this.future.push(this.current);
    this.current = prev;
    this.seal();
    return prev;
  }

  redo(): T | null {
    const next = this.future.pop();
    if (next === undefined) return null;
    this.past.push(this.current);
    this.current = next;
    this.seal();
    return next;
  }

  /** Replace the value and forget all history. */
  reset(value: T): void {
    this.past = [];
    this.future = [];
    this.current = value;
    this.seal();
  }
}
