//! Offline protection: a softer world for the last members of a lineage
//! whose owner is logged out.
//!
//! The simulation runs whether or not a player is online, so a small
//! lineage could be wiped out by one bad night while its owner sleeps. The
//! game design leaves the rule open ("home ranges or surviving founder
//! groups"); this module implements the *surviving core* form of it:
//!
//! - The caller (the server) says which lineages are protected: those whose
//!   owner is offline, for a capped time after logout. The world never
//!   decides who is online.
//! - Protection only engages while the lineage is down to its last few
//!   members ([`ProtectionRules::floor`] living organisms, agents and cohort
//!   members together). A thriving lineage plays by the normal rules, so
//!   logging out never makes a lineage stronger in the race for legacy; it
//!   only keeps a small one from being wiped out.
//! - While engaged, a strike that would kill a protected organism succeeds
//!   with [`ProtectionRules::predation`] of its usual chance, and starvation
//!   and thirst damage is scaled by [`ProtectionRules::hardship`]. Cohorts
//!   of abstract regions (`lod`) get the same: their exposure to hunting
//!   cohorts and their starvation and thirst hazards are scaled. Everything
//!   else (cold, heat, old age, disease, catastrophes) is unchanged.
//! - Organisms the caller names as exempt (those in an open mating
//!   agreement or on the market: their trip carries risk by design) and
//!   possessed organisms are never protected.
//!
//! Whether a lineage is engaged is decided at the day boundary (and
//! whenever the caller changes the protected set), never mid-tick, so the
//! sequential and parallel apply phases see the same answer.
//!
//! Protection is not in world snapshots (the field is skipped): the server
//! keeps who is offline in its own saved game and sets it again on resume.
//! With nothing protected every rule here multiplies by exactly 1.0 and the
//! simulation is bit-identical to a world without the feature.

use std::collections::{BTreeSet, HashSet};

use super::World;
use crate::organism::{OrgId, Organism};

/// How much protection softens the world, and for how small a lineage.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ProtectionRules {
    /// Protection engages at or below this many living members.
    pub floor: usize,
    /// Factor on a strike's kill chance against a protected organism, and
    /// on a protected cohort's exposure to hunting cohorts.
    pub predation: f32,
    /// Factor on starvation and thirst damage (agents) and hazards (cohorts).
    pub hardship: f32,
}

impl Default for ProtectionRules {
    fn default() -> Self {
        Self { floor: 12, predation: 0.25, hardship: 0.5 }
    }
}

/// The protected lineages and exempt organisms (`World::set_offline_protection`).
#[derive(Clone, Debug, Default)]
pub struct OfflineProtection {
    rules: ProtectionRules,
    lineages: BTreeSet<u32>,
    exempt: HashSet<OrgId>,
    /// Indexed by lineage: protected and at or below the floor.
    engaged: Vec<bool>,
}

impl OfflineProtection {
    pub fn rules(&self) -> ProtectionRules {
        self.rules
    }

    /// Lineages the caller protects (engaged or not).
    pub fn lineages(&self) -> &BTreeSet<u32> {
        &self.lineages
    }

    /// Whether `lineage` is protected and small enough for protection to
    /// apply (as of the last day boundary or change).
    pub fn engaged(&self, lineage: u32) -> bool {
        self.engaged.get(lineage as usize).copied().unwrap_or(false)
    }

    pub fn is_exempt(&self, id: OrgId) -> bool {
        self.exempt.contains(&id)
    }

    fn covers(&self, o: &Organism) -> bool {
        self.engaged(o.lineage) && o.possessed_by.is_none() && !self.exempt.contains(&o.id)
    }
}

impl World {
    /// Protect `lineages` (their owners are offline), except the organisms
    /// in `exempt`. Replaces the previous sets; empty sets turn it off.
    pub fn set_offline_protection(&mut self, lineages: BTreeSet<u32>, exempt: HashSet<OrgId>) {
        self.offline.lineages = lineages;
        self.offline.exempt = exempt;
        self.refresh_offline_protection();
    }

    pub fn set_offline_rules(&mut self, rules: ProtectionRules) {
        self.offline.rules = rules;
        self.refresh_offline_protection();
    }

    pub fn offline_protection(&self) -> &OfflineProtection {
        &self.offline
    }

    /// Decide which protected lineages are small enough for protection to
    /// apply. Called at each day boundary and on every change.
    pub(super) fn refresh_offline_protection(&mut self) {
        let p = &mut self.offline;
        p.engaged.clear();
        if p.lineages.is_empty() {
            return;
        }
        p.engaged.resize(self.lineages.len(), false);
        for &l in &p.lineages {
            let Some(e) = p.engaged.get_mut(l as usize) else { continue };
            let agents = self.lineage_alive.get(l as usize).copied().unwrap_or(0);
            let alive = agents + self.lod.lineage_count(l).alive;
            *e = alive > 0 && alive <= p.rules.floor;
        }
    }

    /// Factor on the kill chance of a strike against `o`.
    pub(super) fn offline_predation_factor(&self, o: &Organism) -> f32 {
        if self.offline.covers(o) {
            self.offline.rules.predation
        } else {
            1.0
        }
    }

    /// Factor on the starvation and thirst damage `o` takes.
    pub(super) fn offline_hardship_factor(&self, o: &Organism) -> f32 {
        if self.offline.covers(o) {
            self.offline.rules.hardship
        } else {
            1.0
        }
    }

    /// Factors (predation, hardship) for a cohort of `lineage`.
    pub(super) fn offline_cohort_factors(&self, lineage: u32) -> (f32, f32) {
        if self.offline.engaged(lineage) {
            (self.offline.rules.predation, self.offline.rules.hardship)
        } else {
            (1.0, 1.0)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::WorldConfig;
    use crate::organism::DeathCause;
    use crate::presets;

    fn world_with_player() -> (World, u32) {
        let config = WorldConfig { seed: 5, width: 64, height: 64, ..WorldConfig::default() };
        let mut w = presets::wild_world(config, 1);
        let design = presets::player_designs().swap_remove(1);
        w.found_lineage(7, &design).unwrap();
        let l = (w.lineages.len() - 1) as u32;
        (w, l)
    }

    #[test]
    fn protection_engages_only_at_or_below_the_floor() {
        let (mut w, l) = world_with_player();
        let alive = w.organisms.iter().filter(|o| o.alive && o.lineage == l).count();
        assert!(alive > 2);
        w.set_offline_rules(ProtectionRules { floor: alive - 1, ..Default::default() });
        w.set_offline_protection(BTreeSet::from([l]), HashSet::new());
        assert!(!w.offline_protection().engaged(l), "above the floor");
        let i = w.organisms.iter().position(|o| o.alive && o.lineage == l).unwrap();
        w.kill(i, DeathCause::Predation);
        // Decided at the day boundary or on a change, not at once.
        assert!(!w.offline_protection().engaged(l));
        w.refresh_offline_protection();
        assert!(w.offline_protection().engaged(l));
        let o = w.organisms.iter().find(|o| o.alive && o.lineage == l).unwrap();
        assert_eq!(w.offline_predation_factor(o), 0.25);
        assert_eq!(w.offline_hardship_factor(o), 0.5);
        assert_eq!(w.offline_cohort_factors(l), (0.25, 0.5));
        // Turned off: everything back to exactly 1.
        w.set_offline_protection(BTreeSet::new(), HashSet::new());
        let o = w.organisms.iter().find(|o| o.alive && o.lineage == l).unwrap();
        assert_eq!(w.offline_predation_factor(o), 1.0);
        assert_eq!(w.offline_cohort_factors(l), (1.0, 1.0));
    }

    #[test]
    fn exempt_and_possessed_organisms_are_not_protected() {
        let (mut w, l) = world_with_player();
        let ids: Vec<OrgId> = w.organisms.iter().filter(|o| o.lineage == l).map(|o| o.id).collect();
        w.set_offline_rules(ProtectionRules { floor: 1000, ..Default::default() });
        w.set_offline_protection(BTreeSet::from([l]), HashSet::from([ids[0]]));
        let f = |w: &World, id| w.offline_predation_factor(w.organism(id).unwrap());
        assert_eq!(f(&w, ids[0]), 1.0, "exempt");
        assert_eq!(f(&w, ids[1]), 0.25);
        w.possess(7, ids[1]).unwrap();
        assert_eq!(f(&w, ids[1]), 1.0, "possessed");
    }

    /// Protected prey isn't killed by hunters while kill chance is cut to
    /// zero; unprotected, the same prey is hunted. Other lineages carry on.
    #[test]
    fn protected_prey_escapes_hunters() {
        let run = |protect: bool| {
            let config = WorldConfig { seed: 5, width: 64, height: 64, ..WorldConfig::default() };
            let mut w = presets::wild_world(config, 1);
            // Lineage 0: the voles, the foxes' staple.
            w.set_offline_rules(ProtectionRules { floor: usize::MAX, predation: 0.0, hardship: 1.0 });
            if protect {
                w.set_offline_protection(BTreeSet::from([0]), HashSet::new());
            }
            for _ in 0..20 * w.config.ticks_per_day {
                w.step();
            }
            w.counters.deaths_by_lineage.get(&(0, DeathCause::Predation)).copied().unwrap_or(0)
        };
        let (open, sheltered) = (run(false), run(true));
        assert!(open > 0, "voles should be hunted");
        assert_eq!(sheltered, 0, "protected voles hunted {sheltered} times");
    }
}
