import { describe, expect, it } from 'vitest';
import { notificationFor, parsePayload, pickClient, targetUrl } from './swLogic';

const payload = {
  title: 'Mating offer · Main', body: 'ann asks to mate.', world: 'main', tab: 'matings', kind: 'mating', count: 1, tag: 'evo-main', organism: 4,
};

describe('service worker logic', () => {
  it('accepts the server payload and refuses anything else', () => {
    expect(parsePayload(payload)).toEqual(payload);
    expect(parsePayload({ ...payload, kind: 'bogus' })).toBeNull();
    expect(parsePayload({ ...payload, world: 3 })).toBeNull();
    expect(parsePayload('text')).toBeNull();
    expect(parsePayload(null)).toBeNull();
    // Defaults for what the server always sends but a stranger might not.
    const p = parsePayload({ ...payload, count: undefined, tag: '', organism: 'x' })!;
    expect([p.count, p.tag, p.organism]).toEqual([1, 'evo-main', undefined]);
    expect(parsePayload({ ...payload, body: 'x'.repeat(5000) })!.body.length).toBe(2000);
  });

  it('shows one notification per world carrying where a click goes', () => {
    const n = notificationFor(parsePayload(payload));
    expect(n.title).toBe('Mating offer · Main');
    expect(n.options).toMatchObject({ body: 'ann asks to mate.', tag: 'evo-main', data: { world: 'main', tab: 'matings' } });
    // A push always shows something.
    const generic = notificationFor(null);
    expect(generic.title).toBe('Evolution MMO');
    expect(generic.options.data).toBeNull();
  });

  it('opens the game on the world and tab', () => {
    expect(targetUrl('https://game.example/play/', { world: 'isle', tab: 'market' })).toBe('https://game.example/play/?world=isle&tab=market');
    expect(targetUrl('https://game.example/', null)).toBe('https://game.example/');
  });

  it('reuses a focused, else visible, else any game window in scope', () => {
    const scope = 'https://game.example/play/';
    const other = { url: 'https://game.example/admin', focused: true };
    const hidden = { url: `${scope}?a`, visibilityState: 'hidden' };
    const visible = { url: scope, visibilityState: 'visible' };
    const focused = { url: scope, focused: true };
    expect(pickClient([other, hidden, visible, focused], scope)).toBe(3);
    expect(pickClient([other, hidden, visible], scope)).toBe(2);
    expect(pickClient([other, hidden], scope)).toBe(1);
    expect(pickClient([other], scope)).toBe(-1);
  });
});
