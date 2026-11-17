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

use std::collections::BTreeMap;

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
            Some(t) => (t.center, t.radius),
            None => {
                let r = self.rally_of(o)?;
                (r.center, r.radius)
            }
        };
        (o.needs.hunger / 100.0 <= stray_hunger(o)).then_some((c, r))
    }

    /// Drop rally orders that have lapsed. Runs once per tick, before decide.
    pub(crate) fn settle_rallies(&mut self) {
        if !self.rallies.is_empty() {
            let now = self.tick;
            self.rallies.retain(|_, r| now < r.expires_tick);
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
    /// has arrived: [`MIGRATE_ARRIVAL`](super::commands::MIGRATE_ARRIVAL),
    /// or the circle the group would gather in at a rally if larger.
    pub fn arrival_radius(members: usize) -> f32 {
        Self::rally_radius(members).max(super::commands::MIGRATE_ARRIVAL)
    }

    /// Once per hour (and only with group orders standing): a migration
    /// most of whose group's adults (or members, with no adults) are within
    /// [`World::arrival_radius`] of its target is done, and a group with no
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
        // (members, adults, adults near the target, members near it).
        let mut tally: BTreeMap<(PlayerId, u8), [usize; 4]> = BTreeMap::new();
        for &k in self.migrate_orders.keys().chain(self.territories.keys()).chain(self.rallies.keys()) {
            tally.insert(k, [0; 4]);
        }
        for o in self.organisms.iter().filter(|o| o.alive) {
            if let Some(t) = tally.get_mut(&(o.owner, o.group)) {
                t[0] += 1;
                t[1] += usize::from(o.matured);
            }
        }
        for o in self.organisms.iter().filter(|o| o.alive) {
            let key = (o.owner, o.group);
            let Some(&target) = self.migrate_orders.get(&key) else { continue };
            let t = tally.get_mut(&key).expect("tallied");
            if o.pos.dist(target) <= Self::arrival_radius(t[0]) {
                t[2] += usize::from(o.matured);
                t[3] += 1;
            }
        }
        for (key, [members, adults, near_adults, near]) in tally {
            let (player, group) = key;
            let lineage_gone = || self.owner_count(player) == 0 && self.lod.owner_count(player) == 0;
            if members == 0 && (group != 0 || lineage_gone()) {
                self.migrate_orders.remove(&key);
                self.territories.remove(&key);
                self.rallies.remove(&key);
                self.push_group_event(GroupEvent::Emptied { player, group });
                continue;
            }
            let arrived = if adults > 0 { 2 * near_adults >= adults } else { members > 0 && 2 * near >= members };
            if arrived {
                if let Some(target) = self.migrate_orders.remove(&key) {
                    self.push_group_event(GroupEvent::Arrived { player, group, target });
                }
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

    fn claim_cooldown_ticks(&self) -> u64 {
        (CLAIM_COOLDOWN_DAYS * self.config.ticks_per_day as f32).round() as u64
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
                })
            })
            .collect()
    }

    /// [`World::groups_of`] over every organism.
    pub fn groups(&self, player: PlayerId) -> Vec<GroupInfo> {
        self.groups_of(player, &self.organisms)
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
        let key = (player, group);
        if let Some(&at) = self.territory_claims.get(&key) {
            let ready = at + self.claim_cooldown_ticks();
            if self.tick < ready {
                return Err(CommandError::ClaimCooldown { ticks_left: ready - self.tick });
            }
        }
        let center = self.on_map(center)?;
        let radius = if radius.is_finite() { radius } else { TERRITORY_MIN_RADIUS };
        let t = Territory {
            center,
            radius: radius.clamp(TERRITORY_MIN_RADIUS, Self::territory_radius_limit(adults)),
            claimed_tick: self.tick,
        };
        self.territories.insert(key, t);
        self.territory_claims.insert(key, self.tick);
        self.migrate_orders.remove(&key);
        self.rallies.remove(&key);
        Ok(t)
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
        let key = (player, group);
        self.rallies.insert(key, r);
        self.migrate_orders.remove(&key);
        self.territories.remove(&key);
        Ok(r)
    }

    /// Group command: `group` stops answering its rally.
    pub fn cancel_rally(&mut self, player: PlayerId, group: u8) -> Result<Rally, CommandError> {
        Self::check_group(group)?;
        let r = self.rallies.remove(&(player, group));
        r.filter(|r| self.tick < r.expires_tick).ok_or(CommandError::NoRally(group))
    }

    /// Group command: `group` gives up its territory.
    pub fn abandon_territory(&mut self, player: PlayerId, group: u8) -> Result<Territory, CommandError> {
        Self::check_group(group)?;
        self.territories.remove(&(player, group)).ok_or(CommandError::NoTerritory(group))
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
                for &id in ids {
                    let i = self.live_index(id)?;
                    let o = &self.organisms[i];
                    if o.owner != player {
                        return Err(CommandError::NotOwned(id));
                    }
                    if o.group != group {
                        return Err(CommandError::NotInGroup(id));
                    }
                    idx.push(i);
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
        Ok(members)
    }
}
