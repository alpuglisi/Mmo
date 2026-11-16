import { describe, expect, it } from 'vitest';
import { GameModel } from './model';

describe('toasts', () => {
  it('folds a burst of alike messages into one with a count', () => {
    const m = new GameModel();
    m.toast('info', 'Founder #3 was eaten.');
    m.toast('info', 'Founder #17 was eaten.');
    m.toast('info', 'Founder #20 was eaten.');
    expect(m.toasts).toEqual([{ kind: 'info', text: 'Founder #20 was eaten.', count: 3 }]);
    m.toast('info', 'A storm is coming.');
    m.toast('error', 'A storm is coming.');
    expect(m.toasts.map((t) => t.count ?? 1)).toEqual([3, 1, 1]);
  });
});
