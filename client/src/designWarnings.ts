// The design screen's warnings about the world the founders land in and
// about the design itself (playtest 3, finding 20): a drought at any
// founding size (a 94% drought gave none), the catastrophe under way, and
// wings a heavy body can't use. Pure, so it is tested.

import { adultMass, flight, geneValue } from './bodyMath';
import { DROUGHT_CLEAR, DROUGHT_WARN } from './droughtInfo';
import type { DesignSpec, WorldSummary } from './protocol';
import { catastropheBanner, catastropheEffect, catastropheLabel } from './ui/chronicle';

/** Drought from which the warning says it is extreme. */
export const DROUGHT_EXTREME = 0.75;

export interface DesignWarning {
  id: 'drought' | 'catastrophe' | 'wings';
  level: 'warn' | 'bad';
  text: string;
}

const kg = (x: number) => (x >= 10 ? Math.round(x) : Math.round(x * 10) / 10).toLocaleString('en-US');

export function designWarnings(d: DesignSpec, world: Pick<WorldSummary, 'weather' | 'clock'> | null): DesignWarning[] {
  const out: DesignWarning[] = [];
  const drought = world?.weather.drought ?? 0;
  if (drought >= DROUGHT_CLEAR) {
    const share = Math.round(drought * 100);
    const extreme = drought >= DROUGHT_EXTREME;
    out.push({
      id: 'drought',
      level: extreme || drought >= DROUGHT_WARN ? 'bad' : 'warn',
      text: extreme
        ? `Extreme drought: ${share}% of the land is dry. Founders landing away from lakes and rivers can die of thirst within days, and plants are scarce everywhere. Founding after it eases is safer; if you found now, keep them by water (Food layer on the map).`
        : `Drought: ${share}% of the land is dry. Animals far from lakes and rivers go thirsty and plants thin out, whatever the founding size. Keep the founders near water.`,
    });
  }
  const cat = world?.weather.catastrophe;
  const banner = catastropheBanner(cat, world?.clock.day ?? 0);
  if (cat && banner) {
    const effect = catastropheEffect(cat.kind);
    out.push({
      id: 'catastrophe',
      level: 'bad',
      text: `${catastropheLabel(cat.kind)} under way (${Math.max(0, cat.end_day - (world?.clock.day ?? 0))} days left). ${effect} New founders land in it too; founding after it ends is safer.`,
    });
  }
  const wings = geneValue(d, 'wings');
  if (wings > 0.25) {
    const f = flight(d);
    const mass = kg(adultMass(d));
    if (!f.glide) {
      out.push({ id: 'wings', level: 'warn', text: `Wings do nothing on a body this heavy (adult ≈ ${mass} kg): it can neither fly nor glide, yet the wings cost points and food upkeep. Lower the body size, muscle, fat or armor, or drop the wings.` });
    } else if (wings > 0.5 && !f.fly) {
      out.push({ id: 'wings', level: 'warn', text: `Too heavy to fly (adult ≈ ${mass} kg): these wings only glide. A lighter body flies.` });
    }
  }
  return out;
}

/** The founding ramp, in a sentence (league.rs: 10% on the first day, 100% after 10). */
export const FOUNDING_RAMP_TEXT = 'A new lineage\'s legacy counts gradually toward the ranked season: 10% on its first day, rising to 100% after 10 days (the founding ramp), so dying out or retiring and founding again does not pay.';
