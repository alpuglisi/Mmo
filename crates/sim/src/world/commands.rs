//! Player levers ("Player controls" in the design doc): directed matings,
//! possession and group commands. The future server validates sessions and
//! consent between players, then calls these.

use std::fmt;

use rand::Rng;

use super::World;
use crate::genome::{Gene, PlayerId};
use crate::mating;
use crate::math::Vec2;
use crate::organism::{OrgId, Sex};
use crate::rng::{rng_for, stream};

/// Organisms must physically meet for a directed mating.
pub const MEETING_DISTANCE: f32 = 1.5;
/// Hunger added to both organisms per unit of override cost.
pub const HUNGER_PER_COST: f32 = 15.0;
/// A possessed organism this close to its move target has arrived: the
/// order is dropped and it goes back to looking after itself.
pub const ORDER_ARRIVAL: f32 = 0.75;
/// Need level a possessed organism that broke off its order to eat or drink
/// (`Balance::possess_urgency`) brings it down to before going back.
pub const ORDER_RESUME: f32 = 30.0;

#[derive(Clone, Debug, PartialEq)]
pub enum CommandError {
    NotFound(OrgId),
    NotOwned(OrgId),
    /// The other organism belongs to another player who hasn't consented.
    NoConsent,
    NotMature(OrgId),
    SameSex,
    AlreadyPregnant,
    TooFar { distance: f32 },
    NoSlotsLeft,
    PossessedByOther(OrgId),
}

impl fmt::Display for CommandError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CommandError::NotFound(id) => write!(f, "organism {id} not found"),
            CommandError::NotOwned(id) => write!(f, "organism {id} is not yours"),
            CommandError::NoConsent => write!(f, "the other player has not consented"),
            CommandError::NotMature(id) => write!(f, "organism {id} is not old enough to breed"),
            CommandError::SameSex => write!(f, "organisms are the same sex"),
            CommandError::AlreadyPregnant => write!(f, "the female is already pregnant"),
            CommandError::TooFar { distance } => {
                write!(f, "organisms are {distance:.1} tiles apart; they must be within {MEETING_DISTANCE}")
            }
            CommandError::NoSlotsLeft => write!(f, "no directed-mating slots left this season"),
            CommandError::PossessedByOther(id) => write!(f, "organism {id} is possessed by another player"),
        }
    }
}

impl std::error::Error for CommandError {}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DirectedMating {
    pub player: PlayerId,
    pub a: OrgId,
    pub b: OrgId,
    /// Set by the server when the other organism's owner agreed (and was paid).
    pub partner_consent: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DirectedOutcome {
    /// Larger of the two organisms' preference mismatches.
    pub mismatch: f32,
    pub cost: f32,
    pub failure_chance: f32,
    /// Fertility multiplier: outbreeding depression times the mates'
    /// infections (`Balance::disease_fertility`).
    pub fertility: f32,
    pub conceived: bool,
}

/// What a directed mating between two organisms would give, computed without
/// running it (no slot used, nothing changes). Lets a player see whether a
/// pair can conceive before asking another player for consent.
#[derive(Clone, Debug, PartialEq)]
pub struct MatingPreview {
    /// As in [`DirectedOutcome`].
    pub mismatch: f32,
    pub cost: f32,
    pub failure_chance: f32,
    /// Fertility the mating would run with: the lower of the two below,
    /// times the mates' infections.
    pub fertility: f32,
    /// [`mating::body_plan_fertility`]: 0 for different body plans.
    pub body_plan_fertility: f32,
    /// [`mating::distance_fertility`] of the whole-genome distance.
    pub distance_fertility: f32,
    /// Body-plan genes whose difference lowers fertility (above
    /// [`mating::BODY_PLAN_FULL`]), with the difference in expressed value.
    pub body_plan_gaps: Vec<(Gene, f32)>,
    /// Tiles between the two organisms (they must meet to mate).
    pub distance: f32,
}

impl World {
    fn live_index(&self, id: OrgId) -> Result<usize, CommandError> {
        self.index
            .get(&id)
            .copied()
            .filter(|&i| self.organisms[i].alive)
            .ok_or(CommandError::NotFound(id))
    }

    pub fn mating_slots_left(&self, player: PlayerId) -> u32 {
        self.config
            .mating_slots_per_season
            .saturating_sub(self.slots_used.get(&player).copied().unwrap_or(0))
    }

    /// Pair two organisms, overriding their own preferences at a cost that
    /// rises steeply outside them: `k * max(0, mismatch - 1)^2`.
    pub fn directed_mating(&mut self, req: DirectedMating) -> Result<DirectedOutcome, CommandError> {
        let (ia, ib) = (self.live_index(req.a)?, self.live_index(req.b)?);
        let (a, b) = (&self.organisms[ia], &self.organisms[ib]);
        if a.owner != req.player && b.owner != req.player {
            return Err(CommandError::NotOwned(req.a));
        }
        if a.owner != b.owner && !req.partner_consent {
            return Err(CommandError::NoConsent);
        }
        for o in [a, b] {
            if !o.matured {
                return Err(CommandError::NotMature(o.id));
            }
        }
        if a.sex == b.sex {
            return Err(CommandError::SameSex);
        }
        let (f, m) = if a.sex == Sex::Female { (ia, ib) } else { (ib, ia) };
        if self.organisms[f].pregnancy.is_some() {
            return Err(CommandError::AlreadyPregnant);
        }
        let distance = a.pos.dist(b.pos);
        if distance > MEETING_DISTANCE {
            return Err(CommandError::TooFar { distance });
        }
        if self.mating_slots_left(req.player) == 0 {
            return Err(CommandError::NoSlotsLeft);
        }
        *self.slots_used.entry(req.player).or_default() += 1;

        let (mismatch, cost, failure_chance, fertility) = self.directed_terms(f, m);

        for k in [f, m] {
            let n = &mut self.organisms[k].needs;
            n.hunger = (n.hunger + cost * HUNGER_PER_COST).min(100.0);
        }
        let mut rng = rng_for(self.config.seed, stream::COMMANDS, self.tick, req.a ^ (req.b << 1));
        // The pair meets whether or not it conceives.
        self.mating_contact(f, m);
        let conceived = fertility > 0.0 && rng.gen::<f32>() >= failure_chance;
        if conceived {
            self.conceive(f, m, fertility);
        }
        self.counters.directed_matings += 1;
        Ok(DirectedOutcome { mismatch, cost, failure_chance, fertility, conceived })
    }

    /// Preference mismatch, override cost, failure chance and fertility of a
    /// directed mating between female `f` and male `m` (indices).
    fn directed_terms(&self, f: usize, m: usize) -> (f32, f32, f32, f32) {
        let (female, male) = (&self.organisms[f], &self.organisms[m]);
        let mismatch = mating::mismatch(&female.pheno, male.display()).max(mating::mismatch(&male.pheno, female.display()));
        let cost = mating::override_cost(mismatch, self.config.override_k);
        let failure_chance = mating::override_failure_chance(cost);
        let fertility = mating::fertility(&female.genome, &male.genome) * self.disease_fertility(f, m);
        (mismatch, cost, failure_chance, fertility)
    }

    /// What [`World::directed_mating`] of `a` and `b` would give right now,
    /// without running it. Needs two living organisms of different sexes;
    /// ownership, maturity, pregnancy and distance are left to the caller.
    pub fn mating_preview(&self, a: OrgId, b: OrgId) -> Result<MatingPreview, CommandError> {
        let (ia, ib) = (self.live_index(a)?, self.live_index(b)?);
        let (oa, ob) = (&self.organisms[ia], &self.organisms[ib]);
        if oa.sex == ob.sex {
            return Err(CommandError::SameSex);
        }
        let (f, m) = if oa.sex == Sex::Female { (ia, ib) } else { (ib, ia) };
        let (mismatch, cost, failure_chance, fertility) = self.directed_terms(f, m);
        let (ga, gb) = (&oa.genome, &ob.genome);
        let body_plan_gaps = mating::BODY_PLAN_GENES
            .iter()
            .map(|&g| (g, (ga.expr(g) - gb.expr(g)).abs()))
            .filter(|&(_, gap)| gap > mating::BODY_PLAN_FULL)
            .collect();
        Ok(MatingPreview {
            mismatch,
            cost,
            failure_chance,
            fertility,
            body_plan_fertility: mating::body_plan_fertility(ga, gb),
            distance_fertility: mating::distance_fertility(ga.distance(gb)),
            body_plan_gaps,
            distance: oa.pos.dist(ob.pos),
        })
    }

    /// Keep an organism from breeding on its own until `until_tick` (it can
    /// still take part in a directed mating). The server holds a female
    /// promised in an open mating agreement, so she isn't already pregnant
    /// by the time the partner arrives.
    pub fn hold_breeding(&mut self, id: OrgId, until_tick: u64) {
        if let Ok(i) = self.live_index(id) {
            let o = &mut self.organisms[i];
            o.breed_ready_tick = o.breed_ready_tick.max(until_tick);
        }
    }

    /// Take direct control of one of your organisms.
    pub fn possess(&mut self, player: PlayerId, id: OrgId) -> Result<(), CommandError> {
        let i = self.live_index(id)?;
        let o = &mut self.organisms[i];
        if o.owner != player {
            return Err(CommandError::NotOwned(id));
        }
        if o.possessed_by.is_some_and(|p| p != player) {
            return Err(CommandError::PossessedByOther(id));
        }
        o.possessed_by = Some(player);
        Ok(())
    }

    /// Move a possessed organism. It still eats and drinks where it stands.
    pub fn order_move(&mut self, player: PlayerId, id: OrgId, target: Vec2) -> Result<(), CommandError> {
        let i = self.live_index(id)?;
        let target = self.map.clamp(target);
        let o = &mut self.organisms[i];
        if o.possessed_by != Some(player) {
            return Err(CommandError::NotOwned(id));
        }
        o.order = Some(target);
        Ok(())
    }

    /// Drop the move orders of possessed organisms that have arrived (or
    /// died, or are no longer possessed). Runs once per tick, before decide.
    pub(crate) fn settle_orders(&mut self) {
        for o in &mut self.organisms {
            if let Some(t) = o.order {
                if !o.alive || o.possessed_by.is_none() || o.pos.dist(t) <= ORDER_ARRIVAL {
                    o.order = None;
                }
            }
        }
    }

    /// Return a possessed organism to its genetic behavior.
    pub fn release(&mut self, player: PlayerId, id: OrgId) -> Result<(), CommandError> {
        let i = self.live_index(id)?;
        let o = &mut self.organisms[i];
        if o.possessed_by != Some(player) {
            return Err(CommandError::NotOwned(id));
        }
        o.possessed_by = None;
        o.order = None;
        Ok(())
    }

    /// Group command: the player's adults drift toward `target` when no
    /// urgent need overrides it.
    pub fn command_migrate(&mut self, player: PlayerId, target: Vec2) {
        let target = self.map.clamp(target);
        self.migrate_orders.insert(player, target);
    }

    pub fn clear_migration(&mut self, player: PlayerId) {
        self.migrate_orders.remove(&player);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::design::{BirthType, FounderDesign, Thermo};
    use crate::genome::Gene;
    use crate::WorldConfig;

    fn world_with_pair(pref_mismatch: bool) -> (World, OrgId, OrgId) {
        let mut w = World::new(WorldConfig { width: 48, height: 48, ..Default::default() });
        let mut d = FounderDesign::new("t", Thermo::Warm, BirthType::Live, 2).founders(2);
        if pref_mismatch {
            d = d.with(Gene::PrefBodySize, 1.0).with(Gene::TolBodySize, 0.1).with(Gene::Pickiness, 1.0);
        }
        let ids = w.found_lineage(1, &d).unwrap();
        let (fa, fb) = (w.index[&ids[0]], w.index[&ids[1]]);
        let p = w.organisms[fa].pos;
        w.organisms[fb].pos = p;
        (w, ids[0], ids[1])
    }

    #[test]
    fn matching_pair_costs_nothing() {
        let (mut w, a, b) = world_with_pair(false);
        let out = w.directed_mating(DirectedMating { player: 1, a, b, partner_consent: false }).unwrap();
        assert_eq!(out.cost, 0.0);
        assert!(out.conceived);
        assert_eq!(w.mating_slots_left(1), w.config.mating_slots_per_season - 1);
    }

    #[test]
    fn mismatched_pair_costs_more() {
        let (mut w, a, b) = world_with_pair(true);
        let out = w.directed_mating(DirectedMating { player: 1, a, b, partner_consent: false }).unwrap();
        assert!(out.mismatch > 1.0);
        assert!(out.cost > 0.0);
        assert!(out.failure_chance > 0.0);
    }

    #[test]
    fn preview_matches_the_mating_and_changes_nothing() {
        let (mut w, a, b) = world_with_pair(true);
        let p = w.mating_preview(a, b).unwrap();
        assert_eq!(w.mating_slots_left(1), w.config.mating_slots_per_season);
        assert!(p.body_plan_fertility > 0.0 && p.fertility > 0.0, "{p:?}");
        assert!(p.distance < 1e-6);
        let out = w.directed_mating(DirectedMating { player: 1, a, b, partner_consent: false }).unwrap();
        assert_eq!((p.mismatch, p.cost, p.failure_chance, p.fertility), (out.mismatch, out.cost, out.failure_chance, out.fertility));
    }

    #[test]
    fn preview_names_the_body_plan_genes_that_differ() {
        let mut w = World::new(WorldConfig { width: 48, height: 48, ..Default::default() });
        let warm = w.found_lineage(1, &FounderDesign::new("w", Thermo::Warm, BirthType::Live, 2).founders(2)).unwrap();
        let cold = w.found_lineage(2, &FounderDesign::new("c", Thermo::Cold, BirthType::Live, 2).founders(2)).unwrap();
        let sex = |w: &World, id| w.organism(id).unwrap().sex;
        let b = *cold.iter().find(|&&c| sex(&w, c) != sex(&w, warm[0])).unwrap();
        let p = w.mating_preview(warm[0], b).unwrap();
        assert_eq!(p.fertility, 0.0);
        assert_eq!(p.body_plan_fertility, 0.0);
        assert!(p.distance_fertility > 0.9);
        assert_eq!(p.body_plan_gaps.iter().map(|g| g.0).collect::<Vec<_>>(), vec![Gene::Endothermy]);
        assert_eq!(w.mating_preview(warm[0], warm[0]), Err(CommandError::SameSex));
    }

    #[test]
    fn held_organisms_skip_natural_breeding_but_not_directed_matings() {
        let (mut w, a, b) = world_with_pair(false);
        let until = w.tick + 100;
        w.hold_breeding(a, until);
        assert!(!w.organism(a).unwrap().can_breed(w.tick));
        assert!(w.organism(a).unwrap().can_breed(until));
        let out = w.directed_mating(DirectedMating { player: 1, a, b, partner_consent: false }).unwrap();
        assert!(out.conceived);
    }

    #[test]
    fn rules_are_enforced() {
        let (mut w, a, b) = world_with_pair(false);
        let req = DirectedMating { player: 2, a, b, partner_consent: true };
        assert_eq!(w.directed_mating(req), Err(CommandError::NotOwned(a)));
        let ib = w.index[&b];
        w.organisms[ib].pos += Vec2::new(5.0, 0.0);
        let req = DirectedMating { player: 1, a, b, partner_consent: false };
        assert!(matches!(w.directed_mating(req), Err(CommandError::TooFar { .. })));
    }

    #[test]
    fn possession_overrides_movement() {
        let (mut w, a, _) = world_with_pair(false);
        w.possess(1, a).unwrap();
        assert_eq!(w.possess(2, a), Err(CommandError::NotOwned(a)));
        let start = w.organism(a).unwrap().pos;
        let target = w.map.clamp(start + Vec2::new(3.0, 0.0));
        w.order_move(1, a, target).unwrap();
        w.step();
        w.step();
        let o = w.organism(a).unwrap();
        assert_eq!(o.action, crate::organism::Action::Ordered);
        w.release(1, a).unwrap();
        assert!(w.organism(a).unwrap().possessed_by.is_none());
    }

    /// A possessed organism with a move order toward a far corner.
    fn ordered(thirst: f32) -> (World, OrgId, Vec2) {
        let (mut w, a, _) = world_with_pair(false);
        let i = w.index[&a];
        w.organisms[i].needs.thirst = thirst;
        w.organisms[i].needs.hunger = 0.0;
        w.possess(1, a).unwrap();
        let p = w.organisms[i].pos;
        let target = w.map.clamp(Vec2::new(if p.x < 24.0 { 47.0 } else { 0.0 }, p.y));
        w.order_move(1, a, target).unwrap();
        (w, a, target)
    }

    #[test]
    fn arrival_clears_the_order() {
        let (mut w, a, _) = world_with_pair(false);
        w.possess(1, a).unwrap();
        let start = w.organism(a).unwrap().pos;
        let target = w.map.clamp(start + Vec2::new(3.0, 0.0));
        w.order_move(1, a, target).unwrap();
        for _ in 0..w.config.ticks_per_day {
            if w.organism(a).unwrap().order.is_none() {
                break;
            }
            w.step();
        }
        let o = w.organism(a).unwrap();
        assert!(o.order.is_none(), "still ordered at {:?} (target {target:?})", o.pos);
        assert!(o.pos.dist(target) <= ORDER_ARRIVAL + 0.5);
        // Still possessed, but back to its own behavior.
        assert_eq!(o.possessed_by, Some(1));
        w.step();
        assert_ne!(w.organism(a).unwrap().action, crate::organism::Action::Ordered);
    }

    #[test]
    fn parched_possessed_organism_goes_to_drink() {
        let (mut w, a, _) = ordered(95.0);
        w.step();
        assert_eq!(w.organism(a).unwrap().action, crate::organism::Action::Drink);
        // It drinks its fill (and keeps its order for afterwards).
        let mut least = 100.0f32;
        for _ in 0..2 * w.config.ticks_per_day {
            w.step();
            least = least.min(w.organism(a).unwrap().needs.thirst);
        }
        assert!(least <= ORDER_RESUME, "never drank: thirst stayed at {least}");
        assert!(w.organism(a).is_some_and(|o| o.alive));
    }

    #[test]
    fn moderately_thirsty_possessed_organism_follows_the_order() {
        let (mut w, a, target) = ordered(60.0);
        let before = w.organism(a).unwrap().pos.dist(target);
        w.step();
        w.step();
        let o = w.organism(a).unwrap();
        assert_eq!(o.action, crate::organism::Action::Ordered);
        assert_eq!(o.order, Some(target));
        assert!(o.pos.dist(target) < before);
    }
}
