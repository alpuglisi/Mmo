// Player groups and territories (the design doc's group commands: migrate,
// claim or abandon territory, split a group; and rallies): naming, colors,
// what the toolbar's group menu offers, the two-tap territory draw and the
// commands each tool sends. Pure logic, so it is tested without the DOM
// (groups.test.ts); the server's rules are in crates/sim/src/world/groups.rs.

import type { ClientMsg, GroupView, StateView } from './protocol';

/** Groups a player can have at once (`evo_sim::world::groups::MAX_GROUPS`). */
export const MAX_GROUPS = 4;
/** Smallest territory radius in tiles (`TERRITORY_MIN_RADIUS`; smaller claims are rounded up). */
export const TERRITORY_MIN_RADIUS = 3;

/** What the group menu has picked: every group, or one by id. */
export type GroupChoice = 'all' | number;

const LETTERS = 'ABCD';
/** One color per group id (territory fill and outline, the menu swatch). */
export const GROUP_COLORS = [0x3fb950, 0xf0883e, 0xa371f7, 0x39c5cf];

export function groupLetter(id: number): string {
  return LETTERS[id] ?? String(id);
}

/** "Group A (main)", "Group B", … */
export function groupName(id: number): string {
  return id === 0 ? 'Group A (main)' : `Group ${groupLetter(id)}`;
}

export function groupColor(id: number): number {
  return GROUP_COLORS[id % GROUP_COLORS.length];
}

/**
 * The player's groups from a state: the server's list, or (older servers)
 * one main group counted from the organisms.
 */
export function groupsOf(s: StateView | null): GroupView[] {
  if (!s) return [];
  if (s.groups && s.groups.length > 0) return s.groups;
  const adults = s.organisms.filter((o) => o.matured).length;
  return [{ id: 0, members: s.organisms.length, adults, ...(s.migration ? { migration: s.migration } : {}), max_radius: 4 + 2 * Math.sqrt(adults) }];
}

/** A choice still valid for these groups (a merged-away group falls back to all). */
export function validChoice(choice: GroupChoice, groups: GroupView[]): GroupChoice {
  return choice === 'all' || groups.some((g) => g.id === choice) ? choice : 'all';
}

/**
 * The one group a tool acts on: the chosen group, or with "all groups" the
 * main group when it is the only one. `null` when the player must pick one.
 */
export function targetGroup(choice: GroupChoice, groups: GroupView[]): number | null {
  if (choice !== 'all') return groups.some((g) => g.id === choice) ? choice : null;
  return groups.length === 1 ? groups[0].id : null;
}

/** The menu entries: every group first, then each group with its size. */
export function groupOptions(groups: GroupView[]): { value: string; text: string }[] {
  const opts = [{ value: 'all', text: groups.length > 1 ? `All groups (${groups.reduce((n, g) => n + g.members, 0)})` : 'All' }];
  for (const g of groups) opts.push({ value: String(g.id), text: `${groupName(g.id)} · ${g.members}` });
  return opts;
}

export function parseChoice(v: string): GroupChoice {
  if (v === 'all') return 'all';
  const n = Number(v);
  return Number.isInteger(n) && n >= 0 && n < MAX_GROUPS ? n : 'all';
}

/** The migrate command for a choice ("all" moves every group, as the old command did). */
export function migrateMsg(choice: GroupChoice, x: number, y: number): ClientMsg {
  return choice === 'all' ? { type: 'migrate', x, y } : { type: 'migrate', x, y, group: choice };
}

export function clearMigrationMsg(choice: GroupChoice): ClientMsg {
  return choice === 'all' ? { type: 'clear_migration' } : { type: 'clear_migration', group: choice };
}

/** The rally command: `group` gathers at the tapped point. */
export function rallyMsg(group: number, x: number, y: number): ClientMsg {
  return { type: 'rally', group, x, y };
}

/** Ticks until a group's rally lapses (0: no rally). */
export function rallyLeft(g: GroupView, tick: number): number {
  return g.rally ? Math.max(0, g.rally.expires_tick - tick) : 0;
}

/** The territory radius a second tap at `edge` sets around `center`: the distance, kept between the minimum and the group's limit. */
export function territoryRadius(center: { x: number; y: number }, edge: { x: number; y: number }, maxRadius: number): number {
  const d = Math.hypot(edge.x - center.x, edge.y - center.y);
  const max = Math.max(TERRITORY_MIN_RADIUS, maxRadius);
  return Math.min(max, Math.max(TERRITORY_MIN_RADIUS, d));
}

/** A territory being drawn: the first tap set the center; the radius previews the group's limit. */
export interface TerritoryDraft {
  group: number;
  x: number;
  y: number;
  radius: number;
}

/**
 * One tap of the territory tool: the first sets the center (a draft at the
 * group's largest radius), the second the edge, which gives the claim.
 */
export function territoryTap(draft: TerritoryDraft | null, group: GroupView, p: { x: number; y: number }): { draft: TerritoryDraft | null; claim: ClientMsg | null } {
  if (!draft || draft.group !== group.id) {
    return { draft: { group: group.id, x: p.x, y: p.y, radius: Math.max(TERRITORY_MIN_RADIUS, group.max_radius) }, claim: null };
  }
  const radius = Math.round(territoryRadius(draft, p, group.max_radius) * 10) / 10;
  return { draft: null, claim: { type: 'claim_territory', group: group.id, x: draft.x, y: draft.y, radius } };
}

/** Ticks until a group may claim again (0: now). */
export function claimWait(g: GroupView, tick: number): number {
  return Math.max(0, (g.claim_ready_tick ?? 0) - tick);
}

/** Can this group be split (members on both sides, and room for another group)? */
export function canSplit(g: GroupView, groups: GroupView[]): boolean {
  return g.members >= 2 && groups.filter((x) => x.members > 0).length < MAX_GROUPS;
}

/** One line on what a group is doing. */
export function groupOrders(g: GroupView): string {
  if (g.rally) return `rallying at ${g.rally.x.toFixed(0)}, ${g.rally.y.toFixed(0)}`;
  if (g.territory) {
    const t = g.territory;
    return `holding a territory (radius ${t.radius.toFixed(1)}) at ${t.x.toFixed(0)}, ${t.y.toFixed(0)}`;
  }
  if (g.migration) return `migrating toward ${g.migration[0].toFixed(0)}, ${g.migration[1].toFixed(0)}`;
  return 'roaming freely';
}

/**
 * How a group heeds its orders, from its members' mean obedience gene
 * (`Gene::Obedience`; 0.5 obeys as the commands were tuned). Empty when the
 * server doesn't say or the group has no members.
 */
export function groupObedience(g: GroupView): string {
  if (g.obedience === undefined || g.members === 0) return '';
  const o = g.obedience;
  const word = o >= 0.65 ? 'obedient' : o > 0.35 ? 'heeds orders' : 'headstrong';
  return `${word} (obedience ${o.toFixed(2)})`;
}

/** Members of each group among the drawn organisms, by group id. */
export function memberCounts(s: StateView): Map<number, number> {
  const m = new Map<number, number>();
  for (const o of s.organisms) m.set(o.group ?? 0, (m.get(o.group ?? 0) ?? 0) + 1);
  return m;
}
