import { describe, expect, it } from 'vitest';
import type { CurrentSeason, PlayerSeason, RankedSeasonHead, SeasonWinner } from '../protocol';
import {
  categoryLabel, formatValue, hallOfFame, myPlaces, myRecord, ordinal, placementText, seasonClock, seasonKey, winnersLine,
} from './seasons';

function head(season: number, winners: SeasonWinner[], extra: Partial<RankedSeasonHead> = {}): RankedSeasonHead {
  return {
    world_id: 'w', season, first_day: (season - 1) * 60, last_day: season * 60 - 1, length: 60, days: 60,
    ended_tick: 0, ended_unix: 0, players: 3, winners, summary: '', ...extra,
  };
}

const win = (category: SeasonWinner['category'], player: number, name: string, value: number): SeasonWinner => ({ category, player, name, value });

describe('formatting', () => {
  it('writes ordinals', () => {
    expect([1, 2, 3, 4, 11, 12, 13, 21, 22, 23, 101, 111, 112].map(ordinal)).toEqual([
      '1st', '2nd', '3rd', '4th', '11th', '12th', '13th', '21st', '22nd', '23rd', '101st', '111th', '112th',
    ]);
  });

  it('formats each category with its unit', () => {
    expect(formatValue('legacy', 0.1234)).toBe('12.3%');
    expect(formatValue('lineage', 1)).toBe('1 organism');
    expect(formatValue('lineage', 412)).toBe('412 organisms');
    expect(formatValue('species', 2)).toBe('2 species');
    expect(formatValue('market', 30)).toBe('30 renown');
    expect(formatValue('survival', 1)).toBe('1 catastrophe');
    expect(categoryLabel('market')).toBe('Stud market');
    expect(categoryLabel('new_board')).toBe('new board');
    expect(placementText({ category: 'legacy', rank: 2, of: 5, value: 0.05 })).toBe('2nd of 5 · 5.0%');
  });

  it('shows the season clock, 1-based', () => {
    const c: CurrentSeason = {
      world_id: 'w', season: 3, first_day: 120, last_day: 179, length: 60, days: 48, days_left: 12, offset: 0, limit: 5, standings: [],
    };
    expect(seasonClock(c)).toBe('Season 3 · days 121–180 · 12 days left');
    expect(seasonClock({ ...c, days_left: 1 })).toBe('Season 3 · days 121–180 · 1 day left');
    expect(seasonClock({ ...c, days_left: 0 })).toBe('Season 3 · days 121–180 · ends with the next daily record');
    expect(seasonKey(c)).toBe('w/3');
  });
});

describe('standings', () => {
  it('finds my place in each category, out of everyone ranked', () => {
    const standings: CurrentSeason['standings'] = [
      { category: 'legacy', total: 30, rows: [{ player: 2, name: 'bob', rank: 1, value: 0.3 }, { player: 1, name: 'ann', rank: 17, value: 0.01 }] },
      { category: 'market', total: 2, rows: [{ player: 2, name: 'bob', rank: 1, value: 9 }] },
    ];
    expect(myPlaces(standings, 1)).toEqual([
      { category: 'legacy', place: { category: 'legacy', rank: 17, value: 0.01, of: 30 } },
      { category: 'market', place: null },
    ]);
  });

  it('lists the winners in category order, ties joined', () => {
    const h = head(1, [win('market', 1, 'ann', 30), win('legacy', 2, 'bob', 0.25), win('market', 3, 'cat', 30)]);
    expect(winnersLine(h)).toBe('Legacy: bob (25.0%) · Stud market: ann & cat (30 renown)');
    expect(winnersLine(head(2, []))).toBe('');
  });
});

describe('hall of fame', () => {
  it('ranks players by legacy crowns, then titles, then name', () => {
    const seasons = [
      head(3, [win('legacy', 1, 'ann', 0.2), win('lineage', 2, 'bob', 50)]),
      head(2, [win('legacy', 2, 'bob', 0.2), win('species', 2, 'bob', 1), win('market', 3, 'cat', 5)]),
      head(1, [win('legacy', 1, 'ann', 0.1), win('survival', 4, 'dee', 1)]),
    ];
    expect(hallOfFame(seasons).map((r) => [r.name, r.crowns, r.titles])).toEqual([
      ['ann', 2, 2], ['bob', 1, 3], ['cat', 0, 1], ['dee', 0, 1],
    ]);
    expect(hallOfFame([])).toEqual([]);
  });

  it('sums up my record with my best place per category', () => {
    const history: PlayerSeason[] = [
      { world_id: 'w', season: 3, first_day: 120, last_day: 179, ended_unix: 0, placements: [
        { category: 'legacy', rank: 1, value: 0.3, of: 4 }, { category: 'market', rank: 2, value: 5, of: 2 }] },
      { world_id: 'w', season: 2, first_day: 60, last_day: 119, ended_unix: 0, placements: [
        { category: 'legacy', rank: 3, value: 0.1, of: 4 }, { category: 'lineage', rank: 1, value: 80, of: 4 }] },
      { world_id: 'w', season: 1, first_day: 0, last_day: 59, ended_unix: 0, placements: [
        { category: 'legacy', rank: 1, value: 0.2, of: 4 }] },
    ];
    expect(myRecord(history)).toEqual({
      seasons: 3,
      crowns: 2,
      titles: 3,
      best: [
        { category: 'legacy', rank: 1, season: 1 },
        { category: 'lineage', rank: 1, season: 2 },
        { category: 'market', rank: 2, season: 3 },
      ],
    });
    expect(myRecord([])).toEqual({ seasons: 0, crowns: 0, titles: 0, best: [] });
  });
});
