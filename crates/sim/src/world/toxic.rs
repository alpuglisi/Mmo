//! Learned avoidance of toxic prey (`Balance::toxic_learning`, version 44;
//! *Playtest 2: predators, toxins and drowning* in the README).
//!
//! Playtest 2's winning strategy was a drab, toxic, burrowing egg swarm:
//! toxins poisoned every predator that bit it, and nothing told the
//! predators (or their players) which prey was toxic. Now each predator
//! lineage learns: every poisoning raises its lesson about the prey's
//! lineage, warning colours teach faster than a drab coat (an honest
//! signal pays), and hunters pass up prey with the lesson's probability as
//! far as they can recognise it: a drab coat looks like any other prey's,
//! so a drab toxic lineage is still taken most of the time
//! ([`RECOGNIZE_DRAB`]), while a bright one is left alone. A lineage that
//! knows a toxin takes less harm when it does bite ([`TOLERANCE`]). Naive
//! predators are still poisoned; repeated poisoning teaches the lineage.
//! Lessons fade when they stop being renewed, so a lineage that no longer
//! meets the prey forgets it.

use super::World;

/// Lesson gained per poisoning, at `toxic_learning` 1, before the prey's
/// coloration multiplies it (`1 + COLOR_TEACHES × coloration`).
pub const LESSON_STEP: f32 = 0.15;
/// How much more a fully bright prey teaches than a fully drab one.
pub const COLOR_TEACHES: f32 = 2.0;
/// Days for a lesson to fade to 1/e when it isn't renewed.
pub const LESSON_DAYS: f32 = 60.0;
/// How recognisable the drabbest prey is: a lesson about a lineage is
/// acted on at `RECOGNIZE_DRAB + (1 − RECOGNIZE_DRAB) × coloration` of its
/// strength (`World::toxic_avoidance`). A drab coat looks like any other
/// prey's, so drab toxins never buy full protection: the honest signal pays.
pub const RECOGNIZE_DRAB: f32 = 0.3;
/// How much of a poisoning a lineage that knows the toxin is spared
/// (`World::toxin_harm`): it tastes and spits rather than eats.
pub const TOLERANCE: f32 = 0.7;
/// With `Balance::toxic_avoidance` (playtest 4): the drabbest prey's
/// recognisability, so a learned lesson is acted on at
/// `RECOGNIZE_DRAB_STRONG + (1 − RECOGNIZE_DRAB_STRONG) × coloration` (drab
/// 70%, bright at 0.6 coloration 88%, fully bright 100%), and each
/// poisoning teaches twice as much, so two to four bites make the lesson.
/// Before, 65% of hunters still ate prey their lineage knew was toxic and
/// died of it.
pub const RECOGNIZE_DRAB_STRONG: f32 = 0.7;

/// A lesson this strong reads as "looks poisonous" to a player.
pub const LOOKS_POISONOUS: f32 = 0.3;
/// Lessons below this are forgotten.
const FORGOTTEN: f32 = 0.02;
/// Days over which an animal's own aversion to bright prey after a bite
/// halves (`Balance::honest_signals`).
const BRIGHT_AVERSION_HALF_LIFE: f32 = 40.0;
/// Most a starving hunter discounts its avoidance of a toxic or bright prey
/// (`Balance::hungry_hunters`: from hunger 60, to this at 100).
pub(crate) const HUNGRY_HEED: f32 = 0.6;
/// Coloration from which an animal wears warning colours (above it before
/// `Balance::honest_signals`).
pub(crate) const BRIGHT: f32 = 0.6;

impl World {
    /// What predator lineage `predator` has learned about prey lineage
    /// `prey`: the chance (0–1) its hunters pass that prey up.
    pub fn toxic_lesson(&self, predator: u32, prey: u32) -> f32 {
        if self.toxic_lessons.is_empty() {
            return 0.0;
        }
        self.toxic_lessons.get(&(predator, prey)).copied().unwrap_or(0.0)
    }

    /// The chance a hunter of lineage `predator` passes up prey `q`: its
    /// lineage's lesson about `q`'s, as far as `q` is recognisable by its
    /// coloration ([`RECOGNIZE_DRAB`]).
    pub(crate) fn toxic_avoidance(&self, predator: u32, q: &crate::organism::Organism) -> f32 {
        let lesson = self.toxic_lesson(predator, q.lineage);
        if lesson <= 0.0 {
            return 0.0;
        }
        let c = q.pheno[crate::Gene::Coloration].clamp(0.0, 1.0);
        let drab = if self.config.balance.toxic_avoidance > 0.0 { RECOGNIZE_DRAB_STRONG } else { RECOGNIZE_DRAB };
        let avoid = lesson * (drab + (1.0 - drab) * c);
        // An honest signal is not immunity (`Balance::avoid_cap`).
        let cap = self.config.balance.avoid_cap;
        if cap > 0.0 {
            avoid.min(cap)
        } else {
            avoid
        }
    }

    /// Share of a poisoning a hunter of lineage `predator` takes from prey
    /// of lineage `prey`: less as its lineage knows the toxin ([`TOLERANCE`]).
    pub(crate) fn toxin_harm(&self, predator: u32, prey: u32) -> f32 {
        1.0 - TOLERANCE * self.toxic_lesson(predator, prey)
    }

    /// Whether any lineage of `player` has learned that `prey`'s lineage is
    /// poisonous ([`LOOKS_POISONOUS`]).
    pub fn looks_poisonous_to(&self, player: crate::genome::PlayerId, prey: u32) -> bool {
        !self.toxic_lessons.is_empty()
            && self.toxic_lessons.iter().any(|(&(p, q), &v)| {
                q == prey && v >= LOOKS_POISONOUS && self.lineages.get(p as usize).is_some_and(|l| l.owner == player)
            })
    }

    /// A predator of lineage `predator` was poisoned by prey of lineage
    /// `prey` with expressed coloration `coloration`.
    pub(super) fn learn_toxic(&mut self, predator: u32, prey: u32, coloration: f32) {
        let k = self.config.balance.toxic_learning;
        if k <= 0.0 || predator == prey {
            return;
        }
        let fast = if self.config.balance.toxic_avoidance > 0.0 { 2.0 } else { 1.0 };
        let step = fast * k * LESSON_STEP * (1.0 + COLOR_TEACHES * coloration.clamp(0.0, 1.0));
        let v = self.toxic_lessons.entry((predator, prey)).or_default();
        *v = (*v + step).min(1.0);
    }

    /// Daily: lessons fade ([`LESSON_DAYS`]); faint ones are forgotten.
    /// Whether coloration `c` reads as warning colours (from 0.6 with
    /// `Balance::honest_signals`, above it before).
    pub(crate) fn is_bright(&self, c: f32) -> bool {
        if self.config.balance.honest_signals > 0.0 {
            c >= BRIGHT - 1e-4
        } else {
            c > BRIGHT
        }
    }

    /// How readily hunter `o` passes up prey `q` (0 to 1): its own aversion
    /// to bright prey, the warning-colour group benefit where bright animals
    /// are mostly toxic, and its lineage's lesson of `q`'s toxins. A hungry
    /// hunter heeds them less (`Balance::hungry_hunters`); with
    /// `Balance::honest_signals` the hunger and the cap (`Balance::avoid_cap`)
    /// apply to the whole, before only to the lesson (playtest 7: hungry
    /// packs never took bright toxic prey).
    pub(crate) fn hunter_avoidance(&self, o: &crate::organism::Organism, q: &crate::organism::Organism) -> f32 {
        let bal = &self.config.balance;
        let colour = if self.is_bright(q.pheno[crate::Gene::Coloration]) {
            if o.bright_aversion {
                1.0
            } else {
                self.group.benefit(crate::group::GroupTrait::WarningColoration, q.pos)
            }
        } else {
            0.0
        };
        let hunger = ((o.needs.hunger - 60.0) / 40.0).clamp(0.0, 1.0);
        let heed = 1.0 - HUNGRY_HEED * bal.hungry_hunters * hunger;
        if bal.honest_signals > 0.0 {
            // The colour's pull weakens with hunger by `honest_signals` of
            // the lesson's (at 1, as much).
            let colour_heed = 1.0 - HUNGRY_HEED * bal.hungry_hunters * bal.honest_signals.min(1.0) * hunger;
            let avoid = (colour * colour_heed).max(self.toxic_avoidance(o.lineage, q) * heed);
            if bal.avoid_cap > 0.0 {
                avoid.min(bal.avoid_cap)
            } else {
                avoid
            }
        } else {
            colour.max(self.toxic_avoidance(o.lineage, q) * heed)
        }
    }

    /// Daily: animals' own aversions to bright prey fade
    /// (`Balance::honest_signals`, half-life [`BRIGHT_AVERSION_HALF_LIFE`]).
    pub(super) fn fade_bright_aversions(&mut self) {
        if self.config.balance.honest_signals <= 0.0 {
            return;
        }
        let day = self.tick / self.config.ticks_per_day.max(1) as u64;
        let forget = 1.0 - (-std::f32::consts::LN_2 / BRIGHT_AVERSION_HALF_LIFE).exp();
        let seed = self.config.seed;
        for o in self.organisms.iter_mut().filter(|o| o.alive && o.bright_aversion) {
            let mut rng = crate::rng::rng_for(seed, crate::rng::stream::CHRONICLE, day, (1 << 63) | o.id);
            if rand::Rng::gen::<f32>(&mut rng) < forget {
                o.bright_aversion = false;
            }
        }
    }

    pub(super) fn fade_toxic_lessons(&mut self) {
        if self.toxic_lessons.is_empty() {
            return;
        }
        let keep = (-1.0 / LESSON_DAYS).exp();
        self.toxic_lessons.retain(|_, v| {
            *v *= keep;
            *v >= FORGOTTEN
        });
    }
}
