//! Player levers ("Player controls" in the design doc): directed matings,
//! possession and group commands. The future server validates sessions and
//! consent between players, then calls these.

use std::fmt;

use rand::Rng;

use super::groups::{MigrationPlan, MAX_GROUPS};
use super::World;
use crate::genome::{Gene, PlayerId};
use crate::math::Vec2;
use crate::mating;
use crate::organism::{Action, Dormancy, OrgId, Sex};
use crate::pathfinding::{enter_cost, Outcome};
use crate::rng::{rng_for, stream};
use crate::stats::Body;
use crate::terrain::{can_stand, traverse, Map, Tile, Weather};

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
/// How far (tiles) a migration target on deep water or a cliff is moved to
/// the nearest tile the group can stand on; farther than this, refused.
pub const MIGRATE_SNAP_RADIUS: i32 = 8;
/// Tiles a migrating group's adults must be within (at least, see
/// [`World::arrival_radius`]) of the target for the order to be done.
pub const MIGRATE_ARRIVAL: f32 = 3.0;
/// Need (0–1, hunger or thirst) at which an ordered migrant breaks off to
/// eat or drink, at the design default obedience; it goes back to the
/// order once the need is down to [`ORDER_RESUME`]. Obedience moves it by
/// `MIGRATE_BREAK_OBEDIENCE × (obedience − 0.5)`.
pub const MIGRATE_BREAK: f32 = 0.6;
pub const MIGRATE_BREAK_OBEDIENCE: f32 = 0.2;
/// Most points in a [`World::migration_route`].
pub const ROUTE_POINTS: usize = 32;
/// Most group events kept for the server ([`World::take_group_events`]).
pub(super) const GROUP_EVENTS_KEPT: usize = 256;

/// Something that happened to a group order on its own, for the owner
/// ([`World::take_group_events`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum GroupEvent {
    /// Most of the group's adults reached its migration target: the order
    /// is done.
    Arrived { player: PlayerId, group: u8, target: Vec2 },
    /// The group has no living members left: its migration, territory and
    /// rally are gone.
    Emptied { player: PlayerId, group: u8 },
    /// The migration stalled (`groups::MIGRATE_STALL_DAYS`) or passed its
    /// hard limit (`World::migration_timeout_ticks`) without the group
    /// gathering at the target: the order is ended.
    /// `median` is the median member's distance to the target then.
    /// `dormant_share` is the share of the group that was dormant then
    /// (hibernating or estivating animals can't walk).
    /// `basking_share` the share basking or resting (cold-blooded animals
    /// in the cold), `never_moved` whether the group never got closer than
    /// it started (a way blocked by water or cliffs, or animals busy with
    /// needs), and `pending_claim` whether a territory claim waited on the
    /// arrival (it is dropped too).
    TimedOut {
        player: PlayerId,
        group: u8,
        target: Vec2,
        median: f32,
        dormant_share: f32,
        basking_share: f32,
        /// The share too hungry or thirsty to travel (they stop to eat and
        /// drink first).
        needy_share: f32,
        never_moved: bool,
        pending_claim: bool,
    },
    /// A far territory claim's migration arrived and the territory was
    /// claimed (`World::claim_or_migrate`).
    ClaimedOnArrival { player: PlayerId, group: u8, territory: super::groups::Territory },
}

/// How an attack order ended on its own (`World::take_attack_events`). A
/// release, another possession or a move order ends it silently: the player
/// did that.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AttackEnd {
    /// The target was killed by the attacker or one of its pack (its lineage).
    Killed,
    /// The target died of something else.
    TargetDied,
    /// The target went to ground in its burrow, out of reach.
    WentToGround,
    /// The target got away: farther than 1.5 × `World::attack_range`.
    Lost,
    /// The attacker died.
    AttackerDied,
}

/// An attack order that ended on its own, for the attacker's owner.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AttackEvent {
    pub player: PlayerId,
    pub attacker: OrgId,
    pub target: OrgId,
    pub end: AttackEnd,
}

#[derive(Clone, Debug, PartialEq)]
pub enum CommandError {
    NotFound(OrgId),
    NotOwned(OrgId),
    /// The player's own organism, but a move or release needs it possessed.
    NotPossessed(OrgId),
    /// A target outside the map (refused rather than clamped to the edge).
    OffMap,
    /// A target in deep water or up a cliff with no land the animals could
    /// stand on near it (`MIGRATE_SNAP_RADIUS`), or that they can't reach.
    Unreachable,
    /// The other organism belongs to another player who hasn't consented.
    NoConsent,
    NotMature(OrgId),
    SameSex,
    AlreadyPregnant,
    TooFar {
        distance: f32,
    },
    NoSlotsLeft,
    PossessedByOther(OrgId),
    /// No such group (out of range, or without members).
    NoSuchGroup(u8),
    /// The player already has `groups::MAX_GROUPS` groups.
    GroupLimit,
    /// A split must leave members on both sides.
    GroupTooSmall(u8),
    NotInGroup(OrgId),
    SameGroup(u8),
    /// The group holds no territory.
    NoTerritory(u8),
    /// The group claimed a territory too recently (`groups::CLAIM_COOLDOWN_DAYS`).
    ClaimCooldown {
        ticks_left: u64,
    },
    /// The group is answering no rally.
    NoRally(u8),
    /// A territory or rally circle mostly over water the group can't stand
    /// in (`groups::MAX_WATER_SHARE`); `share` is that water's share.
    MostlyWater {
        share: f32,
    },
    /// A territory circle overlapping another player's territory (whose,
    /// isn't said: claims would map others' territories).
    Overlaps,
    /// A territory radius that is not a positive number.
    BadRadius,
    /// An attack order on one of the player's own organisms.
    OwnTarget(OrgId),
    /// A plant eater can't be ordered to attack ([`HERBIVORE_ATTACK`]).
    NotAHunter(OrgId),
    /// The target is farther than the possessed organism can see.
    OutOfSight(OrgId),
    /// The target is heavier than the attacker (with its pack-mates around)
    /// can take on.
    TooBig {
        mass: f32,
        limit: f32,
    },
    /// The target is down in its burrow.
    Sheltered(OrgId),
    /// The target's lineage was founded too recently (`Balance::attack_grace`).
    NewcomerGrace {
        days_left: f32,
    },
    /// The attacker's own lineage is under its newcomer grace: while it is
    /// shielded from other players, it can't attack them either.
    OwnGrace {
        days_left: f32,
    },
    /// `end_grace` with no grace to end.
    NoGrace,
}

impl fmt::Display for CommandError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CommandError::NotFound(id) => write!(f, "organism {id} not found"),
            CommandError::NotOwned(id) => write!(f, "organism {id} is not yours"),
            CommandError::NotPossessed(id) => write!(f, "organism {id} must be possessed first"),
            CommandError::OffMap => write!(f, "that target is outside the map"),
            CommandError::Unreachable => {
                write!(f, "that target is deep water or impassable terrain, with no land they could reach near it")
            }
            CommandError::NoConsent => write!(f, "the other player has not consented"),
            CommandError::NotMature(id) => write!(f, "organism {id} is not old enough to breed"),
            CommandError::SameSex => write!(f, "organisms are the same sex"),
            CommandError::AlreadyPregnant => write!(f, "the female is already pregnant"),
            CommandError::TooFar { distance } => {
                write!(f, "organisms are {distance:.1} tiles apart; they must be within {MEETING_DISTANCE}")
            }
            CommandError::NoSlotsLeft => write!(f, "no directed-mating slots left this season"),
            CommandError::PossessedByOther(id) => write!(f, "organism {id} is possessed by another player"),
            CommandError::NoSuchGroup(g) => write!(f, "you have no group {g}"),
            CommandError::GroupLimit => {
                write!(f, "you already have {} groups; merge two first", super::groups::MAX_GROUPS)
            }
            CommandError::GroupTooSmall(g) => write!(f, "a split must leave organisms in both halves of group {g}"),
            CommandError::NotInGroup(id) => write!(f, "organism {id} is not in that group"),
            CommandError::SameGroup(g) => write!(f, "group {g} can't merge into itself"),
            CommandError::NoTerritory(g) => write!(f, "group {g} holds no territory"),
            CommandError::ClaimCooldown { ticks_left } => {
                write!(f, "the group claimed a territory recently; it can claim again in {ticks_left} ticks")
            }
            CommandError::NoRally(g) => write!(f, "group {g} has no rally to cancel"),
            CommandError::Overlaps => {
                write!(f, "that circle overlaps another player's territory; claim ground of your own")
            }
            CommandError::BadRadius => write!(f, "a territory's radius must be a positive number of tiles"),
            CommandError::MostlyWater { share } => write!(
                f,
                "{:.0}% of that circle is water the group can't stand in; choose ground with some land",
                share * 100.0
            ),
            CommandError::OwnTarget(id) => write!(f, "organism {id} is yours; you can't attack your own animals"),
            CommandError::NotAHunter(id) => {
                write!(f, "organism {id} is a plant eater: it doesn't hunt (it fights back only when attacked)")
            }
            CommandError::OutOfSight(id) => write!(f, "organism {id} is out of sight; get closer first"),
            CommandError::TooBig { mass, limit } => {
                let kg = |x: f32| if x < 10.0 { format!("{x:.1}") } else { format!("{x:.0}") };
                write!(
                    f,
                    "that animal ({} kg) is too big to take on; this one can tackle up to {} kg, counting the pack-mates around it now",
                    kg(*mass),
                    kg(*limit)
                )
            }
            CommandError::Sheltered(id) => {
                write!(f, "organism {id} is down in its burrow, out of reach: wait for it to come out")
            }
            CommandError::NewcomerGrace { days_left } => write!(
                f,
                "that lineage was founded only recently; it can't be attacked for another {:.0} days",
                days_left.ceil()
            ),
            CommandError::NoGrace => write!(f, "your lineage has no newcomer grace to end"),
            CommandError::OwnGrace { days_left } => write!(
                f,
                "your lineage is still under its newcomer grace; it can't attack other players for another {:.0} days",
                days_left.ceil()
            ),
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

/// How far past `World::attack_range` an attack order is given and kept,
/// as a multiple of it.
pub const ORDER_KEPT: f32 = 1.5;
/// Carnivory below which an animal can't be ordered to attack
/// (`World::possess_attack`): a plant eater.
pub const HERBIVORE_ATTACK: f32 = 0.3;

/// Foundings of at most this many founders keep their grace through their
/// first winter when it would end late in autumn or during it
/// (`World::grace_end`).
pub const GRACE_WINTER_FOUNDERS: u32 = 40;

impl World {
    pub(super) fn live_index(&self, id: OrgId) -> Result<usize, CommandError> {
        self.index.get(&id).copied().filter(|&i| self.organisms[i].alive).ok_or(CommandError::NotFound(id))
    }

    /// Use up one of `player`'s directed-mating slots this season without a
    /// mating (a forfeit: a bought agreement that lapsed unpaid). False if
    /// none was left.
    pub fn forfeit_mating_slot(&mut self, player: PlayerId) -> bool {
        if self.mating_slots_left(player) == 0 {
            return false;
        }
        *self.slots_used.entry(player).or_default() += 1;
        true
    }

    pub fn mating_slots_left(&self, player: PlayerId) -> u32 {
        (self.config.mating_slots_per_season + self.bonus_slots(player))
            .saturating_sub(self.slots_used.get(&player).copied().unwrap_or(0))
    }

    /// Extra directed-mating slots `player` holds this season
    /// ([`World::grant_mating_slots`]).
    pub fn bonus_slots(&self, player: PlayerId) -> u32 {
        self.bonus_slots.get(&player).copied().unwrap_or(0)
    }

    /// Give `player` `n` extra directed-mating slots until the season
    /// changes (the server sells them for renown). Returns the extra slots
    /// held now.
    pub fn grant_mating_slots(&mut self, player: PlayerId, n: u32) -> u32 {
        let b = self.bonus_slots.entry(player).or_default();
        *b = b.saturating_add(n);
        *b
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
        let mismatch =
            mating::mismatch(&female.pheno, male.display()).max(mating::mismatch(&male.pheno, female.display()));
        let cost = mating::override_cost(mismatch, self.config.override_k);
        let failure_chance = mating::override_failure_chance(cost);
        let fertility = mating::fertility_with(&female.genome, &male.genome, mating::Relief::of(&self.config.balance))
            * self.disease_fertility(f, m);
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
            .zip(mating::plan_weights(mating::Relief::of(&self.config.balance)))
            .map(|(&g, w)| (g, (ga.expr(g) - gb.expr(g)).abs() * w))
            .filter(|&(_, gap)| gap > mating::BODY_PLAN_FULL)
            .collect();
        Ok(MatingPreview {
            mismatch,
            cost,
            failure_chance,
            fertility,
            body_plan_fertility: mating::fertility_from_gap(mating::fertility_gap(
                ga,
                gb,
                mating::Relief::of(&self.config.balance),
            )),
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
        // One at a time: possessing another releases the player's others,
        // or the possession bonuses would apply to a whole lineage.
        for (k, x) in self.organisms.iter_mut().enumerate() {
            if k != i && x.possessed_by == Some(player) {
                x.possessed_by = None;
                x.order = None;
                self.attack_orders.remove(&x.id);
            }
        }
        self.organisms[i].possessed_by = Some(player);
        Ok(())
    }

    /// Order a possessed organism to attack another player's (or a wild)
    /// organism: it pursues the target and strikes when in reach, by the
    /// usual strike rules (escape, reach, power against resistance, the
    /// burrow and offline protection), and its pack-mates nearby join the
    /// hunt (`Balance::pack_hunt`). Refused on the player's own organisms,
    /// on a target out of sight, too heavy for the attacker with the
    /// pack-mates it has around (`decide::max_prey_mass`, `pack_scale_at`
    /// at the strength of a player-led hunt, `decide::led_pack`),
    /// down in its burrow, or of a player lineage founded less than
    /// `Balance::attack_grace` days ago. Replaces its move order. The order
    /// ends when either dies, the target goes to ground or out of sight, or
    /// the organism is released (`World::settle_orders`), and gives way to
    /// urgent thirst or hunger like a move order (`Balance::possess_urgency`).
    pub fn possess_attack(&mut self, player: PlayerId, id: OrgId, target: OrgId) -> Result<(), CommandError> {
        let i = self.live_index(id)?;
        let o = &self.organisms[i];
        if o.possessed_by != Some(player) {
            return Err(if o.owner == player { CommandError::NotPossessed(id) } else { CommandError::NotOwned(id) });
        }
        let j = self.live_index(target)?;
        let q = &self.organisms[j];
        if q.owner == player || j == i {
            return Err(CommandError::OwnTarget(target));
        }
        // Plant eaters don't hunt (playtest 6: a possessed grazer one-shot a
        // fox on orders); they fight back when attacked, by the strike rules.
        if o.traits.carnivory < HERBIVORE_ATTACK {
            return Err(CommandError::NotAHunter(id));
        }
        // As far as a standing order is kept (`World::settle_orders`): the
        // player's view lags the world by a second or two, and in a chase
        // a target next to the attacker on screen can be past the sight
        // range by the time the order lands (playtest 4).
        if o.pos.dist(q.pos) > ORDER_KEPT * self.attack_range() {
            return Err(CommandError::OutOfSight(target));
        }
        if q.sheltered {
            return Err(CommandError::Sheltered(target));
        }
        let limit = super::decide::max_prey_mass(o, self.coop_bonus(o))
            * super::decide::led_pack_scale(&self.config.balance, o);
        if q.mass() >= limit {
            return Err(CommandError::TooBig { mass: q.mass(), limit });
        }
        if let Some(days_left) = self.attack_grace_left(q) {
            return Err(CommandError::NewcomerGrace { days_left });
        }
        // Two-sided: a shielded newcomer doesn't attack other players.
        if q.owner != crate::WILD {
            if let Some(days_left) = self.attack_grace_left(o) {
                return Err(CommandError::OwnGrace { days_left });
            }
        }
        self.organisms[i].order = None;
        self.attack_orders.insert(id, target);
        Ok(())
    }

    /// Why an attack order on `target` by `id` may be a bad idea, if it is:
    /// the attacker's lineage has learned that the target's kind is
    /// poisonous (`World::toxic_lesson`). The order is allowed all the same.
    pub fn attack_warning(&self, id: OrgId, target: OrgId) -> Option<String> {
        let (o, q) = (self.organism(id)?, self.organism(target)?);
        let lesson = self.toxic_lesson(o.lineage, q.lineage);
        (lesson >= super::toxic::LOOKS_POISONOUS).then(|| {
            format!(
                "#{target} looks poisonous to your kind: your animals have been poisoned eating its kind (on their own they pass it up {:.0}% of the time)",
                100.0 * self.toxic_avoidance(o.lineage, q)
            )
        })
    }

    /// Days left of the grace a player lineage founded into a running
    /// world has against other players (`Balance::attack_grace`), if any:
    /// no attack orders on it and no hunting by other players' animals
    /// (wild predators still hunt it). Lineages founded at tick 0 (the
    /// world's first players) and wild ones have none.
    pub(crate) fn attack_grace_left(&self, q: &crate::organism::Organism) -> Option<f32> {
        if q.owner == crate::WILD {
            return None;
        }
        self.lineage_grace_left(q.lineage)
    }

    /// Days of newcomer grace lineage `l` has left (`World::attack_grace_left`).
    /// The grace lasts `Balance::attack_grace` days, or
    /// `World::set_attack_grace_min_ticks` ticks if that is longer (a minimum
    /// of real time at a fast clock). It runs from the founding of the
    /// owner's earliest lineage in an unbroken run of foundings, each within
    /// twice the grace of the one before (`World::grace_start`): re-founding
    /// doesn't start it over.
    pub fn lineage_grace_left(&self, l: u32) -> Option<f32> {
        let tpd = self.config.ticks_per_day.max(1) as u64;
        self.lineage_grace_ticks(l).map(|left| left as f32 / tpd as f32)
    }

    /// Ticks of newcomer grace lineage `l` has left (`World::lineage_grace_left`).
    /// None once its owner ended it (`World::end_grace`).
    pub fn lineage_grace_ticks(&self, l: u32) -> Option<u64> {
        if self.config.balance.attack_grace <= 0.0 {
            return None;
        }
        let owner = self.lineages.get(l as usize)?.owner;
        if owner == crate::WILD {
            return None;
        }
        let start = match self.grace_starts.get(l as usize) {
            Some(&t) if self.grace_starts_cooldown == self.grace_cooldown() => t,
            _ => self.grace_start(l)?,
        };
        if start == 0 || self.grace_waived.get(&owner).is_some_and(|&t| t >= start) {
            return None;
        }
        let left = self.grace_end(l, start).saturating_sub(self.tick);
        (left > 0).then_some(left)
    }

    /// The tick lineage `l`'s grace (started at `start`) ends:
    /// [`World::grace_length`] after it, or, for a small founding (at most
    /// [`GRACE_WINTER_FOUNDERS`] founders) whose grace would end late in
    /// autumn or in its first winter, at that winter's end: a small early
    /// founding isn't left to predators exactly as its first winter starts
    /// (playtest 5).
    pub fn grace_end(&self, l: u32, start: u64) -> u64 {
        let end = start + self.grace_length();
        let small = self.lineages.get(l as usize).is_some_and(|x| x.founders <= GRACE_WINTER_FOUNDERS);
        if !small {
            return end;
        }
        let tpd = self.config.ticks_per_day.max(1) as u64;
        let dpy = self.config.days_per_year.max(4) as u64;
        let day = end / tpd;
        let year = day - day % dpy;
        // Winter is the year's last quarter (`Season::from_day`); late autumn the third before it.
        let winter = year + (3 * dpy).div_ceil(4);
        let late_autumn = winter.saturating_sub(dpy / 12);
        if (late_autumn..year + dpy).contains(&day) && start / tpd < winter {
            (year + dpy) * tpd
        } else {
            end
        }
    }

    /// When lineage `l`'s grace started (its founding, or an earlier
    /// lineage's whose grace it continues: [`World::lineage_grace_left`]).
    pub fn lineage_grace_start(&self, l: u32) -> Option<u64> {
        self.grace_start(l)
    }

    /// How long a newcomer grace lasts in ticks: `Balance::attack_grace`
    /// days, or `World::set_attack_grace_min_ticks` if that is longer.
    pub fn grace_length(&self) -> u64 {
        let tpd = self.config.ticks_per_day.max(1) as f32;
        ((self.config.balance.attack_grace.max(0.0) * tpd).round() as u64).max(self.grace_min_ticks)
    }

    /// End the player's newcomer grace now (a predator's owner wants to
    /// hunt): their animals may attack other players' and be attacked.
    /// Re-founding within the cooldown doesn't bring it back
    /// (`World::grace_start`). `NoGrace` if they have none.
    pub fn end_grace(&mut self, player: PlayerId) -> Result<(), CommandError> {
        let graced = (0..self.lineages.len() as u32)
            .any(|l| self.lineages[l as usize].owner == player && self.lineage_grace_ticks(l).is_some());
        if !graced {
            return Err(CommandError::NoGrace);
        }
        self.grace_waived.insert(player, self.tick);
        Ok(())
    }

    /// When each player ended their grace (`World::end_grace`), for the
    /// host to save; not in the snapshot.
    pub fn grace_waived(&self) -> &std::collections::BTreeMap<PlayerId, u64> {
        &self.grace_waived
    }

    /// Restore [`World::grace_waived`] (from the host's saved game).
    pub fn set_grace_waived(&mut self, waived: std::collections::BTreeMap<PlayerId, u64>) {
        self.grace_waived = waived;
    }

    /// When lineage `l`'s newcomer grace started (`World::lineage_grace_left`):
    /// its founding, or that of the owner's earliest lineage in an unbroken
    /// run of foundings each within twice the grace of the next.
    pub(crate) fn grace_start(&self, l: u32) -> Option<u64> {
        let owner = self.lineages.get(l as usize)?.owner;
        let cooldown = self.grace_cooldown();
        let mut start = *self.lineage_founded.get(l as usize)?;
        while let Some(t) = (0..self.lineages.len())
            .filter(|&k| self.lineages[k].owner == owner)
            .filter_map(|k| self.lineage_founded.get(k).copied())
            .filter(|&t| t < start && start - t <= cooldown)
            .min()
        {
            start = t;
        }
        Some(start)
    }

    /// How soon after one founding the owner's next still continues its
    /// grace: twice the grace's whole length (its real-time minimum
    /// included), so a re-founding never starts afresh a grace that would
    /// still be running; the time left carries over.
    fn grace_cooldown(&self) -> u64 {
        2 * self.grace_length()
    }

    /// Recomputes every lineage's grace start (after founding or restoring).
    pub(crate) fn rebuild_grace_starts(&mut self) {
        self.grace_starts_cooldown = self.grace_cooldown();
        self.grace_starts = (0..self.lineages.len() as u32).map(|l| self.grace_start(l).unwrap_or(0)).collect();
    }

    /// Ticks the newcomer grace lasts at least, whatever `Balance::attack_grace`
    /// says (the server's minimum of real minutes; not saved).
    pub fn set_attack_grace_min_ticks(&mut self, ticks: u64) {
        self.grace_min_ticks = ticks;
        self.rebuild_grace_starts();
    }

    /// Whether hunter `a` is kept off prey `b` by founding grace
    /// (`World::attack_grace_left`): `a` is another player's, and either
    /// `b`'s lineage is graced or (two-sided) `a`'s is and `b` isn't wild.
    pub(crate) fn grace_spares(&self, a: &crate::organism::Organism, b: &crate::organism::Organism) -> bool {
        a.owner != crate::WILD
            && a.owner != b.owner
            && (self.attack_grace_left(b).is_some() || (b.owner != crate::WILD && self.attack_grace_left(a).is_some()))
    }

    /// The target of a possessed organism's attack order, if it has one.
    pub fn attack_target(&self, id: OrgId) -> Option<OrgId> {
        self.attack_orders.get(&id).copied()
    }

    /// Move a possessed organism. It still eats and drinks where it stands.
    /// A target it can't get to (out at sea, up a cliff, across water it
    /// can't swim) becomes the nearest tile it can reach, so the order ends
    /// there instead of leaving it standing at the shore: the target used.
    pub fn order_move(&mut self, player: PlayerId, id: OrgId, target: Vec2) -> Result<Vec2, CommandError> {
        let i = self.live_index(id)?;
        let o = &self.organisms[i];
        if o.possessed_by != Some(player) {
            return Err(if o.owner == player { CommandError::NotPossessed(id) } else { CommandError::NotOwned(id) });
        }
        let target = self.reachable_order_target(i, self.map.clamp(target))?;
        self.organisms[i].order = Some(target);
        self.attack_orders.remove(&id);
        Ok(target)
    }

    /// Where organism `i` can actually get toward `target`: the target
    /// itself if a route reaches its tile, else the tile the route ends on
    /// (next to a blocked goal) or, with no route at all, the reachable
    /// tile nearest the target. Flyers go anywhere.
    fn reachable_order_target(&mut self, i: usize, target: Vec2) -> Result<Vec2, CommandError> {
        let o = &self.organisms[i];
        let body = o.traits.body;
        if body.can_fly || self.config.balance.path_nodes < 1.0 {
            return Ok(target);
        }
        let from = o.pos;
        let w = self.map.width;
        let tile = |p: Vec2| {
            let (x, y) = self.map.tile_coords(p);
            (y * w + x) as u32
        };
        let center = |k: u32| Map::tile_center(k as usize % w, k as usize / w);
        let mut scratch = std::mem::take(&mut self.path_scratch);
        let mut way = Vec::new();
        let goal = target;
        let outcome = scratch.search_toward(
            &self.map,
            from,
            goal,
            self.map.width * self.map.height,
            |g, t| enter_cost(traverse(&body, g, t, &Weather::still(t.wind), false)),
            &mut way,
        );
        let nearest = scratch.nearest();
        self.path_scratch = scratch;
        match outcome {
            Outcome::Found => match way.first() {
                Some(&end) if end != tile(goal) => Ok(center(end)),
                _ => Ok(goal),
            },
            Outcome::Partial => Ok(goal),
            Outcome::None => match nearest {
                Some(k) => Ok(center(k)),
                // The search never left the start (an organism stranded on a
                // tile it can't use): no better guess than the target; it
                // struggles toward it as before.
                None => Ok(goal),
            },
        }
    }

    /// Drop the move orders of possessed organisms that have arrived (or
    /// died, or are no longer possessed), and rallies that have lapsed. Runs
    /// once per tick, before decide.
    /// An order whose route search found no way (the terrain changed under
    /// it: ice gone, a river up) is dropped too, rather than leaving the
    /// organism standing until it starves.
    pub(crate) fn settle_orders(&mut self) {
        self.settle_rallies();
        self.settle_groups();
        for o in &mut self.organisms {
            if let Some(t) = o.order {
                let stuck = o.path.as_ref().is_some_and(|p| p.failed && p.serves(o.pos, t));
                if !o.alive || o.possessed_by.is_none() || o.pos.dist(t) <= ORDER_ARRIVAL || stuck {
                    o.order = None;
                }
            }
        }
        self.settle_attack_orders();
    }

    /// Drop attack orders whose attacker died or was released, or whose
    /// target died, went to ground or got out of sight (half as far again
    /// as `World::attack_range`, so a chase isn't dropped at the edge).
    fn settle_attack_orders(&mut self) {
        if self.attack_orders.is_empty() {
            return;
        }
        let orders = std::mem::take(&mut self.attack_orders);
        let range = ORDER_KEPT * self.attack_range();
        for (a, t) in orders {
            let (Some(&i), Some(&j)) = (self.index.get(&a), self.index.get(&t)) else { continue };
            let (o, q) = (&self.organisms[i], &self.organisms[j]);
            let end = if !o.alive {
                Some(AttackEnd::AttackerDied)
            } else if o.possessed_by.is_none() {
                // Released: the player ended it.
                continue;
            } else if !q.alive {
                Some(if q.killer == Some(o.lineage) { AttackEnd::Killed } else { AttackEnd::TargetDied })
            } else if q.sheltered {
                Some(AttackEnd::WentToGround)
            } else if o.pos.dist(q.pos) > range {
                Some(AttackEnd::Lost)
            } else {
                None
            };
            match end {
                None => {
                    self.attack_orders.insert(a, t);
                }
                Some(end) => {
                    if self.attack_events.len() < GROUP_EVENTS_KEPT {
                        self.attack_events.push(AttackEvent { player: o.owner, attacker: a, target: t, end });
                    }
                }
            }
        }
    }

    /// The attack orders that ended on their own since the last call, oldest
    /// first (at most `GROUP_EVENTS_KEPT` kept between calls).
    pub fn take_attack_events(&mut self) -> Vec<AttackEvent> {
        std::mem::take(&mut self.attack_events)
    }

    /// Fraction of a tick's movement one possession sub-step covers.
    pub(super) fn substep_frac(&self) -> f32 {
        1.0 / self.config.possess_substeps.max(1) as f32
    }

    /// Possession sub-steps still to take before the next tick
    /// (`possess_substeps - 1` right after a tick, 0 when sub-steps are off).
    pub fn substeps_left(&self) -> u32 {
        self.config.possess_substeps.max(1) - 1 - self.substep
    }

    /// One possession sub-step, between ticks ("possessed organisms tick
    /// faster", design doc): every possessed organism with a move order that
    /// danger or a severe need doesn't override (as of its last tick) moves
    /// `1 / possess_substeps` of a tick's movement toward it, so a new order
    /// takes effect within a fraction of a tick and the owner sees smooth
    /// movement. Nothing else changes and no randomness is drawn: worlds
    /// without possessed organisms are untouched. Returns the organisms that
    /// moved. A no-op once [`World::substeps_left`] is 0: the tick's own
    /// move is the last sub-step, and [`World::step`] first takes any the
    /// caller skipped, so the total movement per tick is the same either way.
    pub fn substep(&mut self) -> Vec<OrgId> {
        let mut moved = Vec::new();
        if self.substeps_left() == 0 {
            return moved;
        }
        self.substep += 1;
        let frac = self.substep_frac();
        for i in 0..self.organisms.len() {
            let o = &self.organisms[i];
            let (Some(_), Some(target)) = (o.possessed_by, o.order) else { continue };
            if !o.alive || o.dormancy != Dormancy::Awake || o.pos.dist(target) <= ORDER_ARRIVAL {
                continue;
            }
            if self.order_override(o, o.needs.safety / 100.0).is_some() {
                continue;
            }
            let sprint = o.needs.energy > 40.0;
            if self.move_toward(i, target, sprint, frac) > 0.0 {
                let o = &mut self.organisms[i];
                o.action = Action::Ordered;
                o.target = Some(target);
                moved.push(o.id);
            }
        }
        moved
    }

    /// Take the sub-steps left before a tick.
    pub(super) fn finish_substeps(&mut self) {
        while self.substeps_left() > 0 {
            self.substep();
        }
    }

    /// Return a possessed organism to its genetic behavior.
    pub fn release(&mut self, player: PlayerId, id: OrgId) -> Result<(), CommandError> {
        let i = self.live_index(id)?;
        let o = &mut self.organisms[i];
        if o.possessed_by != Some(player) {
            return Err(if o.owner == player { CommandError::NotPossessed(id) } else { CommandError::NotOwned(id) });
        }
        o.possessed_by = None;
        o.order = None;
        self.attack_orders.remove(&id);
        Ok(())
    }

    /// Group command for the main group (group 0), like a rally or a
    /// territory given without a group: [`World::command_migrate_group`].
    pub fn command_migrate(&mut self, player: PlayerId, target: Vec2) -> Result<Vec2, CommandError> {
        self.command_migrate_group(player, 0, target)
    }

    /// Group command for one group: its adults drift toward `target`. Replaces
    /// the group's territory or rally, if it had one. A target off the map
    /// is refused; one on deep water (frozen or not) or a cliff moves to the
    /// nearest tile most of the group can stand on, within
    /// [`MIGRATE_SNAP_RADIUS`], else is refused. The target used.
    ///
    /// The order is done once most of the group's adults have arrived
    /// ([`World::settle_groups`]). On the way, members break off to drink
    /// or eat when a need passes [`MIGRATE_BREAK`] and go back to it once
    /// it is down to [`ORDER_RESUME`], and they keep off ice over water
    /// that would drown them if it melted (`World::ice_trap`).
    pub fn command_migrate_group(&mut self, player: PlayerId, group: u8, target: Vec2) -> Result<Vec2, CommandError> {
        if group >= MAX_GROUPS {
            return Err(CommandError::NoSuchGroup(group));
        }
        // The main group always exists; another only while it has members.
        let middle = self.group_middle(player, group);
        if group != 0 && middle.is_none() {
            return Err(CommandError::NoSuchGroup(group));
        }
        let target = self.on_map(target)?;
        let bodies = self.group_bodies(player, group);
        let target = self.standable_near(&bodies, target, MIGRATE_SNAP_RADIUS).ok_or(CommandError::Unreachable)?;
        let key = (player, group);
        self.migrate_orders.insert(key, target);
        self.group_plans.pending.claims.remove(&key);
        // Where the group's median member starts from: the headway is
        // measured against it (a far claim's start used to be taken at the
        // first hourly check, from wherever the group had got to).
        match self.group_median_dist(player, group, target) {
            Some(d) => self.group_plans.pending.start_median.insert(key, d),
            None => self.group_plans.pending.start_median.remove(&key),
        };
        let deadline = self.tick + self.migration_timeout_ticks(middle.map_or(0.0, |m| m.dist(target)));
        self.group_plans.migrations.insert(key, MigrationPlan::new(self.tick, deadline));
        self.territories.remove(&key);
        self.rallies.remove(&key);
        self.group_plans.settles.remove(&key);
        Ok(target)
    }

    /// Move a standing migration's target (a re-route, e.g. when the target
    /// floods), keeping when it was issued and pushing its deadline out to
    /// fit the new way from where the group is now (never earlier than it
    /// was). The target is checked as a new order's is; `NoSuchGroup` if the
    /// group has no migration. The target used.
    pub fn reroute_migration(&mut self, player: PlayerId, group: u8, target: Vec2) -> Result<Vec2, CommandError> {
        let key = (player, group);
        if !self.migrate_orders.contains_key(&key) {
            return Err(CommandError::NoSuchGroup(group));
        }
        let old = self.group_plans.migrations.get(&key).copied();
        let target = self.command_migrate_group(player, group, target)?;
        if let (Some(old), Some(new)) = (old, self.group_plans.migrations.get_mut(&key)) {
            // The new way starts its headway afresh, from now.
            new.issued_tick = old.issued_tick;
            new.deadline_tick = new.deadline_tick.max(old.deadline_tick);
        }
        Ok(target)
    }

    /// The way a migrating group is expected to go, for drawing: from its
    /// member nearest the group's middle (an adult if it has any) to the
    /// target, as that member would route it (around water it can't
    /// cross and ice it would drown under, `World::ice_trap`), thinned to
    /// at most [`ROUTE_POINTS`] points. Ends short of the target where no
    /// route reaches it. Empty without an order; just the target without
    /// members. A one-off full-map search: a host computes it when the
    /// order is given and keeps it.
    pub fn migration_route(&mut self, player: PlayerId, group: u8) -> Vec<Vec2> {
        let Some(target) = self.migration_order(player, group) else { return Vec::new() };
        let mine = |adult: bool| {
            self.organisms
                .iter()
                .enumerate()
                .filter(move |(_, o)| o.alive && o.owner == player && o.group == group && (o.matured || !adult))
                .map(|(k, _)| k)
                .collect::<Vec<usize>>()
        };
        let mut members = mine(true);
        if members.is_empty() {
            members = mine(false);
        }
        if members.is_empty() {
            return vec![target];
        }
        let mid = members.iter().fold(Vec2::ZERO, |s, &k| s + self.organisms[k].pos) * (1.0 / members.len() as f32);
        let lead = *members
            .iter()
            .min_by(|&&a, &&b| self.organisms[a].pos.dist(mid).total_cmp(&self.organisms[b].pos.dist(mid)))
            .expect("members");
        let (from, body) = (self.organisms[lead].pos, self.organisms[lead].traits.body);
        if body.can_fly {
            return vec![from, target];
        }
        let w = self.map.width;
        let center = |k: u32| Map::tile_center(k as usize % w, k as usize / w);
        let mut scratch = std::mem::take(&mut self.path_scratch);
        let mut way = Vec::new();
        let outcome = scratch.search_toward(
            &self.map,
            from,
            target,
            self.map.width * self.map.height,
            |g, t| {
                if super::apply::ice_trap(&body, t) {
                    return None;
                }
                enter_cost(traverse(&body, g, t, &Weather::still(t.wind), false))
            },
            &mut way,
        );
        let nearest = scratch.nearest();
        self.path_scratch = scratch;
        let (tx, ty) = self.map.tile_coords(target);
        let goal = (ty * w + tx) as u32;
        let mut tiles: Vec<u32> = way.iter().rev().copied().collect();
        let end = match outcome {
            Outcome::Found if tiles.last() == Some(&goal) || tiles.is_empty() => {
                tiles.pop();
                Some(target)
            }
            Outcome::Found | Outcome::Partial => tiles.pop().map(center),
            Outcome::None => nearest.map(center),
        };
        let stride = tiles.len().div_ceil(ROUTE_POINTS.saturating_sub(2).max(1)).max(1);
        let mut route = vec![from];
        route.extend(tiles.iter().skip(stride - 1).step_by(stride).map(|&k| center(k)));
        if let Some(e) = end {
            if route.last() != Some(&e) {
                route.push(e);
            }
        }
        route
    }

    /// `p` if it lies on the map (edges included), else [`CommandError::OffMap`].
    pub(super) fn on_map(&self, p: Vec2) -> Result<Vec2, CommandError> {
        let (w, h) = (self.map.width as f32, self.map.height as f32);
        if (0.0..=w).contains(&p.x) && (0.0..=h).contains(&p.y) {
            Ok(self.map.clamp(p))
        } else {
            Err(CommandError::OffMap)
        }
    }

    /// Bodies of up to 64 members of a group (adults first), or of the
    /// player's organisms if the group has none: whose footing a target is
    /// checked against.
    pub(super) fn group_bodies(&self, player: PlayerId, group: u8) -> Vec<Body> {
        const SAMPLE: usize = 64;
        let mine = || self.organisms.iter().filter(move |o| o.alive && o.owner == player);
        let mut bodies: Vec<Body> =
            mine().filter(|o| o.group == group && o.matured).take(SAMPLE).map(|o| o.traits.body).collect();
        if bodies.is_empty() {
            bodies = mine().filter(|o| o.group == group).take(SAMPLE).map(|o| o.traits.body).collect();
        }
        if bodies.is_empty() {
            bodies = mine().take(SAMPLE).map(|o| o.traits.body).collect();
        }
        bodies
    }

    /// Whether at least half of `bodies` could stand on tile `k` even with
    /// its ice gone, without swimming (`terrain::can_stand`: a strong
    /// swimmer can cross a lake, but a herd can't live in one); any tile
    /// with no bodies to check.
    pub(super) fn standable_tile(&self, bodies: &[Body], k: usize) -> bool {
        if bodies.is_empty() {
            return true;
        }
        let tile = Tile { ice: 0.0, ..self.map.tiles[k].clone() };
        let g = &self.map.ground()[k];
        2 * bodies.iter().filter(|b| can_stand(b, g, &tile)).count() >= bodies.len()
    }

    /// The point nearest `target` (itself if it qualifies) on a tile at
    /// least half of `bodies` could stand on even with its ice gone, within
    /// `radius` tiles; any target with no bodies to check.
    pub(super) fn standable_near(&self, bodies: &[Body], target: Vec2, radius: i32) -> Option<Vec2> {
        if bodies.is_empty() {
            return Some(target);
        }
        let (w, h) = (self.map.width as i32, self.map.height as i32);
        let ok = |x: i32, y: i32| self.standable_tile(bodies, self.map.idx(x as usize, y as usize));
        let (tx, ty) = self.map.tile_coords(target);
        let (tx, ty) = (tx as i32, ty as i32);
        if ok(tx, ty) {
            return Some(target);
        }
        for r in 1..=radius {
            let mut best: Option<(f32, Vec2)> = None;
            for y in (ty - r).max(0)..=(ty + r).min(h - 1) {
                for x in (tx - r).max(0)..=(tx + r).min(w - 1) {
                    if (x - tx).abs().max((y - ty).abs()) != r || !ok(x, y) {
                        continue;
                    }
                    let c = Map::tile_center(x as usize, y as usize);
                    let d = c.dist(target);
                    if best.is_none_or(|(bd, _)| d < bd) {
                        best = Some((d, c));
                    }
                }
            }
            if let Some((_, c)) = best {
                return Some(c);
            }
        }
        None
    }

    /// Drop the migration orders of all the player's groups.
    pub fn clear_migration(&mut self, player: PlayerId) {
        self.migrate_orders.retain(|&(p, _), _| p != player);
    }

    /// Drop one group's migration order.
    /// Whether one of the player's living organisms is in `group`.
    pub fn group_has_members(&self, player: PlayerId, group: u8) -> bool {
        self.organisms.iter().any(|o| o.alive && o.owner == player && o.group == group)
    }

    pub fn clear_migration_group(&mut self, player: PlayerId, group: u8) {
        self.migrate_orders.remove(&(player, group));
    }

    /// A group's migration order.
    pub fn migration_order(&self, player: PlayerId, group: u8) -> Option<Vec2> {
        self.migrate_orders.get(&(player, group)).copied()
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
        assert_eq!(
            (p.mismatch, p.cost, p.failure_chance, p.fertility),
            (out.mismatch, out.cost, out.failure_chance, out.fertility)
        );
    }

    #[test]
    fn preview_names_the_body_plan_genes_that_differ() {
        let mut w = World::new(WorldConfig { width: 48, height: 48, ..Default::default() });
        let warm = w.found_lineage(1, &FounderDesign::new("w", Thermo::Warm, BirthType::Live, 2).founders(2)).unwrap();
        let cold = w.found_lineage(2, &FounderDesign::new("c", Thermo::Cold, BirthType::Live, 2).founders(2)).unwrap();
        let sex = |w: &World, id| w.organism(id).unwrap().sex;
        let b = *cold.iter().find(|&&c| sex(&w, c) != sex(&w, warm[0])).unwrap();
        // Warm × cold: about a third as fertile (`Balance::thermo_hybrid`).
        let p = w.mating_preview(warm[0], b).unwrap();
        assert!(p.body_plan_fertility > 0.25 && p.body_plan_fertility < 0.45, "{p:?}");
        assert_eq!(p.body_plan_gaps.iter().map(|g| g.0).collect::<Vec<_>>(), vec![Gene::Endothermy]);
        w.config.balance.thermo_hybrid = 0.0;
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

    /// A player possesses one organism at a time: possessing another
    /// releases the first (and drops its order). Possession was unlimited,
    /// so a player could possess the whole lineage for the possession
    /// bonuses (10% speed, 20% perception) on every member.
    #[test]
    fn a_player_possesses_one_organism_at_a_time() {
        let (mut w, a, b) = world_with_pair(false);
        w.possess(1, a).unwrap();
        let ia = w.index[&a];
        w.order_move(1, a, Vec2::new(3.0, 3.0)).unwrap();
        w.possess(1, b).unwrap();
        let possessed: Vec<OrgId> = w.organisms.iter().filter(|o| o.possessed_by == Some(1)).map(|o| o.id).collect();
        assert_eq!(possessed, vec![b]);
        assert_eq!(w.organisms[ia].order, None);
        // Possessing the same one again changes nothing.
        w.possess(1, b).unwrap();
        assert_eq!(w.organisms.iter().filter(|o| o.possessed_by == Some(1)).count(), 1);
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
        // (Moved to the nearest tile it can reach, if it can't reach that one.)
        let target = w.order_move(1, a, target).unwrap();
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
        // Below `decide::POSSESS_DRINK`, where water near would win.
        let (mut w, a, target) = ordered(50.0);
        let before = w.organism(a).unwrap().pos.dist(target);
        w.step();
        w.step();
        let o = w.organism(a).unwrap();
        assert_eq!(o.action, crate::organism::Action::Ordered);
        assert_eq!(o.order, Some(target));
        assert!(o.pos.dist(target) < before);
    }

    /// A standard world with player 1's first organism possessed and
    /// ordered 14 tiles away.
    fn possessed_world(substeps: u32) -> (World, OrgId) {
        let config = WorldConfig { seed: 5, width: 64, height: 64, possess_substeps: substeps, ..Default::default() };
        let mut w = crate::presets::standard_world(config, 1);
        let id = w.organisms.iter().find(|o| o.owner == 1).unwrap().id;
        w.possess(1, id).unwrap();
        reorder(&mut w, id);
        (w, id)
    }

    fn reorder(w: &mut World, id: OrgId) {
        let p = w.organism(id).unwrap().pos;
        let target = w.map.clamp(Vec2::new(if p.x < 32.0 { p.x + 14.0 } else { p.x - 14.0 }, p.y));
        w.order_move(1, id, target).unwrap();
    }

    type Snap = (OrgId, bool, Vec2, Action, [u32; 4]);

    fn state(w: &World) -> Vec<Snap> {
        w.organisms
            .iter()
            .map(|o| {
                let n = &o.needs;
                (
                    o.id,
                    o.alive,
                    o.pos,
                    o.action,
                    [n.hunger.to_bits(), n.thirst.to_bits(), n.energy.to_bits(), o.health.to_bits()],
                )
            })
            .collect()
    }

    #[test]
    fn substeps_between_ticks_or_not_give_the_same_world() {
        // A server calls `substep` between ticks; the CLI and tests don't.
        // With the same orders at the same ticks the worlds are identical.
        let (mut a, id) = possessed_world(4);
        let (mut b, _) = possessed_world(4);
        for t in 0..2 * a.config.ticks_per_day {
            if t % 20 == 0 && a.organism(id).is_some_and(|o| o.alive) {
                reorder(&mut a, id);
                reorder(&mut b, id);
            }
            while a.substeps_left() > 0 {
                a.substep();
            }
            a.step();
            b.step();
        }
        assert_eq!(state(&a), state(&b));
        assert_eq!(format!("{:?}", a.counters), format!("{:?}", b.counters));
    }

    #[test]
    fn substeps_leave_unpossessed_worlds_alone() {
        // Nobody possessed: the setting changes nothing.
        let mk = |substeps| {
            let config =
                WorldConfig { seed: 5, width: 64, height: 64, possess_substeps: substeps, ..Default::default() };
            crate::presets::standard_world(config, 1)
        };
        let (mut a, mut b) = (mk(4), mk(1));
        for _ in 0..a.config.ticks_per_day {
            assert!(a.substep().is_empty());
            a.step();
            b.step();
        }
        assert_eq!(state(&a), state(&b));
    }

    #[test]
    fn a_substep_moves_only_the_possessed_organism_and_applies_new_orders() {
        let (mut w, id) = possessed_world(4);
        w.step();
        let before = state(&w);
        let p0 = w.organism(id).unwrap().pos;
        // A new order mid-tick moves it at the next sub-step, toward the
        // new target, without waiting for the tick.
        let target = w.map.clamp(p0 + Vec2::new(0.0, if p0.y < 32.0 { 10.0 } else { -10.0 }));
        w.order_move(1, id, target).unwrap();
        assert_eq!(w.substep(), vec![id]);
        let p1 = w.organism(id).unwrap().pos;
        assert!(p1.dist(target) < p0.dist(target), "{p0:?} -> {p1:?}, target {target:?}");
        assert_eq!(w.organism(id).unwrap().action, Action::Ordered);
        for (x, y) in before.iter().zip(&state(&w)) {
            if x.0 != id {
                assert_eq!(x, y, "organism {} changed in a sub-step", x.0);
            }
        }
        // Three sub-steps per tick at 4, then nothing until the tick.
        assert_eq!(w.substeps_left(), 2);
        w.substep();
        w.substep();
        assert_eq!(w.substeps_left(), 0);
        let p3 = w.organism(id).unwrap().pos;
        assert!(w.substep().is_empty());
        assert_eq!(w.organism(id).unwrap().pos, p3);
        w.step();
        assert_eq!(w.substeps_left(), 3);
    }

    #[test]
    fn substeps_are_no_faster_than_whole_ticks() {
        // Same distance per tick (within terrain rounding) as without
        // sub-steps: possession only gets its small speed bonus.
        let (mut a, id) = possessed_world(4);
        let (mut b, _) = possessed_world(1);
        let start = a.organism(id).unwrap().pos;
        for _ in 0..3 {
            while a.substeps_left() > 0 {
                a.substep();
            }
            a.step();
            b.step();
        }
        let da = a.organism(id).unwrap().pos.dist(start);
        let db = b.organism(id).unwrap().pos.dist(start);
        assert!(db > 0.5, "didn't move without sub-steps: {db}");
        assert!((da - db).abs() <= 0.1 * db, "with sub-steps {da}, without {db}");
        assert_eq!(b.substeps_left(), 0);
        assert!(b.substep().is_empty());
    }

    #[test]
    fn severe_need_overrides_substep_movement() {
        let (mut w, id) = possessed_world(4);
        w.step();
        let i = w.index[&id];
        w.organisms[i].needs.thirst = 95.0;
        let p = w.organisms[i].pos;
        assert!(w.substep().is_empty());
        assert_eq!(w.organisms[i].pos, p);
    }
}
