import { describe, expect, it } from 'vitest';
import { reconcile, type ParentLike } from './reconcile';

/** A parent with an array of children and a log of the DOM moves made. */
class Fake implements ParentLike<{ name: string; text?: string }> {
  children: { name: string; text?: string }[] = [];
  moves = 0;
  insertBefore(node: { name: string }, ref: { name: string } | null): void {
    const from = this.children.indexOf(node);
    if (from >= 0) this.children.splice(from, 1);
    const at = ref ? this.children.indexOf(ref) : -1;
    this.children.splice(at < 0 ? this.children.length : at, 0, node);
    this.moves++;
  }
  removeChild(node: { name: string }): void {
    this.children.splice(this.children.indexOf(node), 1);
    this.moves++;
  }
}

const run = (p: Fake, ids: number[]) =>
  reconcile(p, ids, (i) => String(i), (i) => ({ name: `row${i}`, text: '' as string | undefined }), (el, i) => { el.text = `#${i}`; });

describe('reconcile', () => {
  it('keeps the same element for the same key and makes no moves when nothing changed', () => {
    const p = new Fake();
    run(p, [1, 2, 3]);
    const [a, b, c] = p.children;
    p.moves = 0;
    run(p, [1, 2, 3]);
    expect(p.children).toEqual([a, b, c]);
    expect(p.children[0]).toBe(a);
    expect(p.moves).toBe(0);
  });

  it('adds at the end without touching the rows above (buttons stay put)', () => {
    const p = new Fake();
    run(p, [1, 2]);
    const [a, b] = p.children;
    p.moves = 0;
    run(p, [1, 2, 3]);
    expect(p.children[0]).toBe(a);
    expect(p.children[1]).toBe(b);
    expect(p.children[2].name).toBe('row3');
    expect(p.moves).toBe(1);
  });

  it('removes gone rows, reorders, updates text, and drops strangers', () => {
    const p = new Fake();
    p.children.push({ name: 'None.' });
    run(p, [3, 1, 2]);
    expect(p.children.map((c) => c.name)).toEqual(['row3', 'row1', 'row2']);
    const one = p.children[1];
    run(p, [1, 3]);
    expect(p.children.map((c) => c.name)).toEqual(['row1', 'row3']);
    expect(p.children[0]).toBe(one);
    expect(p.children.map((c) => c.text)).toEqual(['#1', '#3']);
    run(p, []);
    expect(p.children).toEqual([]);
  });
});
