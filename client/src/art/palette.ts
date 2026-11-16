// Colour tokens for the creature art (see STYLE.md, "Palette").
//
// Every creature is painted from one `CreaturePalette`: a body tone ramp
// (shade · base · light, plus a darker `far` ramp for the limbs on the far
// side), a belly tone for countershading, a pattern and an accent colour, and
// the fixed material tokens (ink, bone, hoof, eye, nose, inner ear, membrane)
// that every creature shares, so different species still read as one set.
//
// The body hue comes from the world sprite's palette per colouring class
// (`world/creature.ts` `palette`), so the portrait and the map dot agree;
// `vividness` (the coloration gene) then pushes saturation continuously from
// drab to vivid.

/** 0xRRGGBB. */
export type Color = number;

/** A three-step tone ramp: one shadow and one highlight per surface, never more. */
export interface Tones {
  base: Color;
  shade: Color;
  light: Color;
}

export interface CreaturePalette {
  /** Near side of the body. */
  body: Tones;
  /** Far-side limbs, ears and wing: the body ramp pushed into shadow. */
  far: Tones;
  /** Countershaded belly, muzzle and chest. */
  belly: Color;
  /** Markings: blotches, saddle, stripes. */
  pattern: Color;
  /** Display colour: crest, spots, warning bands. */
  accent: Color;
  /** Fixed materials (shared by every creature). */
  ink: Color;
  bone: Color;
  hoof: Color;
  nose: Color;
  inner: Color;
  membrane: Color;
  iris: Color;
}

/** The outline colour: a warm near-black, never pure black. */
export const INK: Color = 0x221a16;
/** Claws, teeth, horns, club knobs. */
export const BONE: Color = 0xf2e8d2;
export const HOOF: Color = 0x3a2e28;
export const NOSE: Color = 0x2e2224;
/** Inside of the ears, mouth: a muted pink. */
export const INNER: Color = 0xd9a0a0;
/** Eye highlight. */
export const GLINT: Color = 0xffffff;
/** Pupils. */
export const PUPIL: Color = 0x120d0b;
/** Warm-blooded irises (dark amber) and cold-blooded ones (gold). */
export const IRIS_WARM: Color = 0x5a3517;
export const IRIS_COLD: Color = 0xd7a12c;

// ------------------------------------------------------------------ colour math

const ch = (c: Color, s: number) => (c >> s) & 255;

export function mix(a: Color, b: Color, t: number): Color {
  const m = (s: number) => Math.round(ch(a, s) + (ch(b, s) - ch(a, s)) * t) << s;
  return m(16) | m(8) | m(0);
}

export function toHsl(c: Color): [number, number, number] {
  const r = ch(c, 16) / 255, g = ch(c, 8) / 255, b = ch(c, 0) / 255;
  const max = Math.max(r, g, b), min = Math.min(r, g, b);
  const l = (max + min) / 2;
  if (max === min) return [0, 0, l];
  const d = max - min;
  const s = l > 0.5 ? d / (2 - max - min) : d / (max + min);
  const h = max === r ? (g - b) / d + (g < b ? 6 : 0) : max === g ? (b - r) / d + 2 : (r - g) / d + 4;
  return [h * 60, s, l];
}

export function fromHsl(h: number, s: number, l: number): Color {
  h = ((h % 360) + 360) % 360;
  s = Math.max(0, Math.min(1, s));
  l = Math.max(0, Math.min(1, l));
  const c = (1 - Math.abs(2 * l - 1)) * s;
  const x = c * (1 - Math.abs(((h / 60) % 2) - 1));
  const m = l - c / 2;
  const [r, g, b] = h < 60 ? [c, x, 0] : h < 120 ? [x, c, 0] : h < 180 ? [0, c, x] : h < 240 ? [0, x, c] : h < 300 ? [x, 0, c] : [c, 0, x];
  const to = (v: number) => Math.round((v + m) * 255);
  return (to(r) << 16) | (to(g) << 8) | to(b);
}

/**
 * Hue-shifted tone steps (STYLE.md): shadows cool down toward violet and lose
 * a little saturation, highlights warm toward yellow. Never plain darken /
 * lighten, which reads as dirty grey.
 */
export function tonesOf(base: Color): Tones {
  const [h, s, l] = toHsl(base);
  const towards = (from: number, to: number, t: number) => from + ((((to - from) % 360) + 540) % 360 - 180) * t;
  return {
    base,
    shade: fromHsl(towards(h, 255, 0.12), s * 0.9 + 0.04, l * 0.68),
    light: fromHsl(towards(h, 55, 0.12), s * 0.95, l + (1 - l) * 0.36),
  };
}

/** A tone ramp pushed into shadow (the far side of the body). */
export function farTones(t: Tones): Tones {
  return tonesOf(mix(t.base, t.shade, 0.55));
}

/** Saturation scaled about the colour's lightness: 0 grey, 1 unchanged, >1 more vivid. */
export function saturate(c: Color, k: number): Color {
  const [h, s, l] = toHsl(c);
  return fromHsl(h, s * k, l);
}

// ------------------------------------------------------------------ creature palette

export interface PaletteInput {
  /** The world palette's colours for the colouring class. */
  world: { base: Color; dark: Color; light: Color; accent: Color };
  coloring: 'drab' | 'camo' | 'bright' | 'warning';
  /** 0..1, the coloration gene: drab → vivid. */
  vividness: number;
  warm: boolean;
}

/** The creature's paint box. */
export function creaturePalette(i: PaletteInput): CreaturePalette {
  // Drab bodies are muted (not grey); vivid ones gain up to 45% saturation.
  const v = Math.max(0, Math.min(1, i.vividness));
  const sat = i.coloring === 'drab' || i.coloring === 'camo' ? 0.82 + 0.4 * v : 0.95 + 0.25 * v;
  const base = saturate(i.world.base, sat);
  const body = tonesOf(base);
  const belly = i.coloring === 'warning' ? mix(base, 0xfff3c4, 0.45) : mix(saturate(i.world.light, sat), 0xf6ead2, 0.35);
  const pattern = i.coloring === 'camo' ? i.world.accent : i.coloring === 'warning' ? 0x231c18 : mix(i.world.dark, body.shade, 0.4);
  const accent = i.coloring === 'warning' ? 0x231c18 : i.coloring === 'bright' ? saturate(i.world.accent, 1 + 0.2 * v) : mix(i.world.accent, i.world.light, 0.2);
  return {
    body,
    far: farTones(body),
    belly,
    pattern,
    accent,
    ink: INK,
    bone: BONE,
    hoof: HOOF,
    nose: NOSE,
    inner: mix(INNER, base, 0.25),
    membrane: mix(body.light, i.coloring === 'bright' ? i.world.accent : INNER, 0.3),
    iris: i.warm ? IRIS_WARM : IRIS_COLD,
  };
}
