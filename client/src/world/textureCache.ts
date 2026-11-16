// A keyed cache of expensive-to-make values (GPU textures): each key is made
// once and shared by every marker that needs it. Generic and PixiJS-free so
// the caching is unit-tested without a renderer.
//
// Bounded, least recently used first: with morphology a world holds far more
// distinct creature drawings than a client ever shows at once, so when the
// values' total cost (estimated bytes) passes `maxCost`, or their number
// `maxCount`, the cache drops the least recently used ones down to
// `lowWater` of the bound. Values still on screen
// (`inUse`) are never dropped, so the bound is soft: it can be exceeded by
// what is in use, never by what is not. Eviction runs only on a miss that
// passes the bound and then frees a margin, so its cost (one `inUse` call,
// a walk of the oldest entries) is rare.

export interface CacheOptions<T> {
  /** Called on every value dropped (clear or eviction). */
  dispose?: (value: T) => void;
  /** Cost of a value (default 1: a bound on the count). */
  cost?: (value: T) => number;
  /** Total cost from which the least recently used values are dropped (default: unbounded). */
  maxCost?: number;
  /** Number of values from which the least recently used are dropped (default: unbounded). */
  maxCount?: number;
  /** Eviction frees down to this fraction of the bounds (default 0.75). */
  lowWater?: number;
  /** The values that must not be dropped now (on screen); asked once per eviction. */
  inUse?: () => ReadonlySet<T>;
  /** Clock for timing makes (default `performance.now`). */
  now?: () => number;
}

interface Entry<T> {
  value: T;
  cost: number;
}

export class TextureCache<T> {
  /** In recency order: the first entry is the least recently used. */
  private items = new Map<string, Entry<T>>();
  private readonly dispose: (value: T) => void;
  private readonly costOf: (value: T) => number;
  private readonly now: () => number;
  /** How many values were made (cache misses), for tests and debugging. */
  made = 0;
  /** How many values were dropped to stay within `maxCost`. */
  evicted = 0;
  /** Total cost of the values held. */
  cost = 0;
  /** Time spent making values (ms): total and the slowest one. */
  makeMs = 0;
  maxMakeMs = 0;

  private readonly opts: CacheOptions<T>;

  /** `options` may be just the dispose function. */
  constructor(options: ((value: T) => void) | CacheOptions<T> = {}) {
    this.opts = typeof options === 'function' ? { dispose: options } : options;
    this.dispose = this.opts.dispose ?? (() => {});
    this.costOf = this.opts.cost ?? (() => 1);
    this.now = this.opts.now ?? (() => performance.now());
  }

  /** The value for `key`, made with `make` the first time. */
  get(key: string, make: () => T): T {
    const e = this.items.get(key);
    if (e !== undefined) {
      // Most recently used goes last.
      this.items.delete(key);
      this.items.set(key, e);
      return e.value;
    }
    const t0 = this.now();
    const value = make();
    const ms = this.now() - t0;
    this.made++;
    this.makeMs += ms;
    this.maxMakeMs = Math.max(this.maxMakeMs, ms);
    const cost = this.costOf(value);
    this.items.set(key, { value, cost });
    this.cost += cost;
    const { maxCost = Infinity, maxCount = Infinity, lowWater = 0.75 } = this.opts;
    if (this.cost > maxCost || this.items.size > maxCount) this.evict(maxCost * lowWater, Math.floor(maxCount * lowWater), key);
    return value;
  }

  /** Whether `key` is cached (without touching its recency). */
  has(key: string): boolean {
    return this.items.has(key);
  }

  /** Drop least recently used values not in use (and not `keep`) until within `cost` and `count`. */
  private evict(cost: number, count: number, keep: string): void {
    const inUse = this.opts.inUse?.() ?? new Set<T>();
    for (const [k, e] of this.items) {
      if (this.cost <= cost && this.items.size <= count) break;
      if (k === keep || inUse.has(e.value)) continue;
      this.items.delete(k);
      this.cost -= e.cost;
      this.evicted++;
      this.dispose(e.value);
    }
  }

  get size(): number {
    return this.items.size;
  }

  clear(): void {
    const items = [...this.items.values()];
    this.items.clear();
    this.cost = 0;
    for (const e of items) this.dispose(e.value);
  }
}
