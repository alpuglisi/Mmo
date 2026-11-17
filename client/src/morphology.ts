// The creature's body plan: the client-side mirror of docs/morphology-contract.md
// (v1; the full spec is docs/morphology.md).
//
// One plain value type, its defaults and a clamp that turns anything (a
// half-filled object, a stored draft, numbers from a drag) into a valid
// `Morphology`. Angles are degrees on the head: 0 = straight ahead, +90 =
// right side, ±180 = straight back. Features come in mirrored pairs; an odd
// count puts the extra one on the midline (azimuth 0).

export const EAR_SHAPES = ['round', 'pointed', 'long', 'fan'] as const;
export type EarShape = (typeof EAR_SHAPES)[number];

export const TAIL_SHAPES = ['thin', 'bushy', 'fin', 'club', 'prehensile'] as const;
export type TailShape = (typeof TAIL_SHAPES)[number];

export const HORN_FORMS = ['curved', 'straight', 'branched'] as const;
export type HornForm = (typeof HORN_FORMS)[number];

export const PATTERNS = ['plain', 'spots', 'stripes', 'mottled'] as const;
export type CoatPattern = (typeof PATTERNS)[number];

export const LEG_COUNTS = [0, 2, 4, 6, 8] as const;
export type LegCount = (typeof LEG_COUNTS)[number];

export const EAR_COUNTS = [0, 2, 4] as const;
export type EarCount = (typeof EAR_COUNTS)[number];

export interface Morphology {
  /** 1–6. */
  eye_count: number;
  /** 0–180: the pair angle (0 = both forward, 180 = both straight back). */
  eye_azimuth: number;
  /** 0–1 (the `large_eyes` gene). Field width per eye = 140° + 60° × size. */
  eye_size: number;
  /** 0, 2 or 4 (ears come in pairs). */
  ear_count: EarCount;
  ear_shape: EarShape;
  /** 0–1 (the `large_ears` gene). */
  ear_size: number;
  leg_count: LegCount;
  /** 0–1 (the `leg_length` gene). */
  leg_length: number;
  tail_present: boolean;
  tail_shape: TailShape;
  /** 0–1 (the `tail` gene). */
  tail_length: number;
  /** 0–1. Above 0.5 the snout hides part of the binocular field (`SNOUT_OCCLUSION`). */
  snout_length: number;
  /** 0–1 (the `fangs` gene); no fangs below 0.1. */
  fang_size: number;
  /** 0–1 (Phase 3); no horns below `HORNS_MIN`. */
  horn_size: number;
  horn_form: HornForm;
  /** 0 broad to 1 narrow (only drawn, and only matters, with wings). */
  wing_aspect: number;
  pattern: CoatPattern;
}

export const EYE_COUNT_MIN = 1;
export const EYE_COUNT_MAX = 6;
export const EAR_COUNT_MAX = 4;
/** No eye sits further back than this (contract v1). */
export const EYE_PLACEMENT_MAX = 170;
/** Angle between neighbouring pairs of eyes (contract v1: pair k at ±min(170°, azimuth + 50°·k)). */
export const PAIR_STEP = 50;
/** Degrees of binocular overlap lost per unit of snout above 0.5 (contract v1). */
export const SNOUT_OCCLUSION = 40;
/** Below this the tail gene expresses no tail. */
export const TAIL_PRESENT_MIN = 0.05;
/** Below this there are no fangs. */
export const FANG_MIN = 0.1;
/** Below this there are no horns (sim `morph::HORNS_SHOW`). */
export const HORNS_MIN = 0.1;
/**
 * Today's creature: the new fields at their contract defaults, the shared
 * genes at their design defaults (all 0, so no tail).
 */
export const DEFAULT_MORPHOLOGY: Readonly<Morphology> = Object.freeze({
  eye_count: 2,
  eye_azimuth: 60,
  eye_size: 0,
  ear_count: 2,
  ear_shape: 'round',
  ear_size: 0,
  leg_count: 4,
  leg_length: 0,
  tail_present: false,
  tail_shape: 'thin',
  tail_length: 0,
  snout_length: 0.5,
  fang_size: 0,
  horn_size: 0,
  horn_form: 'curved',
  wing_aspect: 0.5,
  pattern: 'plain',
} satisfies Morphology);

export function defaultMorphology(): Morphology {
  return { ...DEFAULT_MORPHOLOGY };
}

const num = (v: unknown, fallback: number): number => (typeof v === 'number' && Number.isFinite(v) ? v : fallback);
const clamp = (v: number, lo: number, hi: number): number => Math.min(hi, Math.max(lo, v));
const unit = (v: unknown, fallback: number): number => clamp(num(v, fallback), 0, 1);
const oneOf = <T extends string>(all: readonly T[], v: unknown, fallback: T): T =>
  (all as readonly unknown[]).includes(v) ? (v as T) : fallback;

/** The nearest allowed ear count (ties go up: 1 → 2, 3 → 4). */
export function nearestEarCount(v: number): EarCount {
  return EAR_COUNTS[clamp(Math.round(v / 2), 0, EAR_COUNTS.length - 1)];
}

/** The nearest allowed leg count (ties go up: 3 → 4). */
export function nearestLegCount(v: number): LegCount {
  return LEG_COUNTS[clamp(Math.round(v / 2), 0, LEG_COUNTS.length - 1)];
}

/**
 * A valid morphology from anything: missing or non-numeric fields take the
 * defaults (or `base`), numbers are clamped to their ranges, counts rounded,
 * unknown enum values replaced.
 */
export function clampMorphology(input: unknown, base: Readonly<Morphology> = DEFAULT_MORPHOLOGY): Morphology {
  const o = (input && typeof input === 'object' ? input : {}) as Record<string, unknown>;
  return {
    eye_count: clamp(Math.round(num(o.eye_count, base.eye_count)), EYE_COUNT_MIN, EYE_COUNT_MAX),
    eye_azimuth: clamp(num(o.eye_azimuth, base.eye_azimuth), 0, 180),
    eye_size: unit(o.eye_size, base.eye_size),
    ear_count: nearestEarCount(num(o.ear_count, base.ear_count)),
    ear_shape: oneOf(EAR_SHAPES, o.ear_shape, base.ear_shape),
    ear_size: unit(o.ear_size, base.ear_size),
    leg_count: nearestLegCount(num(o.leg_count, base.leg_count)),
    leg_length: unit(o.leg_length, base.leg_length),
    tail_present: typeof o.tail_present === 'boolean' ? o.tail_present : base.tail_present,
    tail_shape: oneOf(TAIL_SHAPES, o.tail_shape, base.tail_shape),
    tail_length: unit(o.tail_length, base.tail_length),
    snout_length: unit(o.snout_length, base.snout_length),
    fang_size: unit(o.fang_size, base.fang_size),
    horn_size: unit(o.horn_size, base.horn_size),
    horn_form: oneOf(HORN_FORMS, o.horn_form, base.horn_form),
    wing_aspect: unit(o.wing_aspect, base.wing_aspect),
    pattern: oneOf(PATTERNS, o.pattern, base.pattern),
  };
}

export function sameMorphology(a: Readonly<Morphology>, b: Readonly<Morphology>): boolean {
  return (Object.keys(DEFAULT_MORPHOLOGY) as (keyof Morphology)[]).every((k) => a[k] === b[k]);
}

/** Field-of-view width of one eye, degrees. */
export function eyeFieldWidth(eyeSize: number): number {
  return 140 + 60 * clamp(eyeSize, 0, 1);
}

export function hasFangs(m: Readonly<Morphology>): boolean {
  return m.fang_size >= FANG_MIN;
}

export function hasHorns(m: Readonly<Pick<Morphology, 'horn_size'>>): boolean {
  return m.horn_size >= HORNS_MIN;
}

/** One placed eye: its signed azimuth and, for a mirrored eye, which pair it belongs to. */
export interface EyePlacement {
  /** Signed degrees, −180…180 (negative = left). */
  azimuth: number;
  /** null for the midline eye; else the pair index (0 = the pair `eye_azimuth` sets). */
  pair: number | null;
  side: 'left' | 'right' | 'mid';
}

/** The (unsigned) azimuth of pair `k` for a pair angle (contract v1). */
export function pairAzimuth(eyeAzimuth: number, k: number): number {
  return Math.min(EYE_PLACEMENT_MAX, clamp(eyeAzimuth, 0, 180) + PAIR_STEP * k);
}

/** Degrees of binocular overlap a snout hides. */
export function snoutOcclusion(snoutLength: number): number {
  return SNOUT_OCCLUSION * Math.max(0, clamp(snoutLength, 0, 1) - 0.5);
}

/**
 * Where the eyes sit: the midline eye (odd counts) first, then each pair as
 * right, left. Pair k sits at ±min(170°, eye_azimuth + 50°·k).
 */
export function eyePlacements(m: Pick<Morphology, 'eye_count' | 'eye_azimuth'>): EyePlacement[] {
  const out: EyePlacement[] = [];
  if (m.eye_count % 2 === 1) out.push({ azimuth: 0, pair: null, side: 'mid' });
  const pairs = Math.floor(m.eye_count / 2);
  for (let i = 0; i < pairs; i++) {
    const a = pairAzimuth(m.eye_azimuth, i);
    out.push({ azimuth: a, pair: i, side: 'right' });
    out.push({ azimuth: -a, pair: i, side: 'left' });
  }
  return out;
}

// --- the existing genes (the design editor's sliders)

/** The genes the morphology shares with today's genome: field → gene name. */
export const GENE_OF: Partial<Record<keyof Morphology, string>> = {
  eye_size: 'large_eyes',
  ear_size: 'large_ears',
  leg_length: 'leg_length',
  tail_length: 'tail',
  fang_size: 'fangs',
};

/**
 * A morphology for a design's trait values: the shared genes fill their
 * fields (`tail` also decides `tail_present`), the rest come from `base`.
 */
export function morphologyFromTraits(traits: Record<string, number>, base: Readonly<Morphology> = DEFAULT_MORPHOLOGY): Morphology {
  const m: Record<string, unknown> = { ...base };
  for (const [field, gene] of Object.entries(GENE_OF)) {
    if (typeof traits[gene!] === 'number') m[field] = traits[gene!];
  }
  if (typeof traits.tail === 'number') m.tail_present = traits.tail >= TAIL_PRESENT_MIN;
  return clampMorphology(m, base);
}

/**
 * The shared genes' values for a morphology (to write back into a design's
 * traits). A tail that is switched off writes 0 to the `tail` gene.
 */
export function traitsFromMorphology(m: Readonly<Morphology>): Record<string, number> {
  const out: Record<string, number> = {};
  for (const [field, gene] of Object.entries(GENE_OF)) out[gene!] = m[field as keyof Morphology] as number;
  if (!m.tail_present) out.tail = 0;
  else out.tail = Math.max(TAIL_PRESENT_MIN, m.tail_length);
  return out;
}
