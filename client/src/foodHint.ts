// The design editor's founding-size hint (playtest 1, finding 4): founders
// land together, so a big founding grazes out the ground around it within
// days and most of it starves (80+ founders lost about 90% in 20 days,
// worse in summer and drought). Pure, so it is tested.

import { adultMass, geneValue } from './bodyMath';
import type { DesignSpec, WorldSummary } from './protocol';

/** Founders from which the warning shows (and the size the hint recommends at most). */
export const FOUNDERS_WARN = 60;
/** Total founder mass (kg) from which it shows for heavy animals. */
export const BIOMASS_WARN = 900;
/** A comfortable founding size. */
export const FOUNDERS_EASY = 40;

export interface FoodHint {
  level: 'ok' | 'warn';
  text: string;
}

const kg = (x: number) => (x >= 100 ? Math.round(x / 10) * 10 : Math.round(x)).toLocaleString('en-US');

export function foundingHint(d: DesignSpec, world: WorldSummary | null): FoodHint {
  const n = Math.max(0, Math.floor(d.founders));
  const mass = adultMass(d) * n;
  const hunter = geneValue(d, 'carnivory') >= 0.67;
  const dry = (world?.weather.drought ?? 0) >= 0.3;
  const summer = world?.clock.season === 'summer';
  const cap = world?.lineage_cap;
  const big = n >= FOUNDERS_WARN || mass >= BIOMASS_WARN;
  const food = hunter ? 'prey' : 'plants';
  if (!big) {
    return {
      level: 'ok',
      text: `${n} founders, about ${kg(mass)} kg of animals, start together in one spot. The map's Food layer shows where ${hunter ? 'grazing herds (and so prey)' : 'plants'} grow.`
        + (cap ? ` They can grow to the population cap of ${cap.toLocaleString('en-US')}.` : ''),
    };
  }
  const when = [summer ? 'it is summer' : '', dry ? 'there is a drought' : ''].filter(Boolean).join(' and ');
  return {
    level: 'warn',
    text: `${n} founders (about ${kg(mass)} kg of animals) land together and eat the ${food} around them within days: large foundings tend to lose most of their founders to hunger in the first weeks${when ? `, worse now that ${when}` : ''}. `
      + `Around ${FOUNDERS_EASY} founders spread out and grow into the land; points saved on founders buy traits.`,
  };
}
