// A keyed cache of expensive-to-make values (GPU textures): each key is made
// once and shared by every marker that needs it. Generic and PixiJS-free so
// the caching is unit-tested without a renderer.

export class TextureCache<T> {
  private items = new Map<string, T>();
  /** How many values were made (cache misses), for tests and debugging. */
  made = 0;

  constructor(private dispose: (value: T) => void = () => {}) {}

  /** The value for `key`, made with `make` the first time. */
  get(key: string, make: () => T): T {
    let v = this.items.get(key);
    if (v === undefined) {
      v = make();
      this.made++;
      this.items.set(key, v);
    }
    return v;
  }

  get size(): number {
    return this.items.size;
  }

  clear(): void {
    for (const v of this.items.values()) this.dispose(v);
    this.items.clear();
  }
}
