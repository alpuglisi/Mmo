// The world's palette tokens (see STYLE.md): terrain, water, weather, the
// light of the day, and the in-world markers. One place, so the map, the
// overlays and the legend chips agree.

export type Rgb = [number, number, number];

/** Uniform outline of everything drawn on the map (matches the creatures'). */
export const OUTLINE = 0x15181c;

/** Ground colours (spring, full vegetation, no lighting). */
export const GROUND = {
  grass: [126, 166, 88] as Rgb,
  forest: [58, 112, 70] as Rgb,
  marsh: [92, 124, 88] as Rgb,
  desert: [222, 200, 146] as Rgb,
  tundra: [166, 172, 150] as Rgb,
  mountain: [130, 119, 104] as Rgb,
  /** High rock and peaks. */
  peak: [172, 164, 152] as Rgb,
  /** Beaches under and around lakes. */
  sand: [222, 205, 158] as Rgb,
  /** River banks and dry river beds. */
  bank: [176, 160, 120] as Rgb,
  /** Dried-out lake and marsh floors. */
  mud: [150, 130, 95] as Rgb,
  /** Grazed-out ground (vegetation multiplies toward this). */
  bare: [150, 136, 100] as Rgb,
  /** Parched land in drought. */
  straw: [205, 180, 112] as Rgb,
  /** Fruiting ground: berries, mast and seed heads (a faint tint by the fruit layer). */
  fruit: [176, 74, 96] as Rgb,
};

/** How plant cover looks per season (blended into plant-covered biomes). */
export const SEASON_GREEN: Record<string, Rgb> = {
  spring: [110, 168, 82],
  summer: [150, 168, 76],
  autumn: [190, 132, 64],
  winter: [134, 134, 112],
};

/** Water: the shallows at the shore, open water, the deepest lakes, rivers and flood water. */
export const WATER = {
  shore: [120, 186, 196] as Rgb,
  shallow: [62, 132, 172] as Rgb,
  deep: [30, 74, 128] as Rgb,
  abyss: [22, 54, 104] as Rgb,
  river: [74, 146, 196] as Rgb,
  flood: [104, 138, 158] as Rgb,
  foam: [226, 240, 240] as Rgb,
};

export const ICE: Rgb = [214, 232, 240];
export const SNOW: Rgb = [244, 246, 249];
/** Snow in shade (light comes from the top-left). */
export const SNOW_SHADE: Rgb = [196, 210, 228];

/**
 * Light over the map by hour (multiplied into the terrain): night is a cool
 * moonlit blue, dawn a soft rose, dusk amber, day neutral.
 */
export const LIGHT = {
  night: [128, 142, 188] as Rgb,
  dawn: [236, 206, 196] as Rgb,
  day: [255, 255, 255] as Rgb,
  dusk: [246, 204, 162] as Rgb,
};

/** In-world markers. */
export const MARK = {
  /** Contact shadow under creatures (offset down-right: light from the top-left). */
  shadow: 0x0b0e12,
  shadowAlpha: 0.32,
  select: 0xffffff,
  possessed: 0xff7b72,
  pregnant: 0x7ee787,
  /** The view window's corner brackets. */
  view: 0xe6edf3,
  /** Move orders. */
  order: 0xffd33d,
};

/** Line weights (screen pixels). */
export const LINE = { hair: 1, thin: 1.5, base: 2, bold: 2.5 };

export const toHex = (c: Rgb): number => (Math.round(c[0]) << 16) | (Math.round(c[1]) << 8) | Math.round(c[2]);
export const toCss = (c: Rgb): string => `#${toHex(c).toString(16).padStart(6, '0')}`;
export const mix = (a: Rgb, b: Rgb, t: number): Rgb => [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t];

/** The map light for an hour of the day (0..24, fractional ok), as an RGB multiplier 0..255. */
export function lightAt(hour: number): Rgb {
  const h = ((hour % 24) + 24) % 24;
  // Keyframes: [hour, colour].
  const keys: [number, Rgb][] = [
    [0, LIGHT.night], [4.5, LIGHT.night], [5.75, LIGHT.dawn], [7.5, LIGHT.day],
    [17.5, LIGHT.day], [19, LIGHT.dusk], [20.5, LIGHT.night], [24, LIGHT.night],
  ];
  for (let k = 1; k < keys.length; k++) {
    const [h1, c1] = keys[k];
    const [h0, c0] = keys[k - 1];
    if (h <= h1) {
      const t = (h - h0) / (h1 - h0);
      return mix(c0, c1, t * t * (3 - 2 * t));
    }
  }
  return LIGHT.night;
}
