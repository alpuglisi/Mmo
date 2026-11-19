// The design editor's body plan: how the design's morphology, its trait
// sliders and the server's wire format stay in step (docs/morphology.md,
// sections 6 and 7). Pure, so the sync rules are unit-tested; `ui/design.ts`
// wires them to the sliders, the body builder and the avatar.
//
// One source of truth: an `EditorDesign` is a `DesignSpec` plus its whole
// `morphology`. Five genes are shared (eye, ear, leg, tail and fang size,
// `GENE_OF`): the builder writes them into `traits`, a slider writes its
// field back into `morphology`. The rest of the morphology (counts, shapes,
// placements, snout) travels as `DesignSpec.morph`, and only to a server
// whose `/api/genes` lists a `morph` section; older servers never see it.

import { changedTraits } from './cost';
import {
  clampMorphology,
  DEFAULT_MORPHOLOGY,
  GENE_OF,
  morphologyFromTraits,
  sameMorphology,
  traitsFromMorphology,
  type Morphology,
} from './morphology';
import type { DesignSpec, GeneCatalog, MorphInfo, MorphSpec } from './protocol';
import { DEFAULT_VISION, visionOf, type VisionStats } from './vision';

export interface EditorDesign extends DesignSpec {
  /** The whole body plan; its shared fields mirror `traits`. */
  morphology: Morphology;
}

/** The genes the body plan shares with the sliders. */
export const SHARED_GENES: readonly string[] = Object.values(GENE_OF) as string[];

const MORPH_WIRE: readonly (keyof MorphSpec)[] = [
  'eye_count', 'eye_azimuth', 'ear_count', 'ear_shape', 'leg_count', 'tail_shape', 'snout_length',
  'horn_size', 'horn_form', 'wing_aspect', 'pattern',
];

/**
 * Feature detection: the `morph` rows a server accepts in a design, or null
 * when its `/api/genes` has no `morph` section (it predates morphology; a
 * design must not carry `morph`).
 */
export function morphSupport(cat: Pick<GeneCatalog, 'morph'>): MorphInfo[] | null {
  const rows = cat.morph;
  if (!Array.isArray(rows) || rows.length === 0) return null;
  return rows.filter((r) => r && typeof r.name === 'string');
}

/**
 * The server's structure prices for the builder's add buttons: points per
 * extra eye, leg pair and ear pair (the `morph` rows' costs; the design's
 * total comes from `designCost`, which charges the same rows). Empty for a
 * server without a `morph` section.
 */
export function morphPrices(cat: Pick<GeneCatalog, 'morph'>): { eye?: number; legPair?: number; earPair?: number; hornSize?: number } {
  const rows = morphSupport(cat) ?? [];
  const cost = (name: string) => rows.find((r) => r.name === name)?.cost;
  return { eye: cost('eye_count'), legPair: cost('leg_count'), earPair: cost('ear_count'), hornSize: cost('horn_size') };
}

const sameValue = (a: unknown, b: unknown): boolean =>
  typeof a === 'number' && typeof b === 'number' ? Math.abs(a - b) < 1e-6 : a === b;

/**
 * The `morph` object for a design: every parameter the server lists (and the
 * wire format knows) whose value differs from the server's default. Undefined
 * when the server takes no `morph` or nothing differs, so a default body plan
 * sends exactly what it did before morphology.
 */
export function morphPayload(m: Readonly<Morphology>, cat: Pick<GeneCatalog, 'morph'>): MorphSpec | undefined {
  const rows = morphSupport(cat);
  if (!rows) return undefined;
  const out: Record<string, number | string> = {};
  for (const row of rows) {
    const key = row.name as keyof MorphSpec;
    if (!MORPH_WIRE.includes(key)) continue;
    const v = m[key];
    if (typeof v !== 'number' && typeof v !== 'string') continue;
    if (sameValue(v, row.default ?? DEFAULT_MORPHOLOGY[key])) continue;
    out[key] = v;
  }
  return Object.keys(out).length ? (out as MorphSpec) : undefined;
}

/** What goes to `POST /api/lineage`: changed traits only, and `morph` when the server takes it. */
export function designPayload(d: EditorDesign, cat: GeneCatalog): DesignSpec {
  const out: DesignSpec = {
    name: d.name.trim(),
    thermo: d.thermo,
    birth: d.birth,
    litter_size: d.litter_size,
    traits: changedTraits(d.traits, cat),
    founders: d.founders,
  };
  const morph = morphPayload(d.morphology, cat);
  if (morph) out.morph = morph;
  return out;
}

/** A fresh editor design: every gene at its default, the default body plan. */
/**
 * The starting design's departures from the gene defaults: a sturdy, social,
 * cautious herbivore ready for its first winter, mildly toxic and brightly
 * coloured so hunters learn to leave it alone, foraging a little wider, with
 * 30 founders (`STARTER_FOUNDERS`). It costs about 33 of the 100 points.
 * Round 12: the older starter (no toxins or colours, 20 founders) stalled
 * near 40 animals among rival herds and packs and was retired by every
 * newcomer in playtests 10 and 11; measured with `cargo run --release -p
 * evo-server --example starter_rivals` (docs/starter-design.md). It is a
 * safe first lineage, not the strongest design.
 */
export const STARTER_TRAITS: Readonly<Record<string, number>> = Object.freeze({
  body_size: 0.5, group_tolerance: 0.95, boldness: 0.3, fur: 0.6, fat_stores: 0.6, hibernation: 0.5,
  toxins: 0.5, coloration: 0.6, foraging_range: 0.6,
});

/** The starting design's founders (at least the catalog's free founders). */
export const STARTER_FOUNDERS = 30;

/** A gene's value in the starting design: the starter's, else the catalog default. */
export function starterValue(cat: GeneCatalog, gene: string): number {
  const g = cat.genes.find((x) => x.name === gene);
  return STARTER_TRAITS[gene] ?? g?.default ?? 0;
}

export function newEditorDesign(cat: GeneCatalog): EditorDesign {
  const traits = Object.fromEntries(cat.genes.map((g) => [g.name, starterValue(cat, g.name)]));
  return {
    name: '',
    thermo: 'warm',
    // Incubated eggs, so the starter is its own species: as a live bearer
    // of 3.6 kg it was 100% fertile with the wild Steppe antelope and was
    // absorbed into it (playtest 3). Three to a clutch keeps its growth
    // close to the live bearer's (`examples/starter_survival`).
    birth: 'incubated_eggs',
    litter_size: 3,
    traits,
    founders: Math.max(cat.base_founders, STARTER_FOUNDERS),
    morphology: morphologyFromTraits(traits),
  };
}

/**
 * The builder changed the body plan: take it whole and copy the shared
 * genes into the traits. Returns the genes whose slider must move.
 */
export function applyMorphology(d: EditorDesign, m: Partial<Morphology>): string[] {
  d.morphology = clampMorphology(m, d.morphology);
  const moved: string[] = [];
  for (const [gene, v] of Object.entries(traitsFromMorphology(d.morphology))) {
    if (d.traits[gene] === undefined || !sameValue(d.traits[gene], v)) moved.push(gene);
    d.traits[gene] = v;
  }
  return moved;
}

/**
 * A slider moved: set the trait and, for a shared gene, that one field of
 * the body plan (the rest stays as the builder left it). Returns whether the
 * body plan changed (the builder must follow).
 */
export function applyTrait(d: EditorDesign, gene: string, value: number): boolean {
  d.traits[gene] = value;
  if (!SHARED_GENES.includes(gene)) return false;
  const next = morphologyFromTraits({ [gene]: value }, d.morphology);
  if (sameMorphology(next, d.morphology)) return false;
  d.morphology = next;
  return true;
}

// ------------------------------------------------------------------ readout

export type Trend = 'better' | 'worse' | 'same';

export interface ReadoutRow {
  key: 'coverage' | 'depth' | 'blind';
  label: string;
  /** Display value with its unit ("72%", "20°"). */
  value: string;
  /** Rounded difference from the default body, in display units. */
  delta: number;
  /** "↑8 pts", "↓20°" or "=". */
  arrow: string;
  trend: Trend;
  /** One line on what it means. */
  hint: string;
}

const pct = (x: number) => Math.round(x * 100);
const deg = (x: number) => Math.round(x);

function row(key: ReadoutRow['key'], label: string, now: number, base: number, unit: '%' | '°', higherIsBetter: boolean, hint: string): ReadoutRow {
  const delta = now - base;
  const trend: Trend = delta === 0 ? 'same' : (delta > 0) === higherIsBetter ? 'better' : 'worse';
  const arrow = delta === 0 ? '=' : `${delta > 0 ? '↑' : '↓'}${Math.abs(delta)}${unit === '%' ? ' pts' : '°'}`;
  return { key, label, value: `${now}${unit}`, delta, arrow, trend, hint };
}

/** Where the blind spots are, in a few words ("60° behind, 20° ahead"). */
export function blindSpots(v: VisionStats): string {
  if (v.blindDeg <= 0) return 'none: sees all round';
  const parts: string[] = [];
  if (v.rearBlindDeg > 0) parts.push(`${deg(v.rearBlindDeg)}° behind`);
  if (v.frontBlindDeg > 0) parts.push(`${deg(v.frontBlindDeg)}° ahead`);
  const side = v.blindDeg - v.rearBlindDeg - v.frontBlindDeg;
  if (side > 0.5) parts.push(`${deg(side)}° at the sides`);
  return parts.join(', ');
}

/** The compact vision readout: view, depth and blind spots against the default body. */
export function visionReadout(m: Readonly<Morphology>, base: VisionStats = DEFAULT_VISION): ReadoutRow[] {
  const v = visionOf(m);
  return [
    row('coverage', 'View', pct(v.coverage), pct(base.coverage), '%', true, 'share of the full circle its eyes see'),
    row('depth', 'Depth', deg(v.overlapDeg), deg(base.overlapDeg), '°', true, 'seen by two eyes at once in front: judging distance'),
    row('blind', 'Blind', deg(v.blindDeg), deg(base.blindDeg), '°', false, blindSpots(v)),
  ];
}
