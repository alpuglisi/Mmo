// The drought notice over the map (playtest 2: droughts killed herds with no
// warning, and phones hide the weather line). The server alerts at the same
// thresholds (`game.rs` DROUGHT_WARN / DROUGHT_CLEAR). Pure, so it is tested.

/** The notice shows from this share of land in drought. */
export const DROUGHT_WARN = 0.45;
/** Once shown, it stays until the share drops below this. */
export const DROUGHT_CLEAR = 0.3;

export interface DroughtNotice {
  on: boolean;
  title: string;
  advice: string;
}

/**
 * Whether the drought notice shows (with hysteresis: `wasOn` is whether it
 * showed last time), and its words. A megadrought catastrophe has its own
 * notice, so this one stays off then.
 */
export function droughtNotice(drought: number, wasOn: boolean, megadrought = false): DroughtNotice {
  const on = !megadrought && (drought >= DROUGHT_WARN || (wasOn && drought >= DROUGHT_CLEAR));
  const share = Math.round(drought * 100);
  return {
    on,
    title: `Drought: ${share}% of the land is dry`,
    advice: 'Animals far from lakes and rivers go thirsty and plants thin out. Keep herds near water; territories and rallies away from water are risky.',
  };
}
