//! Group commands beyond migration ("Player controls" in the design doc:
//! "claim or abandon territory, split a group").
//!
//! **Groups.** Each of a player's organisms belongs to one group
//! (`Organism::group`): 0, the main group, until the player splits it.
//! [`World::split_group`] divides a group in two, either the organisms the
//! player names or, by default, the half that lies on one side of the
//! group's widest spread. Up to [`MAX_GROUPS`] groups; young are born into
//! their mother's group; [`World::merge_groups`] joins two again. Migration
//! orders and territories are given per group, so a split herd can be sent
//! two ways. Members of the same player's other groups don't count as kin
//! for cohesion (`Action::Group`), herd size or the social need, so the two
//! halves don't drift back together.
//!
//! **Territory.** [`World::claim_territory`] gives a group a circle to hold.
//! Its members roam and forage inside it (wander targets, food searches and
//! remembered patches outside are dropped), walk back when outside
//! (`Action::Patrol`), and adults chase strangers they see inside it
//! (`Action::Defend`: anything not of the player's lineage and not kin that
//! couldn't beat them). A stranger inside a territory sees a defender coming
//! at it as a threat and flees. The costs, in the design doc's spirit (a
//! territory is something "year-round active" lineages hold, and every lever
//! has a price): the group is confined to the food inside the circle until
//! hunger passes [`STRAY_HUNGER`], chasing burns energy at a sprint, the
//! circle is capped by group size ([`World::territory_radius_limit`]: a few
//! animals can't hold a valley), and a group can claim only once every
//! [`CLAIM_COOLDOWN_DAYS`]. Seasonal migration (genes) still wins over it.
//!
//! **Obedience** (group commands as genes, the design doc's open
//! question). How strongly each organism heeds its group's orders is a
//! heritable instinct, `Gene::Obedience`: [`heed`] scales the utility of an
//! ordered migration, of walking back into a territory and of seeing off
//! an intruder (1 at the design default, 0.5, where the commands were
//! tuned), and [`stray_hunger`] moves the hunger at which a territory
//! holder forages outside. Obedient groups follow orders through hunger
//! (and starve under bad ones); headstrong ones break off to eat. The price
//! is tameness: every animal's urge to flee is weighted by
//! `1 − Balance::tameness × (obedience − 0.5)`, so in a lineage whose player
//! gives no orders, obedience is a cost and selection can wear it down.
//!
//! **Rally.** [`World::rally_group`] gathers a group at a point: members
//! farther than the rally circle's radius ([`World::rally_radius`], which
//! grows with the group) walk to it (`Action::Rally`, weighed like the
//! other orders: [`RALLY_UTILITY`] × [`heed`]) and, once there, roam and
//! forage inside it until the order is cancelled ([`World::cancel_rally`]),
//! replaced by another order, or expires after [`RALLY_DAYS`]. Like a
//! territory, the circle holds them only while hunger is below
//! [`stray_hunger`]; unlike one, it isn't defended and has no cooldown. A
//! group has at most one of a migration order, a territory and a rally: the
//! newest replaces the others.
//!
//! No command draws randomness, and a world where nobody uses them
//! steps exactly as before (every lookup is behind an empty map, every
//! organism is in group 0).

use std::collections::{BTreeMap, BTreeSet};

use super::commands::CommandError;
use super::World;
use crate::genome::{Gene, PlayerId};
use crate::math::Vec2;
use crate::organism::{OrgId, Organism};

/// Groups a player can have at once (ids `0..MAX_GROUPS`).
pub const MAX_GROUPS: u8 = 4;
/// Smallest territory radius in tiles (smaller claims are rounded up).
pub const TERRITORY_MIN_RADIUS: f32 = 3.0;
/// Largest territory radius whatever the group's size.
pub const TERRITORY_MAX_RADIUS: f32 = 24.0;
/// Radius a group of no adults may hold; each adult adds to it with the
/// square root of their number ([`TERRITORY_RADIUS_PER_ADULT`]), so the
/// area held grows with the group.
pub const TERRITORY_BASE_RADIUS: f32 = 4.0;
pub const TERRITORY_RADIUS_PER_ADULT: f32 = 2.0;
/// Days before a group can claim a territory again.
pub const CLAIM_COOLDOWN_DAYS: f32 = 0.5;
/// Hunger (0–1) above which members forage and roam outside their
/// territory ("Hungry organisms prioritize food over everything except
/// immediate danger").
pub const STRAY_HUNGER: f32 = 0.8;
/// Utility of walking back into the territory (an ordered migration is 0.55).
pub const RETURN_UTILITY: f32 = 0.55;
/// Utility of chasing an intruder at full aggression or boldness and no hunger.
pub const DEFEND_UTILITY: f32 = 0.7;
/// How frightening a defender is to the intruder it chases, against an
/// armed hunter of the same power (1).
pub const DEFENDER_SCARE: f32 = 1.0;
/// How far obedience moves the hunger at which a territory holder strays:
/// [`STRAY_HUNGER`] `+ OBEDIENT_STRAY × (obedience − 0.5)`, so 0.65 for
/// the headstrong and 0.95 for the most obedient.
pub const OBEDIENT_STRAY: f32 = 0.3;
/// Days a rally order stands before it lapses (re-issue it to keep the
/// group there).
pub const RALLY_DAYS: f32 = 3.0;
/// Utility of walking to the rally point (an ordered migration is 0.55).
pub const RALLY_UTILITY: f32 = 0.55;
/// Utility of walking back into the home circle a group keeps after a
/// migration ([`Settle`]): weaker than a rally (wandering, cohesion and
/// every need still win when they press).
pub const SETTLE_UTILITY: f32 = 0.35;
/// A territory claim farther than this from its group's middle becomes a
/// migration that claims on arrival ([`World::claim_or_migrate`]): a
/// claim far from the group used to confine nobody, yet counted as its
/// home range.
pub const FAR_CLAIM_TILES: f32 = 30.0;
/// A territory whose standing plants would feed its group for fewer days
/// than this, or with no drinking water, may be moved at once, without
/// the claim cooldown (a big herd strips a territory faster than the wait).
pub const TERRITORY_BARE_DAYS: f32 = 3.0;
/// Days over which [`World::food_check`] spreads the plants standing now,
/// on top of the regrowth at the current season's pace.
pub const FOOD_HORIZON_DAYS: f32 = 30.0;
/// Share of a territory's holders starving or parched (hunger or thirst
/// at 80) at which the hold loosens: the group forages up to
/// [`HOLD_LOOSE_REACH`] beyond the edge (playtest 7: a held territory
/// trapped a starving herd, 1,144 → 449, until it was abandoned).
pub const HOLD_LOOSEN_SHARE: f32 = 0.15;
/// Share under which a loosened hold tightens again.
pub const HOLD_RESTORE_SHARE: f32 = 0.05;
/// How far past its edge a loosened territory's group forages: its own
/// radius, at least this many tiles.
pub const HOLD_LOOSE_REACH: f32 = 10.0;
/// A rival territory held by at most this many animals is a squat a much
/// larger group may claim over ([`SQUAT_RATIO`]): playtest 7 found a
/// 2-animal splinter could deny a key water spot to every rival for good.
pub const SQUAT_MEMBERS: usize = 12;
/// How many times a squat's holders a claiming group must number to take
/// its ground.
pub const SQUAT_RATIO: usize = 10;
/// A rival territory that feeds at least this many times its holders is
/// more ground than they need ([`World::food_check`]): a group
/// [`OVERCLAIM_RATIO`] times their number may claim over it, however many
/// adults hold it (playtest 8: a breeding pair grew past 12 on a key water
/// spot and could never be displaced).
pub const OVERCLAIM_FOOD: usize = 4;
/// How many times an over-large territory's holders a claiming group must
/// number to take its ground.
pub const OVERCLAIM_RATIO: usize = 2;
/// Margin on a group's daily plant need for what [`World::food_check`]
/// leaves out (activity, pregnancy, the cold).
pub const FOOD_NEED_MARGIN: f32 = 1.15;
/// Least share of the time a hungry hunter takes a prey animal for
/// [`World::prey_check`] to count it.
const PREY_TAKEN_MIN: f32 = 0.05;
/// How far beyond a circle's edge prey still counts ([`World::prey_check`]).
pub const PREY_REACH: f32 = 8.0;
/// Hunger (0–1) a migrant that stopped to eat grazes down to before it
/// walks on (it stops to eat at [`stray_hunger`]).
pub const MIGRANT_EAT_RESUME: f32 = 0.5;
/// A migration's headway counts once its median member is this share of
/// the starting distance closer than its best (at least
/// [`MIGRATE_PROGRESS_TILES`]): a big herd's median wanders by a tile or
/// two, which kept long orders that weren't moving alive for weeks.
pub const MIGRATE_PROGRESS_SHARE: f32 = 0.03;
/// Share of the cooldown left after abandoning a territory: a regretted
/// claim can be replaced after a short wait, not the whole cooldown.
pub const ABANDON_COOLDOWN_SHARE: f32 = 0.25;
/// Tiles either side of a route counted as food on the way
/// ([`World::journey_check`]).
pub const JOURNEY_CORRIDOR: usize = 3;
/// Largest arrival radius whatever the group's size: a few tiles, so a
/// big herd has really come (playtest 3: with the rally radius, up to 24
/// tiles, a 486-strong group "arrived" with its median 24–29 tiles out).
pub const MIGRATE_ARRIVAL_MAX: f32 = 8.0;
/// A migration ends once its group's median member has made no headway
/// (at least [`MIGRATE_PROGRESS_TILES`] closer than its best) for this many
/// days ([`GroupEvent::TimedOut`](super::commands::GroupEvent::TimedOut)):
/// scattered or blocked groups stop, long journeys that keep moving don't.
pub const MIGRATE_STALL_DAYS: f32 = 5.0;
pub const MIGRATE_PROGRESS_TILES: f32 = 1.0;
/// The hard limit on any migration: `MIGRATE_TIMEOUT_BASE_DAYS + distance
/// / MIGRATE_TIMEOUT_TILES_PER_DAY` days (distance from the group's middle
/// when ordered), kept between the two bounds.
pub const MIGRATE_TIMEOUT_BASE_DAYS: f32 = 5.0;
pub const MIGRATE_TIMEOUT_TILES_PER_DAY: f32 = 2.0;
pub const MIGRATE_TIMEOUT_MIN_DAYS: f32 = 6.0;
pub const MIGRATE_TIMEOUT_MAX_DAYS: f32 = 120.0;
/// A territory or rally circle with more than this share of its tiles in
/// water the group can't stand in is refused
/// ([`CommandError::MostlyWater`]).
pub const MAX_WATER_SHARE: f32 = 0.5;
/// How far beyond a circle's edge (tiles) drinking water still counts as
/// the circle's own ([`WaterCheck::drinkable`]).
pub const WATER_REACH: f32 = 4.0;
/// Radius of a rally circle for a lone member; each member adds to it with
/// the square root of their number ([`RALLY_RADIUS_PER_MEMBER`]), so the
/// area grows with the group (about π tiles per member) and a big herd
/// gathers with room to graze.
pub const RALLY_BASE_RADIUS: f32 = 2.0;
pub const RALLY_RADIUS_PER_MEMBER: f32 = 1.0;
/// Largest rally circle whatever the group's size: the largest territory
/// ([`TERRITORY_MAX_RADIUS`]), so a rally never holds a group tighter than
/// the circle it could claim (the cap is reached at 484 members; it was
/// 12 tiles, reached at 100, which crammed a 400-head herd onto ~1 tile
/// each).
pub const RALLY_MAX_RADIUS: f32 = TERRITORY_MAX_RADIUS;

/// How strongly `o` heeds its group's commands (`Gene::Obedience`): the
/// factor on the utility of an ordered migration, of walking back into a
/// territory and of seeing off an intruder. 1 at the design default (0.5),
/// as the commands were tuned; from 0.5 (orders only when nothing else
/// presses) to 1.5 (orders over all but danger and severe need).
#[inline]
pub(crate) fn heed(o: &Organism) -> f32 {
    0.5 + o.pheno[Gene::Obedience]
}

/// The need (0–1) at which `o` sets an order aside to drink or eat
/// ([`MIGRATE_BREAK`](super::commands::MIGRATE_BREAK), moved by obedience).
#[inline]
pub(crate) fn need_limit(o: &Organism) -> f32 {
    use super::commands::{MIGRATE_BREAK, MIGRATE_BREAK_OBEDIENCE};
    MIGRATE_BREAK + MIGRATE_BREAK_OBEDIENCE * (o.pheno[Gene::Obedience] - 0.5)
}

/// Whether thirst has `o` leave its group's territory or rally circle to
/// drink: at [`need_limit`], or while it is drinking until thirst is down
/// to [`ORDER_RESUME`](super::commands::ORDER_RESUME), as a migrant breaks
/// off (a drought used to hold groups inside dry circles until they died).
#[inline]
pub(crate) fn thirst_breaks_off(o: &Organism) -> bool {
    let thirst = o.needs.thirst / 100.0;
    thirst >= need_limit(o)
        || (o.action == crate::organism::Action::Drink && thirst > super::commands::ORDER_RESUME / 100.0)
}

/// Hunger (0–1) above which `o` forages outside its group's territory.
#[inline]
pub(crate) fn stray_hunger(o: &Organism) -> f32 {
    STRAY_HUNGER + OBEDIENT_STRAY * (o.pheno[Gene::Obedience] - 0.5)
}

/// A circle a group holds.
#[derive(Clone, Copy, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Territory {
    pub center: Vec2,
    pub radius: f32,
    pub claimed_tick: u64,
}

impl Territory {
    pub fn contains(&self, p: Vec2) -> bool {
        self.center.dist(p) <= self.radius
    }
}

/// A point a group gathers at (`World::rally_group`).
#[derive(Clone, Copy, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Rally {
    pub center: Vec2,
    /// Members farther than this walk to the point; inside it they hold.
    pub radius: f32,
    pub issued_tick: u64,
    /// First tick at which the order no longer stands.
    pub expires_tick: u64,
}

impl Rally {
    pub fn contains(&self, p: Vec2) -> bool {
        self.center.dist(p) <= self.radius
    }
}

/// What a migration order carries besides its target (snapshot version 41).
#[derive(Clone, Copy, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct MigrationPlan {
    pub issued_tick: u64,
    /// The order ends at this tick at the latest if the group hasn't
    /// arrived ([`World::migration_timeout_ticks`]).
    pub deadline_tick: u64,
    /// The closest the group's median member has come to the target
    /// (version 45; infinite until first measured).
    #[cfg_attr(feature = "serde", serde(with = "crate::serde_util::before_v45::infinite"))]
    pub best_median: f32,
    /// When it last came [`MIGRATE_PROGRESS_TILES`] closer (version 45; 0
    /// until first measured).
    #[cfg_attr(feature = "serde", serde(with = "crate::serde_util::before_v45::zero"))]
    pub progress_tick: u64,
}

impl MigrationPlan {
    /// A plan for an order given at `now` with the hard limit `deadline_tick`.
    pub fn new(now: u64, deadline_tick: u64) -> Self {
        MigrationPlan { issued_tick: now, deadline_tick, best_median: f32::INFINITY, progress_tick: now }
    }

    /// When the order ends unless the group gets closer first: the stall
    /// limit from its last headway, or the hard limit if sooner.
    pub fn ends_tick(&self, ticks_per_day: u32) -> u64 {
        let stall = (MIGRATE_STALL_DAYS * ticks_per_day as f32).round() as u64;
        self.progress_tick.max(self.issued_tick).saturating_add(stall).min(self.deadline_tick)
    }
}

/// The home a group keeps where its last migration arrived: members
/// outside the circle walk back into it (`Action::Rally` at
/// [`SETTLE_UTILITY`]) when nothing presses, until the group gets another
/// order (`until_tick` is `u64::MAX`; settle circles saved by version 41
/// to 44 builds lapse at theirs). Not a confinement: they forage and drink
/// anywhere.
#[derive(Clone, Copy, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Settle {
    pub center: Vec2,
    pub radius: f32,
    pub until_tick: u64,
}

/// Migration plans and settle circles by (player, group) (snapshot
/// version 41: empty from older snapshots).
#[derive(Clone, Debug, Default, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct GroupPlans {
    pub migrations: BTreeMap<(PlayerId, u8), MigrationPlan>,
    pub settles: BTreeMap<(PlayerId, u8), Settle>,
    /// Version 48: territory claims waiting on a migration and where
    /// migrations started (empty from older snapshots).
    #[cfg_attr(feature = "serde", serde(with = "crate::serde_util::before_v48"))]
    pub pending: PendingPlans,
    /// Version 66: loosened holds and territories kept while away (empty
    /// from older snapshots).
    #[cfg_attr(feature = "serde", serde(with = "crate::serde_util::before_v66"))]
    pub held: HeldPlans,
}

/// What snapshot version 66 added to [`GroupPlans`].
#[derive(Clone, Debug, Default, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct HeldPlans {
    /// Territories whose holders are starving or parched
    /// ([`HOLD_LOOSEN_SHARE`]): the group forages beyond the edge
    /// ([`World::roam_bound`]) until they recover ([`HOLD_RESTORE_SHARE`]).
    pub loosened: BTreeSet<(PlayerId, u8)>,
    /// The territory a group held when it was ordered to migrate: still
    /// its ground (public, and no rival may claim over it) until the group
    /// arrives or claims anew, and held again if the order is cleared
    /// (playtest 7: a probe migrate cost the territory and the cooldown
    /// locked it out).
    pub away: BTreeMap<(PlayerId, u8), Territory>,
}

/// What snapshot version 48 added to [`GroupPlans`].
#[derive(Clone, Debug, Default, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct PendingPlans {
    /// A far territory claim's circle (centre, radius asked), claimed when
    /// the migration to it arrives ([`World::claim_or_migrate`]).
    pub claims: BTreeMap<(PlayerId, u8), (Vec2, f32)>,
    /// A migration's median member distance when first measured: whether
    /// it ever got going.
    pub start_median: BTreeMap<(PlayerId, u8), f32>,
}

/// What [`World::claim_or_migrate`] did.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ClaimOutcome {
    /// Claimed at once.
    Claimed(Territory),
    /// Too far from the group: it migrates there first and claims on
    /// arrival. `radius` is the one the claim gets now (kept within the
    /// group's limit; it is kept to the limit again on arrival). Playtest
    /// 7: a far claim was rated at a typed radius of 300.
    Migrating { target: Vec2, radius: f32 },
}

/// Prey around a meat-eating group's circle ([`World::prey_check`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PreyCheck {
    /// Prey the group could take on instinct in the circle or within
    /// [`PREY_REACH`] of its edge: other lineages' plant eaters, not down a
    /// burrow, not under a newcomer's grace, not passed up as poisonous,
    /// not too heavy for the pack.
    pub prey: usize,
    pub members: usize,
    /// About how many of the members that prey feeds for good: a
    /// sustainable take of [`PREY_HARVEST`] of it a day against each
    /// member's daily need.
    pub feeds: usize,
}

/// Share of a prey population a pack can take a day and still find as much
/// prey the next ([`World::prey_check`]).
pub const PREY_HARVEST: f32 = 0.03;

/// Plant food in a circle against a group's needs ([`World::food_check`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FoodCheck {
    /// About how many of the group's animals the circle feeds: today's
    /// regrowth (season, snow, drought) plus the standing plants spread over
    /// [`FOOD_HORIZON_DAYS`].
    pub supports: usize,
    pub members: usize,
    /// Days the plants standing in it now would feed the whole group.
    pub standing_days: f32,
    /// The plants the circle grows back in a day now (its yield), less
    /// the share other grazers in it take.
    pub regrowth: f32,
    /// Other plant eaters grazing in the circle now (wild, rivals, the
    /// player's other groups); their need is taken out of the numbers above.
    pub others: usize,
}

/// How far a group's walk takes it and what the land along the way holds
/// ([`World::journey_check`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct JourneyCheck {
    pub tiles: f32,
    /// About how many days it takes ([`World::group_pace`]).
    pub days: f32,
    /// The plants standing along the way against what the group eats in
    /// those days (1: just enough; below, hunger on the way).
    pub food_share: f32,
    pub members: usize,
}

/// Water in and around a territory or rally circle ([`World::water_check`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WaterCheck {
    /// Share of the circle's tiles in water the group can't stand in.
    pub water_share: f32,
    /// Drinking spots in the circle or within [`WATER_REACH`] of its edge:
    /// tiles the group can stand on beside drinkable water (snow aside: in
    /// winter it made nearly every tile a "drinking spot", playtest 8's
    /// 912 in a 17-tile circle, and it melts).
    pub drinkable: usize,
}

/// One of a player's groups, for reports (`World::groups`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GroupInfo {
    pub id: u8,
    /// Living agents in the group (cohort members of abstract regions with
    /// level of detail on belong to no group).
    pub members: usize,
    pub adults: usize,
    pub migration: Option<Vec2>,
    pub territory: Option<Territory>,
    /// The rally the group is answering, if any.
    pub rally: Option<Rally>,
    /// Largest territory the group could claim now.
    pub max_radius: f32,
    /// First tick at which the group may claim a territory (0: now).
    pub claim_ready_tick: u64,
    /// Mean expressed obedience (`Gene::Obedience`) of its members, rounded
    /// to hundredths (0 with no members): how strongly it heeds orders.
    pub obedience: f32,
    /// The tick its migration ends if it hasn't arrived by then.
    pub migration_deadline_tick: Option<u64>,
    /// Where it settles after arriving from a migration, while that lasts.
    pub settle: Option<Settle>,
    /// The territory it left behind on a migration and still holds
    /// ([`HeldPlans::away`]).
    pub territory_away: Option<Territory>,
}

/// What [`World::split_group`] did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SplitOutcome {
    /// The new group.
    pub group: u8,
    /// Organisms moved into it.
    pub moved: usize,
    /// Organisms left in the group split.
    pub kept: usize,
}

impl World {
    /// The territory `o`'s group holds, if any.
    #[inline]
    pub(crate) fn territory_of(&self, o: &Organism) -> Option<&Territory> {
        if self.territories.is_empty() {
            return None;
        }
        self.territories.get(&(o.owner, o.group))
    }

    /// The rally `o`'s group is answering, if any (and still standing).
    #[inline]
    pub(crate) fn rally_of(&self, o: &Organism) -> Option<&Rally> {
        if self.rallies.is_empty() {
            return None;
        }
        self.rallies.get(&(o.owner, o.group)).filter(|r| self.tick < r.expires_tick)
    }

    /// The circle `o` roams and forages in: its group's territory or rally
    /// circle, unless it is hungry enough to look further ([`stray_hunger`]).
    #[inline]
    pub(crate) fn roam_bound(&self, o: &Organism) -> Option<(Vec2, f32)> {
        let (c, r) = match self.territory_of(o) {
            Some(t) if self.group_plans.held.loosened.contains(&(o.owner, o.group)) => {
                (t.center, t.radius + t.radius.max(HOLD_LOOSE_REACH))
            }
            Some(t) => (t.center, t.radius),
            None => {
                let r = self.rally_of(o)?;
                (r.center, r.radius)
            }
        };
        (o.needs.hunger / 100.0 <= stray_hunger(o) && !thirst_breaks_off(o)).then_some((c, r))
    }

    /// Where `o` looks for food and wanders: its territory or rally circle
    /// ([`World::roam_bound`]), else twice its home circle while it is not
    /// hungry or thirsty enough to range further ([`need_limit`]), so a herd
    /// doesn't forage its way back to where it came from.
    #[inline]
    pub(crate) fn forage_bound(&self, o: &Organism) -> Option<(Vec2, f32)> {
        if let Some(b) = self.roam_bound(o) {
            return Some(b);
        }
        if self.territory_of(o).is_some() || self.rally_of(o).is_some() {
            return None;
        }
        let s = self.settle_of(o)?;
        let free = o.needs.hunger / 100.0 >= need_limit(o) || thirst_breaks_off(o);
        (!free).then_some((s.center, 2.0 * s.radius))
    }

    /// The settle circle `o`'s group keeps after arriving, if any and
    /// still standing.
    #[inline]
    pub(crate) fn settle_of(&self, o: &Organism) -> Option<&Settle> {
        if self.group_plans.settles.is_empty() {
            return None;
        }
        self.group_plans.settles.get(&(o.owner, o.group)).filter(|s| self.tick < s.until_tick)
    }

    /// The settle circle a group keeps after arriving, if still standing.
    pub fn settle_circle(&self, player: PlayerId, group: u8) -> Option<Settle> {
        self.group_plans.settles.get(&(player, group)).copied().filter(|s| self.tick < s.until_tick)
    }

    /// The deadline of a group's migration order, if it has one.
    pub fn migration_plan(&self, player: PlayerId, group: u8) -> Option<MigrationPlan> {
        self.migration_order(player, group)?;
        self.group_plans.migrations.get(&(player, group)).copied()
    }

    /// Ticks a migration over `dist` tiles may take at most (the hard limit;
    /// it ends sooner if it stalls, [`MIGRATE_STALL_DAYS`]).
    pub fn migration_timeout_ticks(&self, dist: f32) -> u64 {
        let days = (MIGRATE_TIMEOUT_BASE_DAYS + dist / MIGRATE_TIMEOUT_TILES_PER_DAY)
            .clamp(MIGRATE_TIMEOUT_MIN_DAYS, MIGRATE_TIMEOUT_MAX_DAYS);
        ((days * self.config.ticks_per_day as f32).round() as u64).max(1)
    }

    /// Where a group's living members are on average, if it has any.
    pub fn group_middle(&self, player: PlayerId, group: u8) -> Option<Vec2> {
        let (sum, n) = self
            .organisms
            .iter()
            .filter(|o| o.alive && o.owner == player && o.group == group)
            .fold((Vec2::ZERO, 0usize), |(s, n), o| (s + o.pos, n + 1));
        (n > 0).then(|| sum * (1.0 / n as f32))
    }

    /// Drop rally orders that have lapsed. Runs once per tick, before decide.
    pub(crate) fn settle_rallies(&mut self) {
        if !self.rallies.is_empty() {
            let now = self.tick;
            self.rallies.retain(|_, r| now < r.expires_tick);
        }
        if !self.group_plans.settles.is_empty() {
            let now = self.tick;
            self.group_plans.settles.retain(|_, s| now < s.until_tick);
        }
    }

    /// Radius of the circle a group of `members` gathers in at a rally.
    pub fn rally_radius(members: usize) -> f32 {
        (RALLY_BASE_RADIUS + RALLY_RADIUS_PER_MEMBER * (members as f32).sqrt()).min(RALLY_MAX_RADIUS)
    }

    /// How long a rally stands: [`RALLY_DAYS`], or the host's floor in
    /// ticks ([`World::set_rally_min_ticks`]) if longer.
    pub fn rally_ticks(&self) -> u64 {
        ((RALLY_DAYS * self.config.ticks_per_day as f32).round() as u64).max(self.rally_min_ticks).max(1)
    }

    /// A floor on how long a rally stands, in ticks: a server sets it to a
    /// few minutes of real time, since [`RALLY_DAYS`] pass in seconds at a
    /// fast tick rate. Not saved with the world (the host sets it again
    /// after a restore); a standing rally keeps the expiry it was given.
    pub fn set_rally_min_ticks(&mut self, ticks: u64) {
        self.rally_min_ticks = ticks;
    }

    /// Tiles from its migration target within which a group of `members`
    /// has arrived (its median member): [`MIGRATE_ARRIVAL`](super::commands::MIGRATE_ARRIVAL)
    /// plus a quarter tile per square root of members, at most
    /// [`MIGRATE_ARRIVAL_MAX`] (room for half a big herd to stand).
    pub fn arrival_radius(members: usize) -> f32 {
        (super::commands::MIGRATE_ARRIVAL + 0.25 * (members as f32).sqrt()).min(MIGRATE_ARRIVAL_MAX)
    }

    /// The home circle a group keeps after arriving: 1.5× the arrival
    /// radius (at most 12 tiles), centred on the target. Small on purpose:
    /// the rally circle (up to 24 tiles) could hold the place a big herd
    /// came from, and it walked straight back to it (playtest 4).
    pub fn home_radius(members: usize) -> f32 {
        1.5 * Self::arrival_radius(members)
    }

    /// The median distance of a group's living members from `p`.
    pub fn group_median_dist(&self, player: PlayerId, group: u8, p: Vec2) -> Option<f32> {
        let mut d: Vec<f32> = self
            .organisms
            .iter()
            .filter(|o| o.alive && o.owner == player && o.group == group)
            .map(|o| o.pos.dist(p))
            .collect();
        if d.is_empty() {
            return None;
        }
        let mid = d.len() / 2;
        Some(*d.select_nth_unstable_by(mid, f32::total_cmp).1)
    }

    /// The share of a group's living members deep in a dormancy
    /// (hibernating, brumating, estivating): they can't walk to a migration
    /// target.
    pub fn dormant_share(&self, player: PlayerId, group: u8) -> f32 {
        let (n, dormant) = self
            .organisms
            .iter()
            .filter(|o| o.alive && o.owner == player && o.group == group)
            .fold((0usize, 0usize), |(n, d), o| (n + 1, d + usize::from(o.dormancy.is_deep())));
        if n == 0 {
            0.0
        } else {
            dormant as f32 / n as f32
        }
    }

    /// Once per hour (and only with group orders standing): a migration
    /// whose group's median member is within [`World::arrival_radius`] of
    /// its target is done (the target becomes the group's home,
    /// [`World::home_radius`]), one that has stalled for
    /// [`MIGRATE_STALL_DAYS`] or passed its hard limit
    /// ([`World::migration_timeout_ticks`]) is ended, and a group with no
    /// living members loses its migration, territory and rally. The main
    /// group only counts as empty once the player has nothing alive at all
    /// (its agents may all be away as cohorts in abstract regions, where
    /// they belong to no group). Each change is a [`GroupEvent`] for the
    /// server.
    pub(crate) fn settle_groups(&mut self) {
        use super::commands::GroupEvent;
        if self.migrate_orders.is_empty() && self.territories.is_empty() && self.rallies.is_empty() {
            return;
        }
        if !self.tick.is_multiple_of(self.config.ticks_per_hour().max(1)) {
            return;
        }
        self.group_plans.migrations.retain(|k, _| self.migrate_orders.contains_key(k));
        let orders = &self.migrate_orders;
        self.group_plans.pending.claims.retain(|k, _| orders.contains_key(k));
        self.group_plans.pending.start_median.retain(|k, _| orders.contains_key(k));
        self.group_plans.held.away.retain(|k, _| orders.contains_key(k));
        let held = &self.territories;
        self.group_plans.held.loosened.retain(|k| held.contains_key(k));
        // Members per group with an order; each migrating member's distance
        // to its group's target.
        let mut members: BTreeMap<(PlayerId, u8), usize> = BTreeMap::new();
        for &k in self.migrate_orders.keys().chain(self.territories.keys()).chain(self.rallies.keys()) {
            members.insert(k, 0);
        }
        let mut dists: BTreeMap<(PlayerId, u8), Vec<f32>> = BTreeMap::new();
        let mut dormant: BTreeMap<(PlayerId, u8), usize> = BTreeMap::new();
        let mut basking: BTreeMap<(PlayerId, u8), usize> = BTreeMap::new();
        let mut needy: BTreeMap<(PlayerId, u8), usize> = BTreeMap::new();
        // Holders starving (hunger at 80) and parched (thirst at 80).
        let mut starving: BTreeMap<(PlayerId, u8), (usize, usize)> = BTreeMap::new();
        for o in self.organisms.iter().filter(|o| o.alive) {
            let key = (o.owner, o.group);
            if let Some(n) = members.get_mut(&key) {
                *n += 1;
                if self.territories.contains_key(&key) {
                    let e = starving.entry(key).or_default();
                    e.0 += usize::from(o.needs.hunger >= 80.0);
                    e.1 += usize::from(o.needs.thirst >= 80.0);
                }
                if let Some(&target) = self.migrate_orders.get(&key) {
                    dists.entry(key).or_default().push(o.pos.dist(target));
                    if o.dormancy.is_deep() {
                        *dormant.entry(key).or_default() += 1;
                    }
                    if o.action == crate::organism::Action::Bask {
                        *basking.entry(key).or_default() += 1;
                    }
                    if o.needs.thirst / 100.0 >= need_limit(o) || o.needs.hunger / 100.0 >= stray_hunger(o) {
                        *needy.entry(key).or_default() += 1;
                    }
                }
            }
        }
        for (key, n) in members {
            let (player, group) = key;
            let lineage_gone = || self.owner_count(player) == 0 && self.lod.owner_count(player) == 0;
            if n == 0 && (group != 0 || lineage_gone()) {
                self.migrate_orders.remove(&key);
                self.territories.remove(&key);
                self.rallies.remove(&key);
                self.group_plans.settles.remove(&key);
                self.group_plans.held.away.remove(&key);
                self.group_plans.held.loosened.remove(&key);
                self.push_group_event(GroupEvent::Emptied { player, group });
                continue;
            }
            if let Some(&(hungry, parched)) = starving.get(&key) {
                // Re-checked every hour: a claim made before a drought or a
                // lean season doesn't hold a starving group on bare ground.
                let n = n.max(1) as f32;
                let bad = hungry.max(parched) as f32 / n;
                let loose = self.group_plans.held.loosened.contains(&key);
                if !loose && bad >= HOLD_LOOSEN_SHARE {
                    self.group_plans.held.loosened.insert(key);
                    self.push_group_event(GroupEvent::HoldLoosened {
                        player,
                        group,
                        hungry_share: hungry as f32 / n,
                        parched_share: parched as f32 / n,
                    });
                } else if loose && bad < HOLD_RESTORE_SHARE {
                    self.group_plans.held.loosened.remove(&key);
                    self.push_group_event(GroupEvent::HoldRestored { player, group });
                }
            }
            let Some(&target) = self.migrate_orders.get(&key) else { continue };
            // The group's actual spread: arrived once its median member is
            // within the arrival radius (half the group there, not the
            // first few).
            let mut d = dists.remove(&key).unwrap_or_default();
            let median = if d.is_empty() {
                f32::INFINITY
            } else {
                let mid = d.len() / 2;
                *d.select_nth_unstable_by(mid, f32::total_cmp).1
            };
            let radius = Self::arrival_radius(n);
            if median <= radius {
                self.migrate_orders.remove(&key);
                self.group_plans.migrations.remove(&key);
                // The target becomes the group's home until its next order.
                let settle = Settle { center: target, radius: Self::home_radius(n), until_tick: u64::MAX };
                self.group_plans.settles.insert(key, settle);
                self.group_plans.pending.start_median.remove(&key);
                // The ground it left is given up now it has arrived.
                self.group_plans.held.away.remove(&key);
                match self.group_plans.pending.claims.remove(&key) {
                    Some((center, radius)) => {
                        let adults = self.group_counts(player, group).1;
                        let t = Territory {
                            center,
                            radius: radius.clamp(TERRITORY_MIN_RADIUS, Self::territory_radius_limit(adults)),
                            claimed_tick: self.tick,
                        };
                        // A rival may have claimed there on the way.
                        let Ok(squats) = self.claim_squats(player, group, t.center, t.radius) else {
                            self.push_group_event(GroupEvent::ClaimBlocked { player, group, target });
                            continue;
                        };
                        self.displace_squats(&squats, player);
                        self.territories.insert(key, t);
                        self.territory_claims.insert(key, self.tick);
                        self.rallies.remove(&key);
                        self.push_group_event(GroupEvent::ClaimedOnArrival { player, group, territory: t });
                    }
                    None => self.push_group_event(GroupEvent::Arrived { player, group, target }),
                }
                continue;
            }
            let now = self.tick;
            let tpd = self.config.ticks_per_day;
            let plan = match self.group_plans.migrations.get(&key).copied() {
                Some(p) => p,
                None => {
                    // An order from before plans were kept: its time runs from now.
                    let dist = self.group_middle(player, group).map_or(0.0, |m| m.dist(target));
                    MigrationPlan::new(now, now + self.migration_timeout_ticks(dist))
                }
            };
            let mut plan = plan;
            if median.is_finite() {
                self.group_plans.pending.start_median.entry(key).or_insert(median);
            }
            let start = self.group_plans.pending.start_median.get(&key).copied().unwrap_or(median);
            let step = MIGRATE_PROGRESS_TILES.max(MIGRATE_PROGRESS_SHARE * start);
            if !plan.best_median.is_finite() || median < plan.best_median - step {
                // Headway (or the first measure, from a plan saved before it was kept).
                if plan.best_median.is_finite() || plan.progress_tick == 0 {
                    plan.progress_tick = now;
                }
                plan.best_median = median;
            }
            self.group_plans.migrations.insert(key, plan);
            if now >= plan.ends_tick(tpd) {
                self.migrate_orders.remove(&key);
                self.group_plans.migrations.remove(&key);
                let median = if median.is_finite() { median } else { 0.0 };
                let share =
                    |m: &BTreeMap<(PlayerId, u8), usize>| m.get(&key).copied().unwrap_or(0) as f32 / n.max(1) as f32;
                let start = self.group_plans.pending.start_median.remove(&key).unwrap_or(plan.best_median);
                let gained = (start - plan.best_median).max(0.0);
                let never_moved = gained < MIGRATE_PROGRESS_TILES;
                let pending_claim = self.group_plans.pending.claims.remove(&key).is_some();
                // Still on the ground it left: it holds it again.
                let territory_kept = match self.group_plans.held.away.remove(&key) {
                    Some(t) if self.group_median_dist(player, group, t.center).is_some_and(|d| d <= t.radius) => {
                        self.territories.insert(key, t);
                        true
                    }
                    _ => false,
                };
                self.push_group_event(GroupEvent::TimedOut {
                    player,
                    group,
                    target,
                    median,
                    dormant_share: share(&dormant),
                    basking_share: share(&basking),
                    needy_share: share(&needy),
                    never_moved,
                    gained,
                    pending_claim,
                    territory_kept,
                });
            }
        }
    }

    fn push_group_event(&mut self, e: super::commands::GroupEvent) {
        if self.group_events.len() >= super::commands::GROUP_EVENTS_KEPT {
            self.group_events.remove(0);
        }
        self.group_events.push(e);
    }

    /// Group orders that ended on their own since the last call (arrivals,
    /// emptied groups), oldest first.
    pub fn take_group_events(&mut self) -> Vec<super::commands::GroupEvent> {
        std::mem::take(&mut self.group_events)
    }

    /// `p` pulled inside `bound` (90% of the radius) if it lies outside.
    pub(crate) fn confine(&self, p: Vec2, bound: Option<(Vec2, f32)>) -> Vec2 {
        match bound {
            Some((c, r)) if p.dist(c) > r => self.map.clamp(c + (p - c).norm() * (0.9 * r)),
            _ => p,
        }
    }

    /// Largest territory radius a group of `adults` may claim.
    pub fn territory_radius_limit(adults: usize) -> f32 {
        (TERRITORY_BASE_RADIUS + TERRITORY_RADIUS_PER_ADULT * (adults as f32).sqrt())
            .clamp(TERRITORY_MIN_RADIUS, TERRITORY_MAX_RADIUS)
    }

    /// How long a group waits between territory claims (abandoning one in
    /// between changes nothing): [`CLAIM_COOLDOWN_DAYS`], or the host's
    /// floor in ticks ([`World::set_claim_cooldown_min_ticks`]) if longer.
    pub fn claim_cooldown_ticks(&self) -> u64 {
        ((CLAIM_COOLDOWN_DAYS * self.config.ticks_per_day as f32).round() as u64).max(self.claim_cooldown_min_ticks)
    }

    /// A floor on the claim cooldown in ticks: a server sets it to a few
    /// minutes of real time, since half a day passes in seconds at a fast
    /// tick. Not saved (the host sets it again after a restore).
    pub fn set_claim_cooldown_min_ticks(&mut self, ticks: u64) {
        self.claim_cooldown_min_ticks = ticks;
    }

    /// Water in and around the circle at `center`, `radius`, for the
    /// bodies of `player`'s `group` ([`WaterCheck`]).
    pub fn water_check(&self, player: PlayerId, group: u8, center: Vec2, radius: f32) -> WaterCheck {
        let bodies = self.group_bodies(player, group);
        let (w, h) = (self.map.width as i32, self.map.height as i32);
        let reach = radius + WATER_REACH;
        let (cx, cy) = (center.x.floor() as i32, center.y.floor() as i32);
        let span = reach.ceil() as i32 + 1;
        let (mut tiles, mut wet, mut drinkable) = (0usize, 0usize, 0usize);
        for y in (cy - span).max(0)..=(cy + span).min(h - 1) {
            for x in (cx - span).max(0)..=(cx + span).min(w - 1) {
                let d = crate::terrain::Map::tile_center(x as usize, y as usize).dist(center);
                if d > reach {
                    continue;
                }
                let k = self.map.idx(x as usize, y as usize);
                let stands = self.standable_tile(&bodies, k);
                // A drinking spot: ground the group stands on beside water
                // (or snow). Water behind a cliff or deep in the sea doesn't
                // count (playtest 6: 758 "drinkable" coastal tiles, and the
                // herd walked 15-20 tiles to a river).
                let beside_water = || {
                    ((y - 1).max(0)..=(y + 1).min(h - 1)).any(|ny| {
                        ((x - 1).max(0)..=(x + 1).min(w - 1))
                            .any(|nx| self.map.tiles[self.map.idx(nx as usize, ny as usize)].has_drinkable_water())
                    })
                };
                if stands && beside_water() {
                    drinkable += 1;
                }
                if d <= radius.max(0.5) {
                    tiles += 1;
                    wet += usize::from(!stands);
                }
            }
        }
        WaterCheck { water_share: if tiles == 0 { 0.0 } else { wet as f32 / tiles as f32 }, drinkable }
    }

    /// Plant food in the circle at `center`, `radius` against `player`'s
    /// `group`: what it feeds at this season's regrowth plus the plants
    /// standing now spread over [`FOOD_HORIZON_DAYS`], and how long what
    /// stands lasts ([`FoodCheck`]). Playtest 4 found the old measure (peak
    /// regrowth, 0.39 × capacity a day) 2–3× too hopeful. `None` for a group with no members, or one
    /// that lives mostly on meat (its food walks in and out).
    pub fn food_check(&self, player: PlayerId, group: u8, center: Vec2, radius: f32) -> Option<FoodCheck> {
        let bal = &self.config.balance;
        let (members, need) = self.plant_need(player, group)?;
        // What the group's bodies reach (browse above them doesn't feed it).
        let body = self.group_bodies(player, group).first().copied();
        let (w, h) = (self.map.width as i32, self.map.height as i32);
        let (cx, cy) = (center.x.floor() as i32, center.y.floor() as i32);
        let span = radius.ceil() as i32 + 1;
        let (mut yield_day, mut standing) = (0.0f32, 0.0f32);
        for y in (cy - span).max(0)..=(cy + span).min(h - 1) {
            for x in (cx - span).max(0)..=(cx + span).min(w - 1) {
                if crate::terrain::Map::tile_center(x as usize, y as usize).dist(center) > radius.max(0.5) {
                    continue;
                }
                let k = self.map.idx(x as usize, y as usize);
                let (g, t) = (&self.map.ground()[k], &self.map.tiles[k]);
                standing += match &body {
                    Some(b) => t.reachable_veg(g, b, bal.canopy_share),
                    None => t.veg,
                };
                if g.veg_max <= 0.0 || (g.plain > 0.0 && t.water_depth > 0.0) {
                    continue;
                }
                // Today's regrowth at today's plants, in this season's warmth,
                // snow cover and dryness (the terrain's own logistic rule):
                // a grazed tile grows back on its root term only.
                let warmth = crate::terrain::growth_warmth(g.base_temp + self.season_offset);
                let cover = (1.0 - t.snow * 3.0).clamp(0.0, 1.0);
                let wet = 1.0 - bal.rain_growth + bal.rain_growth * (t.wetness / 0.5).clamp(0.0, 1.4);
                let v = t.veg.min(g.veg_max);
                yield_day += bal.veg_regrowth * warmth * cover * wet * (v + 0.25 * g.veg_max) * (1.0 - v / g.veg_max);
            }
        }
        // Other plant eaters grazing the circle (wild herds, rivals, the
        // player's other groups) take their share (playtest 7: a destination
        // rated for 2,695 was grazed bare four days after arrival).
        let (others, others_need) = self.other_grazers(player, group, center, radius);
        let share = need / (need + others_need);
        let per_animal = need / members as f32;
        Some(FoodCheck {
            supports: ((yield_day + standing / FOOD_HORIZON_DAYS) * share / per_animal).floor() as usize,
            members,
            standing_days: standing / (need + others_need),
            regrowth: yield_day * share,
            others,
        })
    }

    /// Plant eaters in the circle at `center`, `radius` other than
    /// `player`'s `group`, and their daily plant need (no margin).
    fn other_grazers(&self, player: PlayerId, group: u8, center: Vec2, radius: f32) -> (usize, f32) {
        let bal = &self.config.balance;
        let r = radius.max(0.5);
        let mut n = 0usize;
        let mut need = 0.0f32;
        for o in self.organisms.iter() {
            if !o.alive || (o.owner == player && o.group == group) || o.traits.carnivory > 0.6 {
                continue;
            }
            if o.pos.dist(center) > r {
                continue;
            }
            let eff = o.traits.plant_efficiency(bal);
            if eff <= 0.0 {
                continue;
            }
            n += 1;
            need += self.steady_hunger_per_day(o) * o.food_per_hunger() / eff.max(0.05) * (1.0 - o.traits.carnivory);
        }
        (n, need)
    }

    /// A group's walk along `route`: about how many days it takes at the
    /// pace a group of its size keeps ([`World::group_pace`]), and how much
    /// of its plant food on the way the land along it holds (standing plants
    /// within [`JOURNEY_CORRIDOR`] tiles of the route, against the group's
    /// need for those days). `None` without members or a route.
    pub fn journey_check(&self, player: PlayerId, group: u8, route: &[Vec2]) -> Option<JourneyCheck> {
        if route.len() < 2 {
            return None;
        }
        let members = self.group_counts(player, group).0;
        if members == 0 {
            return None;
        }
        let tiles: f32 = route.windows(2).map(|p| p[0].dist(p[1])).sum();
        let days = tiles / Self::group_pace(members);
        // Standing plants along the way (each tile once).
        let mut seen = std::collections::BTreeSet::new();
        let r = JOURNEY_CORRIDOR as i32;
        for pair in route.windows(2) {
            let (a, b) = (pair[0], pair[1]);
            let steps = (a.dist(b) / 1.0).ceil().max(1.0) as usize;
            for k in 0..=steps {
                let p = a + (b - a) * (k as f32 / steps as f32);
                let (x, y) = self.map.tile_coords(p);
                for dy in -r..=r {
                    for dx in -r..=r {
                        let (nx, ny) = (x as i32 + dx, y as i32 + dy);
                        if nx >= 0 && ny >= 0 && (nx as usize) < self.map.width && (ny as usize) < self.map.height {
                            seen.insert(self.map.idx(nx as usize, ny as usize));
                        }
                    }
                }
            }
        }
        let standing: f32 = seen.iter().map(|&k| self.map.tiles[k].veg).sum();
        let food_share = match self.plant_need(player, group) {
            Some((_, need)) => (standing / (need * days.max(1.0))).min(10.0),
            None => 1.0,
        };
        Some(JourneyCheck { tiles, days, food_share, members })
    }

    /// A plant-eating group's members and their daily plant need (with
    /// [`FOOD_NEED_MARGIN`]); `None` without members or for a group that
    /// lives mostly on meat.
    pub fn plant_need(&self, player: PlayerId, group: u8) -> Option<(usize, f32)> {
        let bal = &self.config.balance;
        let (mut members, mut need, mut carnivory) = (0usize, 0.0f32, 0.0f32);
        for o in self.organisms.iter().filter(|o| o.alive && o.owner == player && o.group == group) {
            members += 1;
            carnivory += o.traits.carnivory;
            let eff = o.traits.plant_efficiency(bal).max(0.05);
            need += self.steady_hunger_per_day(o) * o.food_per_hunger() / eff;
        }
        if members == 0 || carnivory / members as f32 > 0.6 || need <= 0.0 {
            return None;
        }
        Some((members, need * FOOD_NEED_MARGIN))
    }

    /// Tiles a day a migrating group of `members` covers, fitted to what
    /// players saw: about 10 for a few dozen, 6 for 300, 3 for 1,000 (big
    /// herds stop to eat, drink and wait on each other).
    pub fn group_pace(members: usize) -> f32 {
        12.0 / (1.0 + members as f32 / 150.0).powf(0.7)
    }

    /// Tiles along `route` (each counted once) the group can't stand on:
    /// open water it would have to swim, or a cliff.
    pub fn route_water_tiles(&self, player: PlayerId, group: u8, route: &[Vec2]) -> usize {
        let bodies = self.group_bodies(player, group);
        // Fliers go over water.
        if !bodies.is_empty() && 2 * bodies.iter().filter(|b| b.can_fly).count() >= bodies.len() {
            return 0;
        }
        let mut seen = std::collections::BTreeSet::new();
        for pair in route.windows(2) {
            let (a, b) = (pair[0], pair[1]);
            let steps = (a.dist(b) / 0.5).ceil().max(1.0) as usize;
            for k in 0..=steps {
                let p = a + (b - a) * (k as f32 / steps as f32);
                let (x, y) = self.map.tile_coords(p);
                let i = self.map.idx(x, y);
                if !self.standable_tile(&bodies, i) {
                    seen.insert(i);
                }
            }
        }
        seen.len()
    }

    /// Refuse a circle mostly over water the group can't stand in.
    fn check_water(&self, player: PlayerId, group: u8, center: Vec2, radius: f32) -> Result<(), CommandError> {
        let share = self.water_check(player, group, center, radius).water_share;
        if share > MAX_WATER_SHARE {
            return Err(CommandError::MostlyWater { share });
        }
        Ok(())
    }

    /// Living agents of `player` in `group`: (members, adults).
    fn group_counts(&self, player: PlayerId, group: u8) -> (usize, usize) {
        self.organisms
            .iter()
            .filter(|o| o.alive && o.owner == player && o.group == group)
            .fold((0, 0), |(n, a), o| (n + 1, a + usize::from(o.matured)))
    }

    fn check_group(group: u8) -> Result<(), CommandError> {
        if group < MAX_GROUPS {
            Ok(())
        } else {
            Err(CommandError::NoSuchGroup(group))
        }
    }

    /// The player's groups from `orgs` (their living organisms, in any
    /// order): every group with members or a standing order, the main group
    /// always, by id.
    pub fn groups_of<'a>(&self, player: PlayerId, orgs: impl IntoIterator<Item = &'a Organism>) -> Vec<GroupInfo> {
        let mut counts = [(0usize, 0usize); MAX_GROUPS as usize];
        let mut obedience = [0f64; MAX_GROUPS as usize];
        for o in orgs {
            if o.alive && o.owner == player && o.group < MAX_GROUPS {
                let c = &mut counts[o.group as usize];
                c.0 += 1;
                c.1 += usize::from(o.matured);
                obedience[o.group as usize] += o.pheno[Gene::Obedience] as f64;
            }
        }
        let cooldown = self.claim_cooldown_ticks();
        (0..MAX_GROUPS)
            .filter_map(|g| {
                let key = (player, g);
                let (members, adults) = counts[g as usize];
                let migration = self.migrate_orders.get(&key).copied();
                let territory = self.territories.get(&key).copied();
                let rally = self.rallies.get(&key).copied().filter(|r| self.tick < r.expires_tick);
                if g != 0 && members == 0 && migration.is_none() && territory.is_none() && rally.is_none() {
                    return None;
                }
                let claim_ready_tick = self.territory_claims.get(&key).map_or(0, |&t| t + cooldown);
                let claim_ready_tick = if claim_ready_tick <= self.tick { 0 } else { claim_ready_tick };
                Some(GroupInfo {
                    id: g,
                    members,
                    adults,
                    migration,
                    territory,
                    rally,
                    max_radius: Self::territory_radius_limit(adults),
                    claim_ready_tick,
                    obedience: (obedience[g as usize] / members.max(1) as f64 * 100.0).round() as f32 / 100.0,
                    migration_deadline_tick: migration
                        .and_then(|_| self.group_plans.migrations.get(&key))
                        .map(|p| p.deadline_tick),
                    settle: self.settle_circle(player, g),
                    territory_away: self.group_plans.held.away.get(&key).copied(),
                })
            })
            .collect()
    }

    /// [`World::groups_of`] over every organism.
    pub fn groups(&self, player: PlayerId) -> Vec<GroupInfo> {
        self.groups_of(player, &self.organisms)
    }

    /// Every territory held, by (player, group): public ground, shown on
    /// everyone's map.
    /// A territory kept while its group migrates away counts
    /// ([`HeldPlans::away`]).
    pub fn all_territories(&self) -> impl Iterator<Item = (PlayerId, u8, &Territory)> {
        self.territories.iter().chain(self.group_plans.held.away.iter()).map(|(&(p, g), t)| (p, g, t))
    }

    /// The territory a migrating group left behind and still holds
    /// ([`HeldPlans::away`]).
    pub fn territory_away(&self, player: PlayerId, group: u8) -> Option<Territory> {
        self.group_plans.held.away.get(&(player, group)).copied()
    }

    /// Whether a group's territory hold is loosened for its starving or
    /// parched holders ([`HeldPlans::loosened`]).
    pub fn hold_loosened(&self, player: PlayerId, group: u8) -> bool {
        self.group_plans.held.loosened.contains(&(player, group))
    }

    /// The territory a group holds.
    pub fn territory(&self, player: PlayerId, group: u8) -> Option<Territory> {
        self.territories.get(&(player, group)).copied()
    }

    /// Group command: `group` holds the circle around `center`. The radius
    /// is kept within [`TERRITORY_MIN_RADIUS`] and what the group's adults
    /// can hold ([`World::territory_radius_limit`]); the territory as
    /// claimed is returned. Replaces the group's migration order (and any
    /// territory it held). Refused for a group without members, and within
    /// [`CLAIM_COOLDOWN_DAYS`] of the group's last claim. Ends a rally.
    pub fn claim_territory(
        &mut self,
        player: PlayerId,
        group: u8,
        center: Vec2,
        radius: f32,
    ) -> Result<Territory, CommandError> {
        Self::check_group(group)?;
        let (members, adults) = self.group_counts(player, group);
        if members == 0 {
            return Err(CommandError::NoSuchGroup(group));
        }
        let center = self.check_claim(player, group, center, radius)?;
        let key = (player, group);
        let t = Territory {
            center,
            radius: radius.clamp(TERRITORY_MIN_RADIUS, Self::territory_radius_limit(adults)),
            claimed_tick: self.tick,
        };
        self.check_water(player, group, t.center, t.radius)?;
        let squats = self.claim_squats(player, group, t.center, t.radius)?;
        self.displace_squats(&squats, player);
        self.territories.insert(key, t);
        self.territory_claims.insert(key, self.tick);
        self.migrate_orders.remove(&key);
        self.rallies.remove(&key);
        self.group_plans.pending.claims.remove(&key);
        self.group_plans.held.away.remove(&key);
        self.group_plans.held.loosened.remove(&key);
        // The group's home stays for when the territory is given up.
        Ok(t)
    }

    /// What a territory claim must pass, before its circle is checked for
    /// water: a positive radius, the claim cooldown (waived while the
    /// group's current territory is dry or grazed bare,
    /// [`World::territory_failing`]), a centre on the map and no overlap
    /// with another player's territory. The centre, on the map.
    fn check_claim(&self, player: PlayerId, group: u8, center: Vec2, radius: f32) -> Result<Vec2, CommandError> {
        if !(radius.is_finite() && radius > 0.0) {
            return Err(CommandError::BadRadius);
        }
        let key = (player, group);
        if let Some(&at) = self.territory_claims.get(&key) {
            let ready = at + self.claim_cooldown_ticks();
            let failing = self.territories.get(&key).is_some_and(|t| self.territory_failing(player, group, t));
            if self.tick < ready && !failing {
                return Err(CommandError::ClaimCooldown { ticks_left: ready - self.tick });
            }
        }
        let center = self.on_map(center)?;
        // The radius the claim would get, not the one typed: a radius of 300
        // used to find any rival territory on the map.
        let adults = self.group_counts(player, group).1;
        let r = radius.clamp(TERRITORY_MIN_RADIUS, Self::territory_radius_limit(adults));
        self.claim_squats(player, group, center, r)?;
        Ok(center)
    }

    /// The rival territories a claim at `center`, radius `r` overlaps, all
    /// of them squats `player`'s `group` may take (at most [`SQUAT_MEMBERS`]
    /// adults and [`SQUAT_RATIO`] times outnumbered, or holding
    /// [`OVERCLAIM_FOOD`] times the food they need and [`OVERCLAIM_RATIO`]
    /// times outnumbered); `Overlaps` if any other rival ground is in the way.
    fn claim_squats(
        &self,
        player: PlayerId,
        group: u8,
        center: Vec2,
        r: f32,
    ) -> Result<Vec<(PlayerId, u8)>, CommandError> {
        let mut squats = Vec::new();
        let mut mine = None;
        for (p, g, t) in self.all_territories() {
            if p == player || t.center.dist(center) >= t.radius + r {
                continue;
            }
            // Adults hold ground, not their young (playtest 8: a breeding
            // pair's litters took it past the squat size for good); and ground
            // far beyond what the holders eat is open to a group twice theirs.
            let (theirs, adults) = self.group_counts(p, g);
            let mine = *mine.get_or_insert_with(|| self.group_counts(player, group).0);
            let squat = adults <= SQUAT_MEMBERS && mine >= SQUAT_RATIO * adults.max(1);
            let overclaimed = mine >= OVERCLAIM_RATIO * theirs.max(1)
                && self
                    .food_check(p, g, t.center, t.radius)
                    .is_some_and(|f| f.supports >= OVERCLAIM_FOOD * theirs.max(1));
            // A newcomer in its grace keeps its ground (playtest 9: a large
            // group took over a graced newcomer's territory).
            let graced = (0..self.lineages.len() as u32)
                .any(|l| self.lineages[l as usize].owner == p && self.lineage_grace_ticks(l).is_some());
            if graced || !(squat || overclaimed) {
                return Err(CommandError::Overlaps);
            }
            squats.push((p, g));
        }
        Ok(squats)
    }

    /// Take the ground of `squats` ([`World::claim_squats`]): their
    /// territories (held or kept while away) are gone, and their owners told.
    fn displace_squats(&mut self, squats: &[(PlayerId, u8)], by: PlayerId) {
        for &key in squats {
            let t = self.territories.remove(&key).or_else(|| self.group_plans.held.away.remove(&key));
            self.group_plans.held.loosened.remove(&key);
            if let Some(t) = t {
                self.push_group_event(super::commands::GroupEvent::Displaced {
                    player: key.0,
                    group: key.1,
                    by,
                    territory: t,
                });
            }
        }
    }

    /// Whether a territory has no drinking water left, is grazed bare
    /// (standing plants for under [`TERRITORY_BARE_DAYS`]) or has been
    /// outgrown (it feeds fewer animals than the group has): the group may
    /// then move or enlarge it at once.
    pub fn territory_failing(&self, player: PlayerId, group: u8, t: &Territory) -> bool {
        self.water_check(player, group, t.center, t.radius).drinkable == 0
            || self
                .food_check(player, group, t.center, t.radius)
                .is_some_and(|f| f.standing_days < TERRITORY_BARE_DAYS || f.supports < f.members)
    }

    /// Group command: claim a territory, or, when its centre is more than
    /// [`FAR_CLAIM_TILES`] from the group, migrate there first and claim on
    /// arrival (the claim's checks run now; the circle is kept to the
    /// group's limit when it is claimed). A far claim used to become the
    /// group's territory (and home range) with nobody in it.
    pub fn claim_or_migrate(
        &mut self,
        player: PlayerId,
        group: u8,
        center: Vec2,
        radius: f32,
    ) -> Result<ClaimOutcome, CommandError> {
        Self::check_group(group)?;
        let Some(middle) = self.group_middle(player, group) else { return Err(CommandError::NoSuchGroup(group)) };
        let at = match self.check_claim(player, group, center, radius) {
            Ok(at) => at,
            Err(CommandError::Overlaps) => {
                // A refused claim over a rival's ground costs a short wait
                // (a quarter of the cooldown), not the whole one: territories
                // are shown on the map, so the refusal tells little.
                let key = (player, group);
                let cd = self.claim_cooldown_ticks();
                let short = (ABANDON_COOLDOWN_SHARE * cd as f32).round() as u64;
                let ready = self.territory_claims.get(&key).map_or(0, |&at| at + cd).max(self.tick + short);
                self.territory_claims.insert(key, ready.saturating_sub(cd));
                return Err(CommandError::Overlaps);
            }
            Err(e) => return Err(e),
        };
        if middle.dist(at) <= FAR_CLAIM_TILES {
            return self.claim_territory(player, group, center, radius).map(ClaimOutcome::Claimed);
        }
        let adults = self.group_counts(player, group).1;
        let r = radius.clamp(TERRITORY_MIN_RADIUS, Self::territory_radius_limit(adults));
        self.check_water(player, group, at, r)?;
        let target = self.command_migrate_group(player, group, at)?;
        self.group_plans.pending.claims.insert((player, group), (at, radius));
        // The wait runs from the order, not the arrival.
        self.territory_claims.insert((player, group), self.tick);
        Ok(ClaimOutcome::Migrating { target, radius: r })
    }

    /// Prey around a meat-eating group's circle, and how many of the group
    /// it can feed ([`PreyCheck`]; playtest 6: burrowed voles, poisonous
    /// herds and other predators all counted, and the warning came only
    /// under half a prey animal per hunter). `None` for a group that lives
    /// mostly on plants ([`World::food_check`] weighs those).
    pub fn prey_check(&self, player: PlayerId, group: u8, center: Vec2, radius: f32) -> Option<PreyCheck> {
        let mut members = 0usize;
        let (mut carnivory, mut need, mut cap) = (0.0f32, 0.0f32, 0.0f32);
        let mut sample = None;
        for o in self.organisms.iter().filter(|o| o.alive && o.owner == player && o.group == group) {
            members += 1;
            carnivory += o.traits.carnivory;
            // A day's hunger (`Balance::hunger_rate`) in food, as meat.
            need += self.config.balance.hunger_rate * o.food_per_hunger() / self.config.balance.meat_per_kg.max(1e-3);
            cap = cap.max(
                super::decide::max_prey_mass(o, self.coop_bonus(o))
                    * super::decide::led_pack_scale(&self.config.balance, o),
            );
            sample.get_or_insert(o);
        }
        let hunter = sample.filter(|_| members > 0 && carnivory / members as f32 > 0.6)?;
        // Poisonous or bright prey counts as far as a hungry hunter takes it
        // (`World::hunter_avoidance` at full hunger, `Balance::hungry_hunters`):
        // playtest 8's pack lived on a toxic herd while every check said
        // "feeds 0–2".
        let mut hungry = hunter.clone();
        hungry.needs.hunger = 100.0;
        let reach = radius + PREY_REACH;
        let (mut prey, mut mass) = (0usize, 0.0f32);
        for q in self.organisms.iter().filter(|q| q.alive && q.owner != player && q.pos.dist(center) <= reach) {
            let catchable = q.traits.carnivory <= super::decide::HUNTER_CARNIVORY
                && !q.sheltered
                && q.mass() < cap
                && !self.grace_spares(hunter, q);
            if !catchable {
                continue;
            }
            let taken = 1.0 - self.hunter_avoidance(&hungry, q).clamp(0.0, 1.0);
            if taken > PREY_TAKEN_MIN {
                prey += 1;
                mass += q.mass() * taken;
            }
        }
        let per_member = need / members as f32;
        let feeds = ((mass * PREY_HARVEST) / per_member.max(1e-6)).floor() as usize;
        Some(PreyCheck { prey, members, feeds })
    }

    /// The rally a group is answering.
    pub fn rally(&self, player: PlayerId, group: u8) -> Option<Rally> {
        self.rallies.get(&(player, group)).copied().filter(|r| self.tick < r.expires_tick)
    }

    /// Group command: `group` gathers at `center` (clamped to the map) and
    /// holds near it for [`RALLY_DAYS`]. The circle's radius comes from the
    /// group's size ([`World::rally_radius`]); the rally as given is
    /// returned. Replaces the group's migration order, territory or earlier
    /// rally. Refused for a group without members.
    pub fn rally_group(&mut self, player: PlayerId, group: u8, center: Vec2) -> Result<Rally, CommandError> {
        Self::check_group(group)?;
        let (members, _) = self.group_counts(player, group);
        if members == 0 {
            return Err(CommandError::NoSuchGroup(group));
        }
        let center = self.on_map(center)?;
        let r = Rally {
            center,
            radius: Self::rally_radius(members),
            issued_tick: self.tick,
            expires_tick: self.tick + self.rally_ticks(),
        };
        self.check_water(player, group, r.center, r.radius)?;
        let key = (player, group);
        self.rallies.insert(key, r);
        self.migrate_orders.remove(&key);
        self.territories.remove(&key);
        self.group_plans.pending.claims.remove(&key);
        // The group's home stays: cancelling the rally sends it back there.
        Ok(r)
    }

    /// Group command: `group` stops answering its rally.
    pub fn cancel_rally(&mut self, player: PlayerId, group: u8) -> Result<Rally, CommandError> {
        Self::check_group(group)?;
        let r = self.rallies.remove(&(player, group));
        r.filter(|r| self.tick < r.expires_tick).ok_or(CommandError::NoRally(group))
    }

    /// Give `group` back a territory it held before an order replaced it
    /// (the host's undo of a cancelled rally), unless another player's
    /// territory overlaps it now. Whether it was restored.
    pub fn restore_territory(&mut self, player: PlayerId, group: u8, t: Territory) -> bool {
        let clash =
            self.all_territories().any(|(p, _, o)| p != player && o.center.dist(t.center) < o.radius + t.radius);
        if clash || self.group_counts(player, group).0 == 0 {
            return false;
        }
        self.territories.insert((player, group), t);
        true
    }

    /// Group command: `group` gives up its territory.
    /// The claim cooldown left then shrinks to [`ABANDON_COOLDOWN_SHARE`] of
    /// a full one if that is sooner: a regretted claim can be replaced after
    /// a short wait (abandoning still doesn't reset it).
    pub fn abandon_territory(&mut self, player: PlayerId, group: u8) -> Result<Territory, CommandError> {
        Self::check_group(group)?;
        let key = (player, group);
        self.group_plans.held.loosened.remove(&key);
        let t = self
            .territories
            .remove(&key)
            .or_else(|| self.group_plans.held.away.remove(&key))
            .ok_or(CommandError::NoTerritory(group))?;
        if let Some(&at) = self.territory_claims.get(&key) {
            let cd = self.claim_cooldown_ticks();
            let short = (ABANDON_COOLDOWN_SHARE * cd as f32).round() as u64;
            let ready = (at + cd).min(self.tick + short);
            self.territory_claims.insert(key, ready.saturating_sub(cd));
        }
        Ok(t)
    }

    /// Group command: divide `group` in two. With `members`, those organisms
    /// (the player's, living, in `group`) form the new group; without, the
    /// group is cut across its widest spread (x or y, whichever varies more)
    /// and the far half (by coordinate, then id; the smaller half when the
    /// count is odd) forms it. The new group takes the lowest free id and
    /// starts with no orders; the old one keeps its own. At least one
    /// organism must stay behind.
    pub fn split_group(
        &mut self,
        player: PlayerId,
        group: u8,
        members: Option<&[OrgId]>,
    ) -> Result<SplitOutcome, CommandError> {
        Self::check_group(group)?;
        let in_group: Vec<usize> = (0..self.organisms.len())
            .filter(|&i| {
                let o = &self.organisms[i];
                o.alive && o.owner == player && o.group == group
            })
            .collect();
        if in_group.is_empty() {
            return Err(CommandError::NoSuchGroup(group));
        }
        let mut used = [false; MAX_GROUPS as usize];
        for o in self.organisms.iter().filter(|o| o.alive && o.owner == player) {
            if let Some(u) = used.get_mut(o.group as usize) {
                *u = true;
            }
        }
        let new = (0..MAX_GROUPS).find(|&g| !used[g as usize]).ok_or(CommandError::GroupLimit)?;
        let moving: Vec<usize> = match members {
            Some(ids) => {
                let mut idx = Vec::with_capacity(ids.len());
                let mut missing = None;
                for &id in ids {
                    // An animal that died since the list was made is skipped
                    // (only a list with nobody alive in it is refused).
                    let Ok(i) = self.live_index(id) else {
                        missing.get_or_insert(id);
                        continue;
                    };
                    let o = &self.organisms[i];
                    if o.owner != player {
                        return Err(CommandError::NotOwned(id));
                    }
                    if o.group != group {
                        return Err(CommandError::NotInGroup(id));
                    }
                    idx.push(i);
                }
                if let (true, Some(id)) = (idx.is_empty(), missing) {
                    return Err(CommandError::NotFound(id));
                }
                idx.sort_unstable();
                idx.dedup();
                idx
            }
            None => {
                let n = in_group.len() as f32;
                let mean = in_group.iter().fold(Vec2::ZERO, |s, &i| s + self.organisms[i].pos) * (1.0 / n);
                let (vx, vy) = in_group.iter().fold((0.0f32, 0.0f32), |(x, y), &i| {
                    let d = self.organisms[i].pos - mean;
                    (x + d.x * d.x, y + d.y * d.y)
                });
                let coord = |i: usize| if vy > vx { self.organisms[i].pos.y } else { self.organisms[i].pos.x };
                let mut order = in_group.clone();
                order.sort_by(|&a, &b| {
                    coord(a).total_cmp(&coord(b)).then(self.organisms[a].id.cmp(&self.organisms[b].id))
                });
                order.split_off(order.len() - order.len() / 2)
            }
        };
        if moving.is_empty() || moving.len() >= in_group.len() {
            return Err(CommandError::GroupTooSmall(group));
        }
        let key = (player, new);
        self.migrate_orders.remove(&key);
        self.territories.remove(&key);
        self.rallies.remove(&key);
        self.group_plans.settles.remove(&key);
        // The new half carries the group's claim cooldown: a split must not
        // reset it.
        match self.territory_claims.get(&(player, group)).copied() {
            Some(at) => self.territory_claims.insert(key, at),
            None => self.territory_claims.remove(&key),
        };
        for &i in &moving {
            self.organisms[i].group = new;
        }
        Ok(SplitOutcome { group: new, moved: moving.len(), kept: in_group.len() - moving.len() })
    }

    /// Group command: every member of `from` joins `into`. `from` loses its
    /// orders; `into` keeps its own (or, if it had no members, starts with
    /// none). Returns how many moved.
    pub fn merge_groups(&mut self, player: PlayerId, from: u8, into: u8) -> Result<usize, CommandError> {
        Self::check_group(from)?;
        Self::check_group(into)?;
        if from == into {
            return Err(CommandError::SameGroup(from));
        }
        let (members, _) = self.group_counts(player, from);
        if members == 0 {
            return Err(CommandError::NoSuchGroup(from));
        }
        if self.group_counts(player, into).0 == 0 {
            self.migrate_orders.remove(&(player, into));
            self.territories.remove(&(player, into));
            self.rallies.remove(&(player, into));
            self.group_plans.settles.remove(&(player, into));
        }
        for o in self.organisms.iter_mut().filter(|o| o.alive && o.owner == player && o.group == from) {
            o.group = into;
        }
        // The merged group keeps the later of the two claim cooldowns.
        if let Some(at) = self.territory_claims.get(&(player, from)).copied() {
            let e = self.territory_claims.entry((player, into)).or_insert(at);
            *e = (*e).max(at);
        }
        self.migrate_orders.remove(&(player, from));
        self.territories.remove(&(player, from));
        self.rallies.remove(&(player, from));
        self.group_plans.settles.remove(&(player, from));
        Ok(members)
    }
}
