import { describe, expect, it } from 'vitest';
import { History } from './history';

describe('History', () => {
  it('undoes and redoes', () => {
    const h = new History(0);
    expect(h.canUndo).toBe(false);
    h.push(1, null, 0);
    h.push(2, null, 0);
    expect(h.value).toBe(2);
    expect(h.undo()).toBe(1);
    expect(h.undo()).toBe(0);
    expect(h.undo()).toBeNull();
    expect(h.value).toBe(0);
    expect(h.redo()).toBe(1);
    expect(h.redo()).toBe(2);
    expect(h.redo()).toBeNull();
  });

  it('a new push drops the redo branch', () => {
    const h = new History('a');
    h.push('b');
    h.undo();
    expect(h.canRedo).toBe(true);
    h.push('c');
    expect(h.canRedo).toBe(false);
    expect(h.undo()).toBe('a');
  });

  it('merges pushes of one group within the window', () => {
    const h = new History(0, { mergeMs: 1000 });
    h.push(1, 'drag-1', 0);
    h.push(2, 'drag-1', 500);
    h.push(3, 'drag-1', 1400); // 900 ms after the last push: still merged
    expect(h.undo()).toBe(0);
    h.redo();
    h.push(4, 'drag-1', 5000); // too late: a new step
    expect(h.undo()).toBe(3);
  });

  it('a different group, or seal(), starts a new step', () => {
    const h = new History(0);
    h.push(1, 'a', 0);
    h.push(2, 'b', 1);
    expect(h.undo()).toBe(1);
    h.redo();
    h.push(3, 'b', 2);
    h.seal();
    h.push(4, 'b', 3);
    expect(h.undo()).toBe(3);
  });

  it('skips values equal to the current one', () => {
    const h = new History({ x: 1 }, { same: (a, b) => a.x === b.x });
    expect(h.push({ x: 1 })).toBe(false);
    expect(h.canUndo).toBe(false);
    expect(h.push({ x: 2 })).toBe(true);
  });

  it('keeps at most `limit` steps and reset() forgets them', () => {
    const h = new History(0, { limit: 3 });
    for (let i = 1; i <= 10; i++) h.push(i);
    let n = 0;
    while (h.undo() !== null) n++;
    expect(n).toBe(3);
    expect(h.value).toBe(7);
    h.reset(42);
    expect(h.value).toBe(42);
    expect(h.canUndo || h.canRedo).toBe(false);
  });
});
