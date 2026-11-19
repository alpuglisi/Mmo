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
//! **Home ranges.** The design doc's other form: a territory a group
//! claimed (`world::groups`) shields its residents while the owner is away.
//! The caller hands the world at most one [`HomeRange`] per offline player
//! (`World::set_home_ranges`): the circle, the group that holds it and a
//! strength from 0 to 1 (the server fades it over a long absence). The
//! rules, `Balance::home_range`:
//!
//! - **Residents** are the living agents of that player's group standing
//!   inside the circle and, with level of detail on, the lineage's cohort
//!   members of abstract regions whose center lies inside it. Exempt and
//!   possessed organisms are not residents. Who is a resident is decided on
//!   the hour (and on every change), never mid-tick.
//! - **What it softens: predation only.** A strike that would kill a
//!   resident succeeds with `1 − strength × share × (1 − predation)` of its
//!   usual chance (0.5 at full strength by default), and a resident cohort's
//!   exposure to hunting cohorts is scaled the same. Starvation and thirst
//!   are untouched: a range holds the food inside it and no more, so a herd
//!   that outgrows it still starves, and nothing is made immortal.
//! - **Residents share one shield.** Up to `residents` (20) residents get
//!   it in full; above that the same softening is spread over all of them
//!   (`share = residents / count`), so a big herd gains little over a small
//!   one and logging off never wins a race for legacy.
//! - **With the surviving core.** An organism under both forms has its
//!   kill chance scaled by the product of the two factors, but never below
//!   `stack_floor` (0.2) on account of the stacking: either form alone
//!   keeps its own factor.
//!
//! Protection is not in world snapshots (the field is skipped): the server
//! keeps who is offline in its own saved game and sets it again on resume.
//! With nothing protected every rule here multiplies by exactly 1.0 and the
//! simulation is bit-identical to a world without the feature.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use super::World;
use crate::genome::PlayerId;
use crate::math::Vec2;
use crate::organism::{OrgId, Organism};
use crate::terrain::CHUNK;

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

/// A claimed territory that shields its residents while its owner is away
/// (`World::set_home_ranges`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HomeRange {
    pub owner: PlayerId,
    /// The owner's lineage (for its cohorts in abstract regions).
    pub lineage: u32,
    /// The group whose territory it is: only its members are residents.
    pub group: u8,
    pub center: Vec2,
    pub radius: f32,
    /// 0 (no effect) to 1 (full softening).
    pub strength: f32,
}

impl HomeRange {
    pub fn contains(&self, p: Vec2) -> bool {
        self.center.dist(p) <= self.radius
    }
}

/// The protected lineages and exempt organisms (`World::set_offline_protection`)
/// and the shielded home ranges (`World::set_home_ranges`).
#[derive(Clone, Debug, Default)]
pub struct OfflineProtection {
    rules: ProtectionRules,
    lineages: BTreeSet<u32>,
    exempt: HashSet<OrgId>,
    /// Indexed by lineage: protected and at or below the floor.
    engaged: Vec<bool>,
    ranges: Vec<HomeRange>,
    /// Kill-chance factor of each resident agent (as of the last refresh).
    residents: HashMap<OrgId, f32>,
    /// Exposure factor of resident cohorts, by (lineage, region).
    resident_cohorts: HashMap<(u32, usize), f32>,
    /// Residents (agents and cohort members) by owner.
    shielded: BTreeMap<PlayerId, usize>,
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

    /// The shielded home ranges.
    pub fn home_ranges(&self) -> &[HomeRange] {
        &self.ranges
    }

    /// How many residents `owner`'s home range shields (agents and cohort
    /// members, as of the last hour or change); 0 without one.
    pub fn shielded(&self, owner: PlayerId) -> usize {
        self.shielded.get(&owner).copied().unwrap_or(0)
    }

    /// Kill-chance factor the home range gives `o` (1 for a non-resident).
    pub fn home_factor(&self, o: &Organism) -> f32 {
        if self.residents.is_empty() || o.possessed_by.is_some() {
            return 1.0;
        }
        self.residents.get(&o.id).copied().unwrap_or(1.0)
    }
}

/// The two forms' kill-chance factors together: their product, but not
/// below `floor` on account of the stacking (either alone is kept as is).
fn stack(core: f32, home: f32, floor: f32) -> f32 {
    if home == 1.0 {
        core
    } else if core == 1.0 {
        home
    } else {
        (core * home).max(floor.min(core).min(home))
    }
}

impl World {
    /// Protect `lineages` (their owners are offline), except the organisms
    /// in `exempt`. Replaces the previous sets; empty sets turn it off.
    pub fn set_offline_protection(&mut self, lineages: BTreeSet<u32>, exempt: HashSet<OrgId>) {
        self.offline.lineages = lineages;
        self.offline.exempt = exempt;
        self.refresh_offline_protection();
        self.refresh_home_ranges();
    }

    /// Shield `ranges` (at most one per owner: a later one for the same
    /// owner is ignored). Replaces the previous ranges; none turns it off.
    /// Organisms named exempt in `set_offline_protection` are not residents.
    pub fn set_home_ranges(&mut self, ranges: Vec<HomeRange>) {
        let mut owners = BTreeSet::new();
        self.offline.ranges = ranges.into_iter().filter(|r| owners.insert(r.owner)).collect();
        self.refresh_home_ranges();
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

    /// Decide who the home ranges shield and how much. Called on the hour
    /// and on every change.
    pub(super) fn refresh_home_ranges(&mut self) {
        let p = &mut self.offline;
        if p.ranges.is_empty() && p.shielded.is_empty() {
            return;
        }
        p.residents.clear();
        p.resident_cohorts.clear();
        p.shielded.clear();
        let bal = self.config.balance.home_range;
        let lod = self.config.lod.enabled;
        let cx = self.map.chunks_x.max(1);
        for r in &p.ranges {
            if r.strength.is_nan() || r.strength <= 0.0 || r.radius <= 0.0 {
                continue;
            }
            let agents: Vec<OrgId> = (self.organisms.iter())
                .filter(|o| o.alive && o.lineage == r.lineage && o.owner == r.owner && o.group == r.group)
                .filter(|o| o.possessed_by.is_none())
                .filter(|o| !p.exempt.contains(&o.id) && r.contains(o.pos))
                .map(|o| o.id)
                .collect();
            let mut cohorts: Vec<(usize, u32)> = Vec::new();
            if lod {
                let half = CHUNK as f32 / 2.0;
                for (q, cs) in self.lod.regions.iter().enumerate() {
                    let mid = Vec2::new(((q % cx) * CHUNK) as f32 + half, ((q / cx) * CHUNK) as f32 + half);
                    if self.lod.active[q] || !r.contains(mid) {
                        continue;
                    }
                    if let Some(c) = cs.iter().find(|c| c.lineage == r.lineage && c.count() > 0) {
                        cohorts.push((q, c.count()));
                    }
                }
            }
            let count = agents.len() + cohorts.iter().map(|&(_, n)| n as usize).sum::<usize>();
            if count == 0 {
                continue;
            }
            let share = (bal.residents.max(0.0) / count as f32).min(1.0);
            let factor = (1.0 - r.strength.min(1.0) * share * (1.0 - bal.predation)).clamp(0.0, 1.0);
            if factor == 1.0 {
                continue;
            }
            p.residents.extend(agents.into_iter().map(|id| (id, factor)));
            p.resident_cohorts.extend(cohorts.into_iter().map(|(q, _)| ((r.lineage, q), factor)));
            p.shielded.insert(r.owner, count);
        }
    }

    /// Factor on the kill chance of a strike against `o`: the surviving
    /// core's and the home range's together.
    pub(super) fn offline_predation_factor(&self, o: &Organism) -> f32 {
        let core = if self.offline.covers(o) { self.offline.rules.predation } else { 1.0 };
        stack(core, self.offline.home_factor(o), self.config.balance.home_range.stack_floor)
    }

    /// Factor on the chance a raid takes clutch `c`'s eggs
    /// (`Balance::raid_rules`): as a strike on its mother would be, under
    /// the surviving core and her home range (playtest 11: raids ignored
    /// offline protection). 1 without the rule.
    pub(super) fn offline_clutch_factor(&self, c: &super::Clutch) -> f32 {
        if self.config.balance.raid_rules <= 0.0 {
            return 1.0;
        }
        let core = if self.offline.engaged(c.lineage) { self.offline.rules.predation } else { 1.0 };
        let home = self.index.get(&c.mother).map_or(1.0, |&k| self.offline.home_factor(&self.organisms[k]));
        stack(core, home, self.config.balance.home_range.stack_floor)
    }

    /// Factor on the starvation and thirst damage `o` takes.
    pub(super) fn offline_hardship_factor(&self, o: &Organism) -> f32 {
        if self.offline.covers(o) {
            self.offline.rules.hardship
        } else {
            1.0
        }
    }

    /// Factors (predation, hardship) for a cohort of `lineage` under the
    /// surviving core.
    pub(super) fn offline_cohort_factors(&self, lineage: u32) -> (f32, f32) {
        if self.offline.engaged(lineage) {
            (self.offline.rules.predation, self.offline.rules.hardship)
        } else {
            (1.0, 1.0)
        }
    }

    /// Factor on the exposure to hunting cohorts of `lineage`'s cohort in
    /// `region`: the surviving core's and the home range's together.
    pub(super) fn offline_cohort_predation(&self, lineage: u32, region: usize) -> f32 {
        let core = self.offline_cohort_factors(lineage).0;
        let cohorts = &self.offline.resident_cohorts;
        let home = if cohorts.is_empty() { 1.0 } else { cohorts.get(&(lineage, region)).copied().unwrap_or(1.0) };
        stack(core, home, self.config.balance.home_range.stack_floor)
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

    /// Days the hunting tests run: long enough for the wild world's hunters
    /// (a part-fruit-eating fox since the fox took its own niche) to take voles.
    const HUNT_DAYS: u32 = 40;

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
            for _ in 0..HUNT_DAYS * w.config.ticks_per_day {
                w.step();
            }
            w.counters.deaths_by_lineage.get(&(0, DeathCause::Predation)).copied().unwrap_or(0)
        };
        let (open, sheltered) = (run(false), run(true));
        assert!(open > 0, "voles should be hunted");
        assert_eq!(sheltered, 0, "protected voles hunted {sheltered} times");
    }

    /// The player's lineage with its group's territory claimed around its
    /// founders, and the range shielding it at `strength`.
    fn world_with_range(strength: f32) -> (World, u32, HomeRange) {
        let (mut w, l) = world_with_player();
        let members: Vec<Vec2> = w.organisms.iter().filter(|o| o.lineage == l).map(|o| o.pos).collect();
        let center = members.iter().fold(Vec2::ZERO, |a, &p| a + p) * (1.0 / members.len() as f32);
        let t = w.claim_territory(7, 0, center, 24.0).unwrap();
        let range = HomeRange { owner: 7, lineage: l, group: 0, center: t.center, radius: t.radius, strength };
        // Every resident shielded in full (`many_residents_share_one_shield`
        // has the default).
        w.config.balance.home_range.residents = 1000.0;
        w.set_home_ranges(vec![range]);
        (w, l, range)
    }

    #[test]
    fn a_home_range_softens_predation_on_its_residents_only() {
        let (mut w, l, range) = world_with_range(1.0);
        let residents: Vec<OrgId> =
            w.organisms.iter().filter(|o| o.lineage == l && range.contains(o.pos)).map(|o| o.id).collect();
        let n = residents.len();
        assert!(n > 20, "{n} residents");
        assert_eq!(w.offline_protection().shielded(7), n);
        let o = w.organism(residents[0]).unwrap();
        assert_eq!(w.offline_predation_factor(o), 0.5);
        // Predation only: starvation and thirst are untouched.
        assert_eq!(w.offline_hardship_factor(o), 1.0);
        // Other lineages, and members outside the circle, are not residents.
        let stranger = w.organisms.iter().find(|o| o.lineage != l && !range.contains(o.pos)).unwrap();
        assert_eq!(w.offline_predation_factor(stranger), 1.0);
        let i = w.organisms.iter().position(|o| o.id == residents[0]).unwrap();
        w.organisms[i].pos = range.center + Vec2::new(range.radius + 1.0, 0.0);
        w.refresh_home_ranges();
        assert_eq!(w.offline_predation_factor(&w.organisms[i]), 1.0, "outside the range");
        // Half strength, half the softening.
        w.set_home_ranges(vec![HomeRange { strength: 0.5, ..range }]);
        let o = w.organism(residents[1]).unwrap();
        assert_eq!(w.offline_predation_factor(o), 0.75);
        // One range per owner: a second one for the same player is ignored.
        let elsewhere = HomeRange { center: Vec2::new(1.0, 1.0), radius: 1.0, ..range };
        w.set_home_ranges(vec![range, elsewhere]);
        assert_eq!(w.offline_protection().home_ranges(), &[range]);
        // Turned off: everything back to exactly 1.
        w.set_home_ranges(Vec::new());
        assert_eq!(w.offline_predation_factor(w.organism(residents[1]).unwrap()), 1.0);
        assert_eq!(w.offline_protection().shielded(7), 0);
    }

    #[test]
    fn many_residents_share_one_shield() {
        let (mut w, l, range) = world_with_range(1.0);
        let n = w.organisms.iter().filter(|o| o.lineage == l && range.contains(o.pos)).count();
        assert!(n > 20, "{n} residents");
        w.config.balance.home_range.residents = crate::config::HomeRangeBalance::default().residents;
        w.refresh_home_ranges();
        let o = w.organisms.iter().find(|o| o.lineage == l && range.contains(o.pos)).unwrap();
        let expected = 1.0 - (20.0 / n as f32) * 0.5;
        assert!((w.offline_predation_factor(o) - expected).abs() < 1e-6);
    }

    #[test]
    fn exempt_and_possessed_organisms_are_not_residents() {
        let (mut w, l, _) = world_with_range(1.0);
        let ids: Vec<OrgId> = w.organisms.iter().filter(|o| o.lineage == l).map(|o| o.id).collect();
        let f = |w: &World, id| w.offline_predation_factor(w.organism(id).unwrap());
        assert_eq!(f(&w, ids[0]), 0.5);
        // Exempt organisms are named with the surviving core's protection
        // (here for a lineage too big for it to engage).
        w.set_offline_protection(BTreeSet::from([l]), HashSet::from([ids[0]]));
        assert_eq!(f(&w, ids[0]), 1.0, "exempt");
        w.possess(7, ids[1]).unwrap();
        assert_eq!(f(&w, ids[1]), 1.0, "possessed");
    }

    #[test]
    fn the_two_forms_stack_down_to_a_floor() {
        let (mut w, l, _) = world_with_range(1.0);
        let id = w.organisms.iter().find(|o| o.lineage == l).unwrap().id;
        w.set_offline_rules(ProtectionRules { floor: 1000, ..Default::default() });
        w.set_offline_protection(BTreeSet::from([l]), HashSet::new());
        // 0.25 × 0.5 = 0.125, floored at 0.2.
        assert_eq!(w.offline_predation_factor(w.organism(id).unwrap()), 0.2);
        w.config.balance.home_range.stack_floor = 0.0;
        assert_eq!(w.offline_predation_factor(w.organism(id).unwrap()), 0.125);
        // A form whose own factor is below the floor keeps it.
        w.config.balance.home_range.stack_floor = 0.2;
        w.set_offline_rules(ProtectionRules { floor: 1000, predation: 0.1, hardship: 0.5 });
        assert_eq!(w.offline_predation_factor(w.organism(id).unwrap()), 0.1);
        assert_eq!(stack(1.0, 1.0, 0.2), 1.0);
        assert_eq!(stack(0.25, 1.0, 0.2), 0.25);
        assert_eq!(stack(1.0, 0.1, 0.2), 0.1);
    }

    /// Voles shielded by a range over the whole map, with the kill chance
    /// against residents cut to zero, are hunted far less (only the young
    /// born since the hour's refresh are exposed); starvation goes on.
    #[test]
    fn residents_escape_hunters() {
        let run = |shield: bool| {
            let config = WorldConfig { seed: 5, width: 64, height: 64, ..WorldConfig::default() };
            let mut w = presets::wild_world(config, 1);
            w.config.balance.home_range =
                crate::config::HomeRangeBalance { predation: 0.0, residents: 1e9, stack_floor: 0.2 };
            if shield {
                let (owner, group) = (w.lineages[0].owner, 0);
                let r =
                    HomeRange { owner, lineage: 0, group, center: Vec2::new(32.0, 32.0), radius: 100.0, strength: 1.0 };
                w.set_home_ranges(vec![r]);
            }
            for _ in 0..HUNT_DAYS * w.config.ticks_per_day {
                w.step();
            }
            w.counters.deaths_by_lineage.get(&(0, DeathCause::Predation)).copied().unwrap_or(0)
        };
        let (open, sheltered) = (run(false), run(true));
        assert!(open > 0, "voles should be hunted");
        assert!(sheltered * 4 <= open, "shielded voles hunted {sheltered} times against {open}");
    }

    /// With level of detail on, a range shields its lineage's cohorts in the
    /// abstract regions whose center it covers.
    #[test]
    fn a_home_range_covers_cohorts_of_abstract_regions() {
        let mut config = WorldConfig { seed: 5, width: 64, height: 64, ..WorldConfig::default() };
        config.lod.enabled = true;
        let mut w = presets::wild_world(config, 1);
        w.run_days(1);
        let (q, c) = (0..w.lod.regions.len())
            .find_map(|q| w.lod.regions[q].iter().find(|c| c.lineage == 0 && c.count() > 0).map(|c| (q, c.clone())))
            .expect("a vole cohort");
        let cx = w.map.chunks_x;
        let mid = Vec2::new(((q % cx) * CHUNK) as f32 + 8.0, ((q / cx) * CHUNK) as f32 + 8.0);
        let r = HomeRange { owner: c.owner, lineage: 0, group: 0, center: mid, radius: 1.0, strength: 1.0 };
        assert_eq!(w.offline_cohort_predation(0, q), 1.0);
        w.set_home_ranges(vec![r]);
        assert!(w.offline_protection().shielded(c.owner) >= c.count() as usize);
        assert!(w.offline_cohort_predation(0, q) < 1.0);
        assert_eq!(w.offline_cohort_predation(1, q), 1.0, "another lineage");
    }
}
