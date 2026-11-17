// The family tree view's model: the server's `FamilyTree` (the lineage's
// recorded days and the species it split into) turned into what the view
// draws. Pure, so it is unit-tested; `ui/familyTree.ts` renders it.
//
//   FamilyTree ──buildFamilyTree──▶ FamilyModel { trunk, branches }
//
// - The trunk is the lineage over time: the founding sample, the days on
//   which its look changed enough to see (`checkpoints`), and today (or the
//   last record, marked extinct, when it died out).
// - Branches are the species its members split into (`origin` split), each
//   hanging off the trunk node it split from (or off its parent branch),
//   with its status: alive (adults), extinct or merged back.
// - Every node carries a design and a body plan for its portrait (the
//   founder avatar, `world/avatar.ts`) and the key changes since the node
//   before it, in the players' trait names ("Ear size ↑", "Stripes
//   appeared").

import { HORN_FORMS, PATTERNS, TAIL_SHAPES, EAR_SHAPES, type Morphology } from './morphology';
import type { BirthSpec, BodyPlanMode, DesignSpec, FamilySample, FamilySpecies, FamilyTree } from './protocol';
import { traitName } from './ui/traitNames';

/** Genes whose change shows on the body (the avatar draws them), in the order changes are listed on a tie. */
export const VISIBLE_GENES = [
  'body_size', 'leg_length', 'large_ears', 'large_eyes', 'tail', 'fangs', 'wings', 'fur', 'scales', 'armor_plates',
  'hooves', 'claws', 'foot_width', 'swimming', 'climbing', 'burrowing', 'muscle', 'fat_stores', 'keen_nose',
  'carnivory', 'camouflage', 'coloration', 'toxins',
] as const;

/** A gene change this large (0..1 scale) is worth a label. */
export const GENE_STEP = 0.08;
/** Axis changes (snout, wing shape) this large, and eye placement in degrees. */
export const AXIS_STEP = 0.15;
export const AZIMUTH_STEP = 20;
/** Horns show from this size (sim `HORNS_SHOW`). */
export const HORNS_MIN = 0.1;
/** Most labels per node. */
export const MAX_LABELS = 3;
/** Most trunk nodes between the founding and today. */
export const MAX_MIDDLE = 4;

export interface Change {
  text: string;
  /** How big: gene and axis steps in 0..1 units, 1 for a new structure or shape. */
  size: number;
}

export interface FamilyNode {
  key: string;
  kind: 'founding' | 'drift' | 'now' | 'extinct' | 'species';
  title: string;
  /** "Day 12 · 30 alive", "Extinct on day 40", … */
  subtitle: string;
  day: number;
  extinct: boolean;
  /** For the portrait (`morphologyOf(design, body)`). */
  design: DesignSpec;
  body: Partial<Morphology>;
  /** The key changes since the node before it, largest first (at most `MAX_LABELS`). */
  changes: string[];
}

export interface FamilyBranch {
  node: FamilyNode;
  species: number;
  /** The trunk node it split from (index into `trunk`), or its parent branch's species. */
  fromTrunk: number;
  fromBranch: number | null;
  depth: number;
}

export interface FamilyModel {
  name: string;
  /** No lineage yet: nothing to draw. */
  empty: boolean;
  /** The lineage died out. */
  extinct: boolean;
  /** The species the trunk belongs to (its founding species), if detected. */
  speciesName: string | null;
  trunk: FamilyNode[];
  branches: FamilyBranch[];
}

const num = (v: unknown, d: number): number => (typeof v === 'number' && Number.isFinite(v) ? v : d);
const gene = (genes: Record<string, number>, g: string): number => num(genes[g], 0);

/** Birth mode gene → design choice (sim `BirthType::from_gene`). */
function birthOf(v: number): BirthSpec {
  return v >= 2 / 3 ? 'live' : v >= 1 / 3 ? 'incubated_eggs' : 'abandoned_eggs';
}

/** A design for a portrait from mean genes: thermoregulation and birth from their genes, every gene as a trait. */
export function designOf(name: string, genes: Record<string, number>): DesignSpec {
  const traits: Record<string, number> = {};
  for (const [k, v] of Object.entries(genes)) if (typeof v === 'number' && Number.isFinite(v)) traits[k] = Math.min(1, Math.max(0, v));
  return {
    name,
    thermo: gene(genes, 'endothermy') >= 0.5 ? 'warm' : 'cold',
    birth: birthOf(num(genes.birth_mode, 0.85)),
    litter_size: 2,
    founders: 10,
    traits,
  };
}

const oneOf = <T extends string>(all: readonly T[], v: unknown, d: T): T => ((all as readonly unknown[]).includes(v) ? (v as T) : d);

/** The body plan for a portrait (the shared sizes come from the genes, `morphologyOf`). */
export function bodyOf(m: BodyPlanMode | undefined): Partial<Morphology> {
  if (!m) return {};
  const ears = num(m.ear_count, 2);
  const legs = num(m.leg_count, 4);
  return {
    eye_count: num(m.eye_count, 2),
    eye_azimuth: num(m.eye_azimuth, 60),
    ear_count: (ears >= 3 ? 4 : ears >= 1 ? 2 : 0) as Morphology['ear_count'],
    ear_shape: oneOf(EAR_SHAPES, m.ear_shape, 'round'),
    leg_count: (Math.max(0, Math.min(8, Math.round(legs / 2) * 2))) as Morphology['leg_count'],
    tail_shape: oneOf(TAIL_SHAPES, m.tail_shape, 'thin'),
    snout_length: num(m.snout_length, 0.5),
    horn_size: num(m.horn_size, 0),
    horn_form: oneOf(HORN_FORMS, m.horn_form, 'curved'),
    wing_aspect: num(m.wing_aspect, 0.5),
    pattern: oneOf(PATTERNS, m.pattern, 'plain'),
  };
}

/** What a node shows, for comparing two of them. */
interface Look {
  genes: Record<string, number>;
  morph?: BodyPlanMode;
}

const COUNT_WORD = ['no', 'one', 'two', 'three', 'four', 'five', 'six', 'seven', 'eight'];
const PATTERN_WORD: Record<string, string> = { spots: 'Spots', stripes: 'Stripes', mottled: 'Mottling' };
const HORN_WORD: Record<string, string> = { curved: 'Curved horns', straight: 'Straight horns', branched: 'Antlers' };
const TAIL_WORD: Record<string, string> = { thin: 'Thin tail', bushy: 'Bushy tail', fin: 'Fin tail', club: 'Club tail', prehensile: 'Grasping tail' };
const EAR_WORD: Record<string, string> = { round: 'Round ears', pointed: 'Pointed ears', long: 'Long ears', fan: 'Fan ears' };

/**
 * The visible changes from `a` to `b`, largest first: new or lost
 * structures and shapes, then gene and axis drifts past their step, named
 * with the players' trait names.
 */
export function changesBetween(a: Look, b: Look): Change[] {
  const out: Change[] = [];
  const ma = a.morph;
  const mb = b.morph;
  if (ma && mb) {
    const pa = ma.pattern ?? 'plain';
    const pb = mb.pattern ?? 'plain';
    if (pa !== pb) {
      if (pb === 'plain') out.push({ text: `${PATTERN_WORD[pa] ?? 'Pattern'} faded`, size: 1 });
      else out.push({ text: `${PATTERN_WORD[pb] ?? 'Pattern'} appeared`, size: 1 });
    }
    const ha = num(ma.horn_size, 0) >= HORNS_MIN;
    const hb = num(mb.horn_size, 0) >= HORNS_MIN;
    if (!ha && hb) out.push({ text: `${HORN_WORD[mb.horn_form] ?? 'Horns'} appeared`, size: 1 });
    else if (ha && !hb) out.push({ text: 'Horns lost', size: 1 });
    else if (ha && hb && ma.horn_form !== mb.horn_form) out.push({ text: HORN_WORD[mb.horn_form] ?? 'New horn shape', size: 1 });
    else if (ha && hb && Math.abs(mb.horn_size - ma.horn_size) >= AXIS_STEP) {
      out.push({ text: `${traitName('horn_size')} ${mb.horn_size > ma.horn_size ? '↑' : '↓'}`, size: Math.abs(mb.horn_size - ma.horn_size) });
    }
    const count = (n: number, what: string) => `${n === 0 ? 'No' : COUNT_WORD[n][0].toUpperCase() + COUNT_WORD[n].slice(1)} ${what}`;
    if (ma.leg_count !== mb.leg_count) out.push({ text: count(mb.leg_count, 'legs'), size: 1 });
    if (ma.eye_count !== mb.eye_count) out.push({ text: count(mb.eye_count, mb.eye_count === 1 ? 'eye' : 'eyes'), size: 1 });
    if (ma.ear_count !== mb.ear_count) out.push({ text: count(mb.ear_count, 'ears'), size: 1 });
    else if (mb.ear_count > 0 && ma.ear_shape !== mb.ear_shape) out.push({ text: EAR_WORD[mb.ear_shape] ?? traitName('ear_shape'), size: 1 });
    if (ma.tail_shape !== mb.tail_shape && gene(b.genes, 'tail') >= 0.05) out.push({ text: TAIL_WORD[mb.tail_shape] ?? traitName('tail_shape'), size: 1 });
    const dAz = num(mb.eye_azimuth, 60) - num(ma.eye_azimuth, 60);
    if (Math.abs(dAz) >= AZIMUTH_STEP) out.push({ text: `Eyes more ${dAz > 0 ? 'to the side' : 'forward'}`, size: Math.abs(dAz) / 180 * 2 });
    const dSn = num(mb.snout_length, 0.5) - num(ma.snout_length, 0.5);
    if (Math.abs(dSn) >= AXIS_STEP) out.push({ text: `${traitName('snout_length')} ${dSn > 0 ? '↑' : '↓'}`, size: Math.abs(dSn) });
    const dW = num(mb.wing_aspect, 0.5) - num(ma.wing_aspect, 0.5);
    if (Math.abs(dW) >= AXIS_STEP && gene(b.genes, 'wings') > 0.25) out.push({ text: `Wings ${dW > 0 ? 'narrower' : 'broader'}`, size: Math.abs(dW) });
  }
  for (const g of VISIBLE_GENES) {
    const d = gene(b.genes, g) - gene(a.genes, g);
    if (Math.abs(d) >= GENE_STEP) out.push({ text: `${traitName(g)} ${d > 0 ? '↑' : '↓'}`, size: Math.abs(d) });
  }
  const warmA = gene(a.genes, 'endothermy') >= 0.5;
  const warmB = gene(b.genes, 'endothermy') >= 0.5;
  if (warmA !== warmB) out.push({ text: warmB ? 'Became warm-blooded' : 'Became cold-blooded', size: 1 });
  // Stable: largest first, ties in the order found.
  return out.map((c, i) => ({ c, i })).sort((x, y) => y.c.size - x.c.size || x.i - y.i).map((x) => x.c);
}

/** Labels for a node: its largest changes, at most `MAX_LABELS`, joined by the view. */
export function changeLabels(a: Look, b: Look): string[] {
  return changesBetween(a, b).slice(0, MAX_LABELS).map((c) => c.text);
}

/** How much changed (discrete changes count 1). */
function changeSize(a: Look, b: Look): number {
  return changesBetween(a, b).reduce((s, c) => s + c.size, 0);
}

/**
 * The trunk's samples: the first, the last, and in between those whose look
 * changed visibly since the previous one kept, at most `max` of them (the
 * largest changes; kept in time order).
 */
export function checkpoints(timeline: readonly FamilySample[], max = MAX_MIDDLE): FamilySample[] {
  if (timeline.length <= 2) return [...timeline];
  const first = timeline[0];
  const last = timeline[timeline.length - 1];
  const picked: { s: FamilySample; size: number; i: number }[] = [];
  let prev = first;
  for (let i = 1; i < timeline.length - 1; i++) {
    const s = timeline[i];
    const size = changeSize(prev, s);
    if (size > 0) {
      picked.push({ s, size, i });
      prev = s;
    }
  }
  const keep = picked.sort((a, b) => b.size - a.size || a.i - b.i).slice(0, max).sort((a, b) => a.i - b.i);
  return [first, ...keep.map((k) => k.s), last];
}

const plural = (n: number, w: string) => `${n} ${w}${n === 1 ? '' : 's'}`;

/** The view's model of a tree (see the module docs). */
export function buildFamilyTree(t: FamilyTree): FamilyModel {
  const name = t.name ?? 'Your lineage';
  const timeline = t.timeline ?? [];
  const extinct = t.lineage !== null && t.alive === 0 && timeline.length > 0;
  const founding = (t.species ?? []).find((s) => s.origin !== 'split' && s.mine > 0) ?? null;
  if (t.lineage === null || timeline.length === 0) {
    return { name, empty: true, extinct: false, speciesName: founding?.name ?? null, trunk: [], branches: [] };
  }
  const samples = checkpoints(timeline);
  const trunk: FamilyNode[] = samples.map((s, i) => {
    const firstNode = i === 0;
    const lastNode = i === samples.length - 1 && i > 0;
    const kind: FamilyNode['kind'] = firstNode ? 'founding' : lastNode ? (extinct ? 'extinct' : 'now') : 'drift';
    const title = firstNode ? 'Founding' : kind === 'now' ? 'Today' : kind === 'extinct' ? 'Last seen' : `Day ${s.day + 1}`;
    const subtitle = kind === 'extinct'
      ? `Day ${s.day + 1} · died out`
      : kind === 'drift'
        ? `${plural(s.alive, 'animal')} alive`
        : `Day ${s.day + 1} · ${plural(s.alive, 'animal')} alive`;
    return {
      key: `t${i}-d${s.day}`,
      kind,
      title,
      subtitle,
      day: s.day,
      extinct: kind === 'extinct',
      design: designOf(name, s.genes),
      body: bodyOf(s.morph),
      changes: firstNode ? [] : changeLabels(samples[i - 1], s),
    };
  });
  // Branches: the species that split, off the trunk node at or before their founding (or off their parent branch).
  const splits = (t.species ?? []).filter((s) => s.origin === 'split');
  const byId = new Map<number, FamilySpecies>(splits.map((s) => [s.id, s]));
  const trunkAt = (day: number): number => {
    let k = 0;
    for (let i = 0; i < samples.length; i++) if (samples[i].day <= day) k = i;
    return k;
  };
  const branches: FamilyBranch[] = [];
  const depthOf = (s: FamilySpecies, seen = new Set<number>()): number => {
    const p = s.parent !== undefined ? byId.get(s.parent) : undefined;
    if (!p || seen.has(p.id)) return 0;
    seen.add(s.id);
    return 1 + depthOf(p, seen);
  };
  for (const s of splits) {
    const parent = s.parent !== undefined ? byId.get(s.parent) : undefined;
    const fromTrunk = trunkAt(s.founded_day);
    const base: Look = parent ? { genes: parent.genes, morph: parent.morph } : samples[fromTrunk];
    const gone = s.extinct_day !== undefined;
    const status = s.merged_into !== undefined && gone
      ? `merged back on day ${s.extinct_day! + 1}`
      : gone
        ? `extinct on day ${s.extinct_day! + 1}`
        : `${plural(Math.round(s.adults), 'adult')}${s.mine > 0 && Math.round(s.mine) < Math.round(s.adults) ? ` (${Math.round(s.mine)} yours)` : ''}`;
    branches.push({
      species: s.id,
      fromTrunk,
      fromBranch: parent ? parent.id : null,
      depth: depthOf(s),
      node: {
        key: `s${s.id}`,
        kind: 'species',
        title: s.name,
        // A species older than the lineage: it joined it (by interbreeding), it did not split from it.
        subtitle: s.founded_day < samples[0].day
          ? `Joined (species since day ${s.founded_day + 1}) · ${status}`
          : `Split on day ${s.founded_day + 1} · ${status}`,
        day: s.founded_day,
        extinct: gone,
        design: designOf(s.name, s.genes),
        body: bodyOf(s.morph),
        changes: changeLabels(base, { genes: s.genes, morph: s.morph }),
      },
    });
  }
  branches.sort((a, b) => a.fromTrunk - b.fromTrunk || a.depth - b.depth || a.node.day - b.node.day || a.species - b.species);
  return { name, empty: false, extinct, speciesName: founding?.name ?? null, trunk, branches };
}

/** The family tree in words (the view's accessible summary). */
export function describeFamily(m: FamilyModel): string {
  if (m.empty) return 'No lineage yet: found one to start its family tree.';
  const parts = [`${m.name}: ${m.trunk.length} stages from its founding`];
  const changed = m.trunk.flatMap((n) => n.changes);
  if (changed.length) parts.push(`changes: ${changed.join(', ')}`);
  if (m.branches.length) {
    const gone = m.branches.filter((b) => b.node.extinct).length;
    parts.push(`${m.branches.length} ${m.branches.length === 1 ? 'branch' : 'branches'} split off${gone ? `, ${gone} extinct` : ''}`);
  }
  if (m.extinct) parts.push('the lineage died out');
  return parts.join('; ') + '.';
}
