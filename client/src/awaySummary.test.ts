import { describe, expect, it } from 'vitest';
import { awaySummary, seenKey } from './awaySummary';
import type { FeedEntry } from './model';

const e = (kind: FeedEntry['kind'], tick: number): FeedEntry => ({ kind, text: kind, tick });

describe('awaySummary', () => {
  it('counts the alerts since the last tick seen, most first, without daily summaries', () => {
    const feed = [e('death', 5), e('death', 20), e('death', 30), e('market', 25), e('daily_summary', 40), e('achievement', 41)];
    expect(awaySummary(feed, 10)).toBe('While you were away: 2 death alerts, 1 achievement, 1 market alert. The Genes tab\'s feed lists them.');
  });
  it('says nothing for a first visit or a quiet absence', () => {
    expect(awaySummary([e('death', 20)], 0)).toBeNull();
    expect(awaySummary([e('death', 5), e('daily_summary', 30)], 10)).toBeNull();
  });
  it('keys by world and player', () => {
    expect(seenKey('w1', 3)).toBe('evo.seen.w1.3');
    expect(seenKey('', 3)).toBe('evo.seen.default.3');
  });
});
