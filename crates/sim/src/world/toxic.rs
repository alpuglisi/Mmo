//! Learned avoidance of toxic prey (`Balance::toxic_learning`, version 42;
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
/// A lesson this strong reads as "looks poisonous" to a player.
pub const LOOKS_POISONOUS: f32 = 0.3;
/// Lessons below this are forgotten.
const FORGOTTEN: f32 = 0.02;

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
        lesson * (RECOGNIZE_DRAB + (1.0 - RECOGNIZE_DRAB) * c)
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
        let step = k * LESSON_STEP * (1.0 + COLOR_TEACHES * coloration.clamp(0.0, 1.0));
        let v = self.toxic_lessons.entry((predator, prey)).or_default();
        *v = (*v + step).min(1.0);
    }

    /// Daily: lessons fade ([`LESSON_DAYS`]); faint ones are forgotten.
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
