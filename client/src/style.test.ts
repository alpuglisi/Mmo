import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';

const css = readFileSync(new URL('./style.css', import.meta.url), 'utf8');

describe('style.css', () => {
  it('turns off the browser tap highlight page-wide', () => {
    // Android Chrome paints a teal box over a tapped control, above every
    // layer: a tapped trait info button showed through the sticky founder
    // preview as a stray square once the editor scrolled under it.
    expect(css).toMatch(/(^|\n)html\s*\{[^}]*-webkit-tap-highlight-color:\s*transparent/);
  });

  it('gives buttons their own press feedback instead', () => {
    expect(css).toMatch(/button:active:not\(:disabled\)\s*\{[^}]*filter/);
  });
});
