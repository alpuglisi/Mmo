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

describe('mating updates', () => {
  it('says a deadline move or a walk step nothing; a status change once', () => {
    const m = new GameModel();
    const offer = { id: 3, from_player: 1, from_name: 'ann', to_player: 2, to_name: 'bob', mine: 5, partner: 6, status: 'accepted', created_tick: 0, expires_tick: 100 };
    const send = (o: object) => m.apply({ type: 'mating_update', offer: { ...offer, ...o } } as never);
    send({});
    const said = m.toasts.length;
    for (let k = 1; k <= 20; k++) send({ expires_tick: 100 + 24 * k, meet: { mine_x: k, mine_y: 0, partner_x: 40, partner_y: 0, distance: 40 - k } });
    expect(m.toasts.length).toBe(said);
    expect(m.toasts[m.toasts.length - 1].count ?? 1).toBe(1);
    expect(m.offers.get(3)?.offer.expires_tick).toBe(100 + 24 * 20);
    send({ status: 'completed' });
    expect(m.toasts.length).toBe(said + 1);
  });
});
