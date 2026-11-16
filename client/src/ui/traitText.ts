// What each design slider means, in play: one line on what the trait is and
// what a low, middle and high setting does, plus a live caption for the
// editor. Written from what the simulation does (crates/sim/src/stats.rs
// `derive`, terrain.rs `traverse`, group.rs, world/seasonal.rs,
// world/decide.rs, mating.rs), not from what the trait sounds like. Keep it
// in step when trait effects change there.
//
// Captions can depend on the whole design (other sliders, warm/cold-blooded,
// birth type, diet): `context` sees the design and the slider's level and
// returns the caption. They only describe: any combination is allowed.

import type { DesignSpec } from '../protocol';

export type Level = 'low' | 'mid' | 'high';

export interface TraitText {
  /** What the slider is: one short sentence. */
  what: string;
  low: string;
  mid: string;
  high: string;
  /** Below `lo` reads as low (default 0.33). */
  lo?: number;
  /** Above `hi` reads as high (default 0.66). */
  hi?: number;
  /**
   * The caption for this design, when the rest of the design changes what
   * the trait does. `base` is the level's own text.
   */
  context?: (d: DesignSpec, level: Level, base: string) => string;
}

/** The design's value for `gene`, or its default if the design leaves it out. */
export function geneValue(d: DesignSpec, gene: string): number {
  return d.traits[gene] ?? DEFAULTS[gene] ?? 0;
}

// Defaults as in crates/sim/src/design.rs `default_value` (the editor always
// fills every gene; this is for partial designs).
const DEFAULTS: Record<string, number> = {
  body_size: 0.3,
  boldness: 0.5,
  aggression: 0.5,
  wanderlust: 0.5,
  group_tolerance: 0.5,
  foraging_range: 0.5,
  pack_instinct: 0.5,
  pickiness: 0.5,
  obedience: 0.5,
  coloration_tolerance: 0.5,
  body_size_tolerance: 0.5,
};

// --- physical properties mirrored from crates/sim/src/stats.rs `derive`

/** Adult mass in kg. */
export function adultMass(d: DesignSpec): number {
  const v = (g: string) => geneValue(d, g);
  return 0.2 * 10 ** (2.5 * v('body_size')) * (1 + 0.3 * v('muscle') + 0.3 * v('fat_stores') + 0.4 * v('armor_plates'));
}

/** Can it fly / glide (stats.rs: wings and wing loading)? */
export function flight(d: DesignSpec): { fly: boolean; glide: boolean } {
  const w = geneValue(d, 'wings');
  const loading = adultMass(d) / (w * 6 + 0.01);
  return { fly: w > 0.5 && loading < 2, glide: w > 0.25 && loading < 6 };
}

/** Grip on ice and slopes (stats.rs `grip`). */
function grip(d: DesignSpec): number {
  return Math.min(1, 0.3 + 0.6 * geneValue(d, 'claws') + 0.2 * geneValue(d, 'climbing'));
}

/** Speed lost on ice (terrain.rs: `0.6 * hoof_slip * (1 - grip)`). */
export function iceSlip(d: DesignSpec): number {
  return 0.6 * geneValue(d, 'hooves') * (1 - grip(d));
}

function buoyancy(d: DesignSpec): number {
  const v = (g: string) => geneValue(d, g);
  return Math.max(0, Math.min(1, 0.4 + 0.5 * v('fat_stores') - 0.5 * v('armor_plates') - 0.2 * v('muscle')));
}

function kg(m: number): string {
  return m < 10 ? `${m.toFixed(1)} kg` : `${Math.round(m)} kg`;
}

const cold = (d: DesignSpec) => d.thermo === 'cold';
/** Seasonal strategies switch on at 0.5 (stats.rs `STRATEGY_THRESHOLD`). */
const STRATEGY = 0.5;
const on = (d: DesignSpec, g: string) => geneValue(d, g) >= STRATEGY;

/** Seasonal strategy: off below 0.5, then works better the higher it is. */
function strategy(what: string, low: string, mid: string, high: string, context?: TraitText['context']): TraitText {
  return { what, low, mid, high, lo: STRATEGY, hi: 0.8, context };
}

export const TRAIT_TEXT: Record<string, TraitText> = {
  // ---------------------------------------------------------------- body
  body_size: {
    what: 'Adult size, from mouse-sized (0.2 kg) to deer-sized (60 kg).',
    low: 'small: quick, stealthy, matures fast; weak and short-lived',
    mid: 'medium: middling strength, defense, speed and lifespan',
    high: 'large: strong, tough, long-lived; slow, hungry, easy to spot',
    context: (d, _l, base) => `${base} (adult ≈ ${kg(adultMass(d))})`,
  },
  muscle: {
    what: 'Muscle mass: strength and speed, paid for in food.',
    low: 'lean: eats little, but weaker and slower',
    mid: 'muscled: stronger and faster, eats more',
    high: 'powerful: strongest and fastest; hungry, heavy, floats poorly',
  },
  fat_stores: {
    what: 'Body fat: energy reserves, warmth and buoyancy.',
    low: 'lean: quick, but small reserves and little warmth',
    mid: 'padded: warmer, more stamina and reserves; a bit slower',
    high: 'fat: big reserves, warm, floats; slow and heavy',
  },

  // ---------------------------------------------------------------- limbs
  leg_length: {
    what: 'Leg length: running speed, stamina and height.',
    low: 'short legs: slow; deep snow and water swamp it',
    mid: 'medium legs: faster, wades shallow water',
    high: 'long legs: fastest and tireless; strides over snow and shallows',
  },
  claws: {
    what: 'Claws: grip, digging and fighting.',
    low: 'no claws: poor grip on ice and slopes',
    mid: 'claws: better grip, stronger and more agile; some upkeep',
    high: 'big claws: sure grip, strong fighter, can dig; food upkeep',
    context: (d, level, base) =>
      level !== 'low' && geneValue(d, 'claws') > 0.5 && geneValue(d, 'group_tolerance') > 0.5
        ? `${base} · can share colony burrows`
        : base,
  },
  hooves: {
    what: 'Hooves: hard feet built for running on firm ground.',
    low: 'soft feet: nimble, good grip',
    mid: 'hard hooves: faster, more stamina; less agile, slips on ice',
    high: 'solid hooves: fast, tireless; clumsy, slips on ice, sinks in snow',
    context: (d, level, base) => {
      if (level === 'low') return base;
      // Claws (and climbing) give the grip that stops hooves slipping.
      if (iceSlip(d) < 0.1) {
        return level === 'mid'
          ? 'hard hooves: faster, more stamina, less agile; claws offset the ice slip'
          : 'solid hooves: fast, tireless, clumsy; claws offset the ice slip';
      }
      return base;
    },
  },
  foot_width: {
    what: 'Foot width: wide paws or webbed feet spread the weight.',
    low: 'narrow feet: heavy bodies sink in snow, mud and sand',
    mid: 'broad feet: firmer on snow and mud, helps swimming',
    high: 'wide webbed feet: walks over snow and marsh, swims better',
  },
  tail: {
    what: 'Tail: balance for quick turns.',
    low: 'no tail: no help with balance',
    mid: 'tail: better balance, more agile',
    // TODO(trait-downsides): the sim agent is giving tails a modest,
    // contextual downside (relative to locomotion); fill in the high text
    // and a `context` rule from its report.
    high: 'long tail: most agile, but [downside TBD]',
  },

  // ---------------------------------------------------------------- locomotion
  swimming: {
    what: 'Swimming: crossing deep and fast water.',
    low: 'non-swimmer: deep water blocks it',
    mid: 'swimmer: crosses calm rivers and lakes',
    high: 'strong swimmer: crosses swollen rivers, quick in water',
    lo: 0.25,
    hi: 0.6,
    context: (d, level, base) => {
      // terrain.rs: swim = swimming + 0.3 × foot width; 0.25 crosses still water.
      const swim = geneValue(d, 'swimming') + 0.3 * geneValue(d, 'foot_width');
      let text = base;
      if (level === 'low' && swim >= 0.25) text = 'paddler: webbed feet get it across calm water';
      if (swim >= 0.25 && buoyancy(d) < 0.15) text += ' · heavy body swims slowly';
      return text;
    },
  },
  wings: {
    what: 'Wings: gliding or flight, limited by body weight.',
    low: 'no wings: ground-bound',
    mid: 'gliding flaps: glides from trees and heights; storms ground it',
    high: 'wings: flies over water, cliffs and predators; costly upkeep',
    lo: 0.25,
    hi: 0.5,
    context: (d, level, base) => {
      const w = geneValue(d, 'wings');
      const f = flight(d);
      if (w <= 0.25) return 'no wings: ground-bound';
      const heavy = `(${kg(adultMass(d))})`;
      if (w > 0.5 && !f.fly) return f.glide ? `too heavy to fly ${heavy}: glides only` : `too heavy to fly or glide ${heavy}`;
      if (!f.glide) return `too heavy to glide ${heavy}`;
      return level === 'high' ? base : 'gliding flaps: glides from trees and heights; storms ground it';
    },
  },
  climbing: {
    what: 'Climbing: cliffs, trees and slopes.',
    low: 'non-climber: cliffs block it, canopy out of reach',
    mid: 'climber: scales cliffs, escapes into the canopy',
    high: 'expert climber: sure-footed, slips through dense forest',
    lo: 0.4,
    hi: 0.7,
  },
  burrowing: {
    what: 'Burrowing: digging a den to hide in.',
    low: 'no burrow: must outrun threats and weather',
    mid: 'shallow digger: shelters only in a shared colony burrow',
    high: 'burrower: hides underground from predators and weather',
    lo: 0.4,
    hi: 0.5,
  },

  // ---------------------------------------------------------------- covering
  fur: {
    what: 'Fur: insulation for warm-blooded bodies.',
    low: 'bare skin: feels the cold, copes with heat',
    mid: 'fur: comfortable in cool weather, slight upkeep',
    high: 'thick fur: warm in deep cold, but overheats in hot weather',
    context: (d, level, base) => {
      if (!cold(d)) return base;
      // stats.rs `fur_mismatch`: fur blocks a cold-blooded body's basking.
      if (level === 'low') return geneValue(d, 'fur') > 0.05 ? 'thin fur: slightly blocks basking' : 'bare skin: basks freely';
      return level === 'mid' ? 'fur blocks basking: cold-blooded body warms slowly' : 'thick fur blocks basking: stays cold and sluggish';
    },
  },
  scales: {
    what: 'Scales: a tough hide against bites and claws.',
    low: 'soft skin: easy to wound',
    mid: 'scales: harder to wound',
    // TODO(trait-downsides): the sim agent is giving scales a modest,
    // contextual downside (relative to thermoregulation and other
    // coverings); fill in the high text and a `context` rule from its report.
    high: 'heavy scales: hard to wound, but [downside TBD]',
  },
  armor_plates: {
    what: 'Armor plates: heavy bony plating.',
    low: 'unarmored: light and quick',
    mid: 'plated: hard to wound; slower, less agile',
    high: 'heavy armor: very tough; slow, clumsy, wide, sinks in water',
  },

  // ---------------------------------------------------------------- head
  fangs: {
    what: 'Fangs: bite strength in fights and hunts.',
    low: 'no fangs',
    mid: 'fangs: stronger bite',
    // TODO(trait-downsides): the sim agent is adding a "no fangs" threshold
    // and a contextual downside for long fangs (worse plant feeding, in
    // proportion to how much the creature eats plants). Set `lo` to the
    // sim's threshold, fill in the high text and add a `context` rule
    // (carnivory) from its report.
    high: 'long fangs: strongest bite, but [downside TBD]',
    lo: 0.1,
  },
  large_eyes: {
    what: 'Large eyes: spotting threats, food and mates from afar.',
    low: 'small eyes: spots danger late',
    mid: 'sharp eyes: sees threats and food sooner; some upkeep',
    high: 'huge eyes: sees far; costly to run (more food)',
  },
  large_ears: {
    what: 'Large ears: hearing, and shedding body heat.',
    low: 'small ears: hears danger late, keeps heat in',
    mid: 'large ears: hears sooner, sheds some heat',
    high: 'huge ears: hears far, copes with heat, loses heat in cold',
    context: (d, _l, base) =>
      geneValue(d, 'large_ears') > 0.5 && geneValue(d, 'vocal_ability') > 0.5 ? `${base} · alarm calls with vocal ability` : base,
  },
  keen_nose: {
    what: 'Keen nose: smelling food and threats.',
    low: 'weak nose: misses what it cannot see',
    mid: 'keen nose: smells food and threats sooner; some upkeep',
    high: 'superb nose: smells far; costly to run (more food)',
  },

  // ---------------------------------------------------------------- other
  carnivory: {
    what: 'Diet: plant eater (0) to meat eater (1).',
    low: 'herbivore: lives on plants, gets little from meat',
    mid: 'omnivore: eats both, but digests each poorly',
    high: 'carnivore: hunts and scavenges, gets little from plants',
    lo: 0.3,
    hi: 0.7,
  },
  camouflage: {
    what: 'Camouflage: hiding from predators and prey.',
    low: 'conspicuous: easy to spot',
    mid: 'camouflaged: harder to spot, better ambushes',
    high: 'hidden: rarely spotted, ambushes well',
    context: (d, level, base) => {
      if (level === 'low') return base;
      if (geneValue(d, 'coloration') > 0.6) return `${base} · bright colours undo much of it`;
      if (geneValue(d, 'body_size') > 0.66) return `${base} · a big body is hard to hide`;
      return base;
    },
  },
  toxins: {
    what: 'Toxins: poison that punishes whoever bites.',
    low: 'harmless: no chemical defense',
    mid: 'toxic: attacks fail more often and hurt the biter; upkeep',
    high: 'very toxic: most attacks fail; upkeep',
    lo: 0.3,
    hi: 0.5,
    context: (d, level, base) =>
      level === 'high' && geneValue(d, 'coloration') > 0.6 ? `${base} · bright colours warn predators off` : base,
  },
  coloration: {
    what: 'Display colour: what mates judge, at a cost in stealth.',
    low: 'drab: stealthy',
    mid: 'colourful: easier to spot',
    high: 'bright: easy to spot; warns predators only if toxic',
    hi: 0.6,
    context: (d, level, base) => {
      if (level !== 'high') return base;
      return geneValue(d, 'toxins') > 0.5
        ? 'bright and toxic: warning colours, predators learn to avoid'
        : 'bright: easy to spot, and not toxic enough to warn';
    },
  },
  vocal_ability: {
    what: 'Voice: calls that warn others of danger.',
    low: 'quiet: no alarm calls',
    mid: 'some calls: not yet enough for alarm calls',
    high: 'vocal: alarm calls, if ears are large too',
    lo: 0.3,
    hi: 0.5,
    context: (d, level, base) => {
      if (level !== 'high') return base;
      return geneValue(d, 'large_ears') > 0.5
        ? 'vocal: alarm calls warn the group (with enough carriers)'
        : 'vocal, but alarm calls need large ears above 0.5';
    },
  },

  // ---------------------------------------------------------------- instinct
  boldness: {
    what: 'Boldness: how readily it flees a threat.',
    low: 'timid: flees early; safe but often hungry',
    mid: 'steady: flees real threats',
    high: 'bold: keeps foraging near danger; eats more, dies more',
  },
  aggression: {
    what: 'Aggression: defending territory and hunting eagerly.',
    low: 'peaceable: seldom drives off intruders',
    mid: 'assertive: defends its territory when fed',
    high: 'aggressive: drives off intruders, hunts before hungry',
  },
  wanderlust: {
    what: 'Wanderlust: how much and how far it roams.',
    low: 'homebody: stays close, roams ~2 tiles',
    mid: 'roamer: wanders ~6 tiles',
    high: 'wanderer: roams ~10 tiles; spreads genes, finds new ground',
  },
  group_tolerance: {
    what: 'Group tolerance: living close to kin.',
    low: 'solitary: keeps apart from others',
    mid: 'sociable: stays near kin; above 0.5 joins group traits',
    high: 'herd animal: sticks close to kin, can keep sentries',
    hi: 0.6,
  },
  foraging_range: {
    what: 'Foraging range: how far it looks for food first.',
    low: 'close forager: searches ~2 tiles, may strip its patch',
    mid: 'forager: searches ~5 tiles',
    high: 'wide forager: searches ~8 tiles, walks more',
  },
  pack_instinct: {
    what: 'Pack instinct: hunting together.',
    low: 'lone hunter',
    mid: 'some pack sense: not yet enough to hunt together',
    high: 'pack hunter: meat eaters hunt large prey together',
    hi: 0.5,
    context: (d, level, base) =>
      level === 'high' && geneValue(d, 'carnivory') <= 0.6 ? 'pack instinct, but only meat eaters (diet > 0.6) hunt in packs' : base,
  },
  pickiness: {
    what: 'Pickiness: how closely mates must match its taste.',
    low: 'easygoing: mates with almost anyone',
    mid: 'choosy: prefers mates near its taste',
    high: 'picky: only close matches; keeps traits, fewer matings',
  },
  preferred_coloration: {
    what: 'Mate preference: the colour it looks for in a mate.',
    low: 'likes drab mates',
    mid: 'likes moderately coloured mates',
    high: 'likes bright mates: drives showy colours',
  },
  coloration_tolerance: {
    what: 'How far a mate’s colour may stray from its preference.',
    low: 'strict about mate colour',
    mid: 'somewhat flexible about colour',
    high: 'accepts almost any colour',
  },
  preferred_body_size: {
    what: 'Mate preference: the body size it looks for in a mate.',
    low: 'likes small mates',
    mid: 'likes medium-sized mates',
    high: 'likes large mates',
  },
  body_size_tolerance: {
    what: 'How far a mate’s size may stray from its preference.',
    low: 'strict about mate size',
    mid: 'somewhat flexible about size',
    high: 'accepts almost any size',
  },
  vigilance: {
    what: 'Sentry instinct: standing watch for the group.',
    low: 'no sentry instinct',
    mid: 'watchful: not yet enough to stand guard',
    high: 'sentry: takes turns on watch in a group; costs food',
    hi: 0.5,
    context: (d, level, base) =>
      level === 'high' && geneValue(d, 'group_tolerance') <= 0.6 ? 'sentry instinct, but needs group tolerance above 0.6' : base,
  },
  obedience: {
    what: 'Obedience: heeding your group commands over instinct.',
    low: 'headstrong: ignores orders, quick to flee',
    mid: 'obeys orders as tuned',
    high: 'obedient: follows orders even hungry; flees late (tame)',
    lo: 0.4,
    hi: 0.6,
  },

  // ---------------------------------------------------------------- seasonal
  hibernation: strategy(
    'Hibernation: sleeps through winter on stored fat.',
    'off: stays active all winter',
    'hibernates: fattens in autumn, sleeps winter; den can be found',
    'deep hibernator: very cheap winter sleep, no winter breeding',
    (d, level, base) => {
      if (level === 'low') return base;
      if (cold(d)) {
        return on(d, 'brumation')
          ? 'unused: a cold-blooded body brumates instead'
          : 'mismatch: cold-blooded can’t warm itself awake; may never wake';
      }
      return base;
    },
  ),
  brumation: strategy(
    'Brumation: winter dormancy that wakes with the weather.',
    'off: stays active all winter',
    'brumates: dormant while cold, wakes on warm days',
    'deep brumation: cheap dormancy, no winter breeding',
    (d, level, base) => {
      if (level === 'low' || cold(d)) return base;
      return on(d, 'hibernation') ? 'unused: a warm-blooded body hibernates instead' : 'mismatch: a warm-blooded body burns fat while dormant';
    },
  ),
  migration: strategy(
    'Migration: winters further south, returns in spring.',
    'off: stays in its home range all year',
    'migrates: good pasture year-round; risky journeys',
    'migrates (no stronger above 0.5); stamina makes trips safer',
  ),
  estivation: strategy(
    'Estivation: sleeps through summer heat and drought.',
    'off: stays active all summer',
    'estivates: fattens in spring, sleeps through heat; loses summer',
    'deep estivation: cheaper summer sleep',
  ),
  food_caching: strategy(
    'Food caching: hoards plant food for winter.',
    'off: no food stores',
    'caches plants in summer and autumn; others can raid it',
    'big caches: more stored for winter; still raidable',
    (d, level, base) => (level !== 'low' && geneValue(d, 'carnivory') >= 0.7 ? `${base} · plants only: little use to a meat eater` : base),
  ),
  torpor: strategy(
    'Torpor: shallow, cheap rest on cold nights.',
    'off: full metabolism on cold nights',
    'torpor: saves food on cold nights; slow to react',
    'deep torpor: saves more food; slow to react till morning',
  ),
  seasonal_coat: strategy(
    'Seasonal coat: molts into a thick winter coat and back.',
    'off: same coat all year',
    'winter coat: warmer in cold; food per molt, hot in warm spells',
    'thick winter coat: much warmer; food per molt',
    (d, level, base) => (level !== 'low' && cold(d) ? 'useless on a cold-blooded body: the coat blocks basking' : base),
  ),
  timed_breeding: strategy(
    'Timed breeding: breeds only in an early-spring window.',
    'off: breeds any time of year',
    'spring breeder: young get the spring flush; none otherwise',
    'strict spring breeder: better conception in the window',
  ),
};

/** The level a value reads as for this trait. */
export function levelOf(t: TraitText, v: number): Level {
  if (v < (t.lo ?? 0.33)) return 'low';
  if (v > (t.hi ?? 0.66)) return 'high';
  return 'mid';
}

/** The live caption for `gene` in this design (null for an unknown gene). */
export function caption(gene: string, d: DesignSpec): { level: Level; text: string } | null {
  const t = TRAIT_TEXT[gene];
  if (!t) return null;
  const level = levelOf(t, geneValue(d, gene));
  const base = t[level];
  return { level, text: t.context ? t.context(d, level, base) : base };
}
