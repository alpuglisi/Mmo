// The design screen's warnings about the world the founders land in and
// about the design itself (playtest 3, finding 20): a drought at any
// founding size (a 94% drought gave none), the catastrophe under way, and
// wings a heavy body can't use. Pure, so it is tested.

import { adultMass, flight, geneValue } from './bodyMath';
import { DROUGHT_CLEAR } from './droughtInfo';
import type { DesignSpec, WorldSummary } from './protocol';
import { catastropheBanner, catastropheEffect, catastropheLabel } from './ui/chronicle';

/** Drought from which the warning says it is extreme. */
export const DROUGHT_EXTREME = 0.75;

/** Eggs a brooding parent keeps warm (the server's `CLUTCH_COVERED`); the rest of a bigger clutch chill on a cool nest. */
export const EGGS_WARMED = 4;

export interface DesignWarning {
  id: 'drought' | 'catastrophe' | 'wings' | 'eggs' | 'carnivore_eggs' | 'clutch' | 'egg_chill';
  level: 'warn' | 'bad';
  text: string;
}

const kg = (x: number) => (x >= 10 ? Math.round(x) : Math.round(x * 10) / 10).toLocaleString('en-US');

/** Drought from which founding now is warned as dangerous (a founding was wiped out at 52%). */
export const DROUGHT_FOUNDING = 0.4;

/** `serverDrought`: the server's own drought-founding warning, shown verbatim when it sends one. */
export function designWarnings(d: DesignSpec, world: Pick<WorldSummary, 'weather' | 'clock'> | null, serverDrought?: string | null): DesignWarning[] {
  const out: DesignWarning[] = [];
  const drought = world?.weather.drought ?? 0;
  if (serverDrought) {
    out.push({ id: 'drought', level: 'bad', text: serverText(serverDrought) });
  } else if (drought >= DROUGHT_CLEAR) {
    const share = Math.round(drought * 100);
    const extreme = drought >= DROUGHT_EXTREME;
    // From 40% a founding can be wiped out within its grace (playtest 6: 20 founders at 52% all dead in 10 days).
    const severe = drought >= DROUGHT_FOUNDING;
    out.push({
      id: 'drought',
      level: severe ? 'bad' : 'warn',
      text: extreme
        ? `Extreme drought: ${share}% of the land is dry. Founders landing away from lakes and rivers can die of thirst within days, and plants are scarce everywhere. Founding after it eases is safer; if you found now, keep them by water (Food layer on the map).`
        : severe
          ? `Drought: ${share}% of the land is dry. A new lineage founded now can die out within days, however few its founders: those far from lakes and rivers go thirsty and plants are thin. Founding after it eases is safer; if you found now, keep them by water.`
          : `Drought: ${share}% of the land is dry. Animals far from lakes and rivers go thirsty and plants thin out, whatever the founding size. Keep the founders near water.`,
    });
  }
  const cat = world?.weather.catastrophe;
  const today = world?.clock.day ?? 0;
  const left = cat ? cat.end_day - today : 0;
  // Only while it lasts: `end_day` is the first day after it (playtest 4: "1 days left" after it ended).
  // Only once it has begun (playtest 10: a severe winter announced before its first day).
  if (cat && cat.active && left > 0 && today >= (cat.start_day ?? 0) && catastropheBanner(cat, today)) {
    const effect = catastropheEffect(cat.kind);
    const days = left === 1 ? 'its last day' : `${left} days left`;
    out.push({
      id: 'catastrophe',
      level: 'bad',
      text: `${catastropheLabel(cat.kind)} under way (${days}). ${effect} New founders land in it too; founding after it ends is safer.`,
    });
  }
  // Warm-blooded eggs need warmth a parent gives: left alone they chill and
  // die unless the weather is hot (playtest 5: 252 chilled, 0 hatched).
  if (d.thermo === 'warm' && d.birth === 'abandoned_eggs') {
    out.push({
      id: 'eggs',
      level: 'bad',
      text: 'Warm-blooded eggs left unattended chill and die unless the weather is hot: this lineage would hardly ever hatch young. Choose incubated eggs (a parent broods them) or live birth, or make it cold-blooded.',
    });
  }
  // A parent warms a few eggs at a time (playtest 8: litter 8 hatched
  // nothing in ~140 days while litter 4 bred well).
  if (d.birth === 'incubated_eggs' && d.litter_size > EGGS_WARMED) {
    const extra = d.litter_size - EGGS_WARMED;
    out.push({
      id: 'clutch',
      level: 'warn',
      text: `A brooding parent warms ${EGGS_WARMED} eggs at most: of each clutch of ${d.litter_size}, the other ${extra} chill whenever the nest is cool, so about ${EGGS_WARMED} hatch. The extra eggs still cost points; a clutch of ${EGGS_WARMED} or fewer hatches whole.`,
    });
  }
  // Incubated eggs chill in the cold too (playtest 8: 392 lost, spring cold).
  const season = world?.clock.season?.toLowerCase();
  if (d.birth === 'incubated_eggs' && (season === 'winter' || season === 'spring')) {
    out.push({
      id: 'egg_chill',
      level: 'warn',
      text: `Incubated eggs still chill on cold nights: a parent lifts them only so far, so clutches laid in winter, early spring or a cold climate often chill.${d.thermo === 'warm' ? ' Warm-blooded eggs need the most heat; cold-blooded eggs or live birth chill less.' : ' Live birth avoids it.'}`,
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

/**
 * A server warning as shown: verbatim, but without notes meant for the
 * developers ("(playtest 5: 252 chilled, none hatched)") and starting with a
 * capital letter.
 */
export function serverText(t: string): string {
  const clean = t.replace(/\s*\((?:playtest|pt)\s*\d+[^()]*\)/gi, '').replace(/\s{2,}/g, ' ').trim();
  return clean.charAt(0).toUpperCase() + clean.slice(1);
}
