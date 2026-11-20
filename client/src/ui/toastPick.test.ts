import { describe, expect, it } from 'vitest';
import type { Toast } from '../model';
import { NEWS_MS, OWN_MS, pickToasts, toastAlive } from './toastPick';

const t = (text: string, news = false, at = 0): Toast => ({ kind: 'info', text, at, ...(news ? { news } : {}) });

describe('pickToasts', () => {
  it('keeps the reply to the player\'s own command above the alerts it set off', () => {
    const claim = t('Claimed: Group A. But: no water in it.');
    const list = [claim, t('hungry', true), t('hunted', true), t('thirsty', true)];
    expect(pickToasts(list, 2).map((x) => x.text)).toEqual([claim.text, 'thirsty']);
    expect(pickToasts(list, 3).map((x) => x.text)).toEqual([claim.text, 'hunted', 'thirsty']);
  });
  it('pins every part of a reply posted together, not just its last line', () => {
    const list = [t('Claimed, but: hunger.', false, 1000), t('33 brooding parents stay.', false, 1200), t('outgrown', true, 4000)];
    expect(pickToasts(list, 2).map((x) => x.text)).toEqual(['Claimed, but: hunger.', '33 brooding parents stay.']);
    // An older reply of the player's own is not part of the newest one.
    expect(pickToasts([t('old reply', false, 0), t('n', true, 5000), t('new reply', false, 9000)], 2).map((x) => x.text)).toEqual(['new reply', 'n']);
  });
  it('shows the newest when all are news', () => {
    expect(pickToasts([t('a', true), t('b', true), t('c', true)], 2).map((x) => x.text)).toEqual(['b', 'c']);
  });
  it('lets a reply outlast the news', () => {
    expect(toastAlive(t('n', true, 0), NEWS_MS + 1)).toBe(false);
    expect(toastAlive(t('own', false, 0), NEWS_MS + 1)).toBe(true);
    expect(toastAlive(t('own', false, 0), OWN_MS + 1)).toBe(false);
    // A long alert stays long enough to read (playtest 13).
    const long = t('x'.repeat(230), true, 0);
    expect(toastAlive(long, NEWS_MS + 1)).toBe(true);
    expect(toastAlive(long, 15001)).toBe(false);
  });
});
