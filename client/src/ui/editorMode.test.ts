import { afterEach, describe, expect, it } from 'vitest';
import { MODE_KEY, readMode, writeMode } from './editorMode';

const g = globalThis as { localStorage?: unknown };

function fakeStorage(): Storage {
  const m = new Map<string, string>();
  return {
    getItem: (k: string) => m.get(k) ?? null,
    setItem: (k: string, v: string) => void m.set(k, v),
    removeItem: (k: string) => void m.delete(k),
    clear: () => m.clear(),
    key: () => null,
    get length() {
      return m.size;
    },
  };
}

afterEach(() => {
  delete g.localStorage;
});

describe('editor mode', () => {
  it('Simple by default, Builder once chosen, remembered', () => {
    g.localStorage = fakeStorage();
    expect(readMode()).toBe('simple');
    writeMode('builder');
    expect((g.localStorage as Storage).getItem(MODE_KEY)).toBe('builder');
    expect(readMode()).toBe('builder');
    writeMode('simple');
    expect(readMode()).toBe('simple');
  });

  it('no storage, or storage that throws: Simple, and writing does not throw', () => {
    expect(readMode()).toBe('simple');
    expect(() => writeMode('builder')).not.toThrow();
    g.localStorage = {
      getItem: () => { throw new Error('denied'); },
      setItem: () => { throw new Error('denied'); },
    };
    expect(readMode()).toBe('simple');
    expect(() => writeMode('builder')).not.toThrow();
    // Junk values read as Simple.
    g.localStorage = fakeStorage();
    (g.localStorage as Storage).setItem(MODE_KEY, 'expert');
    expect(readMode()).toBe('simple');
  });
});
