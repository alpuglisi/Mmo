import { describe, expect, it } from 'vitest';
import { treeRows, treeStages, wrapText, TREE_STAGES } from './card';
import { buildFamilyTree } from '../familyTree';
import { DEMO_TREE } from '../ui/familyDemo';

const measure = (s: string) => s.length * 10;

describe('wrapText', () => {
  it('wraps on words within the width', () => {
    expect(wrapText('a bb ccc dddd', 60, measure)).toEqual(['a bb', 'ccc', 'dddd']);
    expect(wrapText('one', 100, measure)).toEqual(['one']);
    expect(wrapText('', 100, measure)).toEqual([]);
  });

  it('cuts with an ellipsis past the line limit', () => {
    const lines = wrapText('w1 w2 w3 w4 w5 w6 w7 w8', 50, measure, 2);
    expect(lines).toHaveLength(2);
    expect(lines[1].endsWith('…')).toBe(true);
    for (const l of lines) expect(measure(l)).toBeLessThanOrEqual(50);
  });
});

describe('tree card layout', () => {
  it('keeps the founding and the latest stages', () => {
    const m = buildFamilyTree(DEMO_TREE);
    const s = treeStages(m);
    expect(s.length).toBe(Math.min(TREE_STAGES, m.trunk.length));
    expect(s[0]).toBe(m.trunk[0]);
    expect(s[s.length - 1]).toBe(m.trunk[m.trunk.length - 1]);
    const many = { ...m, trunk: Array.from({ length: 10 }, (_, i) => ({ ...m.trunk[0], key: `k${i}` })) };
    const cut = treeStages(many, 4);
    expect(cut.map((n) => n.key)).toEqual(['k0', 'k7', 'k8', 'k9']);
  });

  it('spreads rows evenly with boxes that fit their step', () => {
    const r = treeRows(4, 200, 1000);
    expect(r.y).toEqual([300, 500, 700, 900]);
    // A short tree is centred, its rows at most 380 apart.
    expect(treeRows(2, 0, 2000).y).toEqual([810, 1190]);
    expect(r.box).toBeLessThanOrEqual(200);
    expect(treeRows(0, 0, 100)).toEqual({ y: [], box: 0 });
  });
});
