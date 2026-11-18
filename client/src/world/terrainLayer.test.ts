import { describe, expect, it } from 'vitest';
import { paintBudget, STALE, TERRAIN_BUDGET_MAX_MS, TERRAIN_BUDGET_MS, URGENT, workOrder, type ChunkWork } from './terrainLayer';

const chunk = (name: string, over: Partial<ChunkWork>): ChunkWork & { name: string } => ({
  name, row: 100, rows: 100, shown: true, stale: 0, paintedAt: 0, d: 0, ...over,
});

describe('terrain repaint scheduling', () => {
  it('paints longer on slow frames, within bounds', () => {
    expect(paintBudget(8)).toBe(TERRAIN_BUDGET_MS);
    expect(paintBudget(16.7)).toBeCloseTo(3.34);
    expect(paintBudget(50)).toBeCloseTo(10);
    expect(paintBudget(200)).toBeCloseTo(32);
    expect(paintBudget(5000)).toBe(TERRAIN_BUDGET_MAX_MS);
    expect(paintBudget(Number.NaN)).toBe(TERRAIN_BUDGET_MS);
  });

  it('finishes what it started, then the unpainted, then big changes, then the oldest', () => {
    const order = workOrder([
      chunk('fresh-enough', { stale: STALE / 2, d: 0 }),
      chunk('old-near', { stale: STALE, paintedAt: 900, d: 1 }),
      chunk('old-far', { stale: STALE * 2, paintedAt: 100, d: 40 }),
      chunk('melted', { stale: URGENT, d: 30 }),
      chunk('melted-more', { stale: 1, d: 50 }),
      chunk('never-far', { shown: false, row: 0, d: 20 }),
      chunk('never-near', { shown: false, row: 0, d: 5 }),
      chunk('half-done', { row: 40, stale: 0, d: 60 }),
      chunk('just-started', { row: 0, stale: 0, d: 70 }), // a repaint begun, no row painted yet: not forgotten
    ]).map((c) => c.name);
    expect(order).toEqual(['half-done', 'just-started', 'never-near', 'never-far', 'melted-more', 'melted', 'old-far', 'old-near']);
  });

  it('starves no chunk: the ones round the center changing again and again wait their turn', () => {
    // Twenty chunks, the screen center ones dirtied on every terrain update;
    // two repaints per update fit in the budget.
    const cs = Array.from({ length: 20 }, (_, i) => chunk(`c${i}`, { d: i, stale: STALE, paintedAt: 0 }));
    let now = 0;
    const repaintedAt = new Map<string, number>();
    for (let update = 1; update <= 20; update++) {
      for (const c of cs.slice(0, 3)) c.stale = STALE * 2; // the center keeps changing
      for (const c of workOrder(cs).slice(0, 2)) {
        c.stale = 0;
        c.paintedAt = ++now;
        repaintedAt.set(c.name, update);
      }
    }
    // Every chunk was repainted, not just the center ones.
    expect(repaintedAt.size).toBe(20);
  });
});
