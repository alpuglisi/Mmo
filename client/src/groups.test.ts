import { describe, expect, it } from 'vitest';
import {
  canSplit, claimWait, clearMigrationMsg, groupName, groupObedience, groupOptions, groupOrders, groupsOf, homeRangeGroup, homeRangeNote, memberCounts, protectionLeft, migrateMsgs, parseChoice, rallyLeft, rallyMsg,
  targetGroup, territoryRadius, territoryTap, validChoice, TERRITORY_MIN_RADIUS,
} from './groups';
import type { GroupView, OwnOrganism, StateView } from './protocol';

const g = (id: number, extra: Partial<GroupView> = {}): GroupView => ({ id, members: 6, adults: 4, max_radius: 8, ...extra });

function state(groups: GroupView[] | undefined, orgGroups: (number | undefined)[] = []): StateView {
  const organisms = orgGroups.map((group, i) => ({ id: i + 1, matured: i % 2 === 0, ...(group === undefined ? {} : { group }) }) as unknown as OwnOrganism);
  return { organisms, migration: [5, 6], ...(groups ? { groups } : {}) } as unknown as StateView;
}

describe('groups', () => {
  it('names groups by letter, the main one first', () => {
    expect(groupName(0)).toBe('Group A (main)');
    expect(groupName(1)).toBe('Group B');
    expect(groupName(3)).toBe('Group D');
  });

  it('reads the groups from the state, or makes the main one for older servers', () => {
    expect(groupsOf(null)).toEqual([]);
    const two = [g(0), g(1)];
    expect(groupsOf(state(two))).toBe(two);
    const old = groupsOf(state(undefined, [undefined, undefined, undefined]));
    expect(old).toHaveLength(1);
    expect(old[0]).toMatchObject({ id: 0, members: 3, adults: 2, migration: [5, 6] });
    expect(memberCounts(state([g(0)], [undefined, 1, 1, 0]))).toEqual(new Map([[0, 2], [1, 2]]));
  });

  it('offers every group in the menu and keeps the choice valid', () => {
    expect(groupOptions([g(0)])).toEqual([{ value: 'all', text: 'All' }, { value: '0', text: 'Group A (main) · 6' }]);
    const opts = groupOptions([g(0), g(2, { members: 3 })]);
    expect(opts.map((o) => o.value)).toEqual(['all', '0', '2']);
    expect(opts[0].text).toBe('All groups (9)');
    expect(parseChoice('all')).toBe('all');
    expect(parseChoice('2')).toBe(2);
    expect(parseChoice('7')).toBe('all');
    expect(parseChoice('x')).toBe('all');
    expect(validChoice(2, [g(0), g(2)])).toBe(2);
    expect(validChoice(1, [g(0), g(2)])).toBe('all');
  });

  it('acts on one group: the chosen one, or the only one', () => {
    expect(targetGroup('all', [g(0)])).toBe(0);
    expect(targetGroup('all', [g(0), g(1)])).toBeNull();
    expect(targetGroup(1, [g(0), g(1)])).toBe(1);
    expect(targetGroup(3, [g(0), g(1)])).toBeNull();
  });

  it('sends migration to every group or to one', () => {
    expect(migrateMsgs('all', [g(0), g(2)], 1, 2)).toEqual([{ type: 'migrate', x: 1, y: 2, group: 0 }, { type: 'migrate', x: 1, y: 2, group: 2 }]);
    expect(migrateMsgs('all', [], 1, 2)).toEqual([{ type: 'migrate', x: 1, y: 2, group: 0 }]);
    expect(migrateMsgs(1, [g(0), g(1)], 1, 2)).toEqual([{ type: 'migrate', x: 1, y: 2, group: 1 }]);
    expect(clearMigrationMsg('all')).toEqual({ type: 'clear_migration' });
    expect(clearMigrationMsg(0)).toEqual({ type: 'clear_migration', group: 0 });
  });

  it('draws a territory with two taps, the radius kept within the limits', () => {
    expect(territoryRadius({ x: 0, y: 0 }, { x: 3, y: 4 }, 8)).toBe(5);
    expect(territoryRadius({ x: 0, y: 0 }, { x: 30, y: 40 }, 8)).toBe(8);
    expect(territoryRadius({ x: 0, y: 0 }, { x: 0.5, y: 0 }, 8)).toBe(TERRITORY_MIN_RADIUS);
    const herd = g(1, { max_radius: 9.5 });
    const first = territoryTap(null, herd, { x: 10, y: 12 });
    expect(first.claim).toBeNull();
    expect(first.draft).toEqual({ group: 1, x: 10, y: 12, radius: 9.5 });
    const second = territoryTap(first.draft, herd, { x: 13, y: 16 });
    expect(second.draft).toBeNull();
    expect(second.claim).toEqual({ type: 'claim_territory', group: 1, x: 10, y: 12, radius: 5 });
    // A draft for another group starts over.
    expect(territoryTap(first.draft, g(0), { x: 1, y: 1 }).draft).toMatchObject({ group: 0, x: 1, y: 1 });
  });

  it('describes orders, claim cooldowns and when a split is possible', () => {
    expect(groupOrders(g(0))).toBe('roaming freely');
    expect(groupOrders(g(0, { migration: [4.2, 9.8] }))).toBe('migrating toward 4, 10');
    expect(groupOrders(g(0, { settle: { x: 12.4, y: 7.6, radius: 9, expires_tick: 900 } }))).toBe('at home near 12, 8, where its migration arrived');
    expect(groupOrders(g(0, { territory: { x: 20, y: 30, radius: 6, claimed_tick: 3 } }))).toBe('holding a territory (radius 6.0) at 20, 30');
    const rally = { x: 7.4, y: 8.6, radius: 5, expires_tick: 100 };
    expect(groupOrders(g(0, { rally }))).toBe('rallying at 7, 9');
    expect(rallyLeft(g(0, { rally }), 40)).toBe(60);
    expect(rallyLeft(g(0, { rally }), 140)).toBe(0);
    expect(rallyLeft(g(0), 40)).toBe(0);
    expect(rallyMsg(2, 3, 4)).toEqual({ type: 'rally', group: 2, x: 3, y: 4 });
    expect(groupObedience(g(0))).toBe('');
    expect(groupObedience(g(0, { obedience: 0.5 }))).toBe('heeds orders (obedience 0.50)');
    expect(groupObedience(g(0, { obedience: 0.8 }))).toBe('obedient (obedience 0.80)');
    expect(groupObedience(g(0, { obedience: 0.2 }))).toBe('headstrong (obedience 0.20)');
    expect(groupObedience(g(0, { obedience: 0.5, members: 0 }))).toBe('');
    expect(claimWait(g(0, { claim_ready_tick: 120 }), 100)).toBe(20);
    expect(claimWait(g(0), 100)).toBe(0);
    expect(canSplit(g(0), [g(0)])).toBe(true);
    expect(canSplit(g(0, { members: 1 }), [g(0)])).toBe(false);
    expect(canSplit(g(0), [g(0), g(1), g(2), g(3)])).toBe(false);
  });

  it('names the home range: the territory shielded while the player is offline', () => {
    const territory = { x: 10, y: 12, radius: 6, claimed_tick: 3 };
    const home = g(1, { territory, home_range: true });
    expect(homeRangeGroup([g(0, { territory }), home])).toBe(home);
    expect(homeRangeGroup([g(0, { territory })])).toBeNull();
    // A flag without a territory (an older or inconsistent state) shows nothing.
    expect(homeRangeGroup([g(0, { home_range: true })])).toBeNull();
    expect(homeRangeNote(home)).toMatch(/^Home range: when you log off, members inside this territory are hunted less/);
    expect(homeRangeNote(home)).toContain('Hunger and thirst still bite');
    expect(homeRangeNote(g(0, { territory }))).toBe('');
    expect(homeRangeNote(g(0, { home_range: true }))).toBe('');
    // The protection left, from the server.
    expect(homeRangeNote({ ...home, protection_left_min: 450 })).toMatch(/Protection left: 7 h 30 min \(at most 12 hours in any 24\)\.$/);
    expect(protectionLeft({ ...home, protection_left_min: 720 })).toBe('Protection left: 12 h (at most 12 hours in any 24).');
    expect(protectionLeft({ ...home, protection_left_min: 5 })).toBe('Protection left: 5 min (at most 12 hours in any 24).');
    expect(protectionLeft({ ...home, protection_left_min: 0 })).toMatch(/^Protection used up for now/);
    expect(protectionLeft(home)).toBe('');
  });
});
