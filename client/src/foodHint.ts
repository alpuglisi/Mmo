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

/** A comfortable total founder mass (kg): the advice for heavy animals. */
export const BIOMASS_EASY = 600;

/** How many founders of this design land comfortably: `FOUNDERS_EASY` heads, fewer for heavy animals. */
export function easyFounders(d: DesignSpec): number {
  const each = adultMass(d);
  return Math.max(5, Math.min(FOUNDERS_EASY, Math.floor(BIOMASS_EASY / Math.max(each, 0.1))));
}

/**
 * How much less food the land offers now: summer and winter thin the plants,
 * a drought more so (playtest 6: the warning only at 70 founders whatever
 * the weather). 1 in a mild spring or autumn.
 */
export function foodScale(world: WorldSummary | null): number {
  const season = world?.clock.season;
  const drought = world?.weather.drought ?? 0;
  const s = season === 'summer' || season === 'winter' ? 0.75 : 1;
  const dry = drought >= 0.6 ? 0.4 : drought >= 0.3 ? 0.6 : 1;
  return s * dry;
}

export function foundingHint(d: DesignSpec, world: WorldSummary | null): FoodHint {
  const n = Math.max(0, Math.floor(d.founders));
  const each = adultMass(d);
  const mass = each * n;
  const hunter = geneValue(d, 'carnivory') >= 0.67;
  const dry = (world?.weather.drought ?? 0) >= 0.3;
  const summer = world?.clock.season === 'summer';
  const soft = world?.lineage_cap;
  const hard = world?.lineage_hard_cap;
  // The thresholds shrink with the food the land offers now.
  const scale = foodScale(world);
  const heads = n >= Math.round(FOUNDERS_WARN * scale);
  const heavy = mass >= BIOMASS_WARN * scale;
  const food = hunter ? 'prey' : 'plants';
  if (!heads && !heavy) {
    const grow = !soft ? ''
      : hard && hard > soft
        ? ` They can grow past ${soft.toLocaleString('en-US')}, where breeding slows to half pace; births stop at ${hard.toLocaleString('en-US')}.`
        : ` They can grow to the population cap of ${soft.toLocaleString('en-US')}.`;
    return {
      level: 'ok',
      text: `${n} founders, about ${kg(mass)} kg of animals, start together in one spot. The map's Food layer shows where ${hunter ? 'grazing herds (and so prey)' : 'plants'} grow.${grow}`,
    };
  }
  const winter = world?.clock.season === 'winter';
  const when = [summer ? 'it is summer' : '', winter ? 'it is winter' : '', dry ? 'there is a drought' : ''].filter(Boolean).join(' and ');
  const easy = Math.max(5, Math.round(easyFounders(d) * scale));
  // The advice follows what made it large: heads, or the weight of big animals.
  const why = heavy && !heads
    ? `These are big animals (about ${kg(each)} kg each): ${n} of them weigh about ${kg(mass)} kg and`
    : `${n} founders (about ${kg(mass)} kg of animals)`;
  const advice = heavy && easy < FOUNDERS_EASY
    ? `At this body size around ${easy} founders (about ${kg(easy * each)} kg) spread out and grow into the land${scale < 1 ? ' now' : ''}`
    : `Around ${easy} founders spread out and grow into the land${scale < 1 ? ' now' : ''}`;
  return {
    level: 'warn',
    text: `${why} land together and eat the ${food} around them within days: large foundings tend to lose most of their founders to hunger in the first weeks${when ? `, worse now that ${when}` : ''}. `
      + `${advice}; points saved on founders buy traits.`,
  };
}
